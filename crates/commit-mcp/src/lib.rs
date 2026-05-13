use commit_core::{ExecuteOptions, PrepareOutput, execute_commit_message, prepare_commit_context};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

const PROTOCOL_VERSION: &str = "2025-11-25";
const ENABLE_FALLBACK_TOOLS_ENV: &str = "COMMIT_MCP_ENABLE_FALLBACK_TOOLS";
const COMMIT_SYSTEM_PROMPT: &str = r#"You are a Git commit message expert.

Return exactly one Conventional Commit message and nothing else.

Rules:
- Use an English type/scope prefix.
- Use Simplified Chinese for subject and body.
- Do not wrap the message in Markdown.
- Do not include explanations.
- Omit the body when the subject is enough.
- Use body/footer only when they add useful information."#;

#[derive(Debug, Deserialize)]
pub struct PrepareCommitRequest {
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteCommitRequest {
    pub cwd: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct CommitStagedRequest {
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct CommitStagedOutput {
    pub committed: bool,
    pub action: String,
    pub generated_message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execute: Option<commit_core::ExecuteOutput>,
}

#[derive(Clone, Debug, Default)]
struct ClientCapabilities {
    sampling: bool,
    elicitation: bool,
}

pub fn prepare_commit(
    default_cwd: &Path,
    request: PrepareCommitRequest,
) -> Result<PrepareOutput, String> {
    let cwd = request.cwd.unwrap_or_else(|| default_cwd.to_path_buf());
    prepare_commit_context(&cwd)
}

pub fn execute_commit(
    default_cwd: &Path,
    request: ExecuteCommitRequest,
) -> Result<commit_core::ExecuteOutput, String> {
    let cwd = request.cwd.unwrap_or_else(|| default_cwd.to_path_buf());
    execute_commit_message(
        &cwd,
        ExecuteOptions {
            message: request.message,
            generated_message: None,
        },
    )
}

pub fn serve_stdio(default_cwd: &Path) -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut session = Session::new(default_cwd.to_path_buf(), stdin.lock(), stdout.lock());
    session.run()
}

struct Session<R, W> {
    default_cwd: PathBuf,
    reader: R,
    writer: W,
    next_request_id: u64,
    client_capabilities: ClientCapabilities,
}

impl<R, W> Session<R, W>
where
    R: BufRead,
    W: Write,
{
    fn new(default_cwd: PathBuf, reader: R, writer: W) -> Self {
        Self {
            default_cwd,
            reader,
            writer,
            next_request_id: 1,
            client_capabilities: ClientCapabilities::default(),
        }
    }

    fn run(&mut self) -> Result<(), String> {
        while let Some(value) = self.read_json()? {
            if let Some(response) = self.handle_json(value) {
                self.write_message(&response)?;
            }
        }
        Ok(())
    }

    fn read_json(&mut self) -> Result<Option<Value>, String> {
        loop {
            let mut line = String::new();
            let bytes = self
                .reader
                .read_line(&mut line)
                .map_err(|error| format!("failed to read MCP stdin: {error}"))?;
            if bytes == 0 {
                return Ok(None);
            }
            if line.trim().is_empty() {
                continue;
            }
            return serde_json::from_str::<Value>(&line)
                .map(Some)
                .map_err(|error| format!("failed to parse MCP message: {error}"));
        }
    }

    fn handle_json(&mut self, value: Value) -> Option<Value> {
        if let Value::Array(messages) = value {
            let responses = messages
                .into_iter()
                .filter_map(|message| self.handle_single(message))
                .collect::<Vec<_>>();
            return (!responses.is_empty()).then_some(Value::Array(responses));
        }
        self.handle_single(value)
    }

    fn handle_single(&mut self, value: Value) -> Option<Value> {
        let Some(object) = value.as_object() else {
            return Some(jsonrpc_error(
                Value::Null,
                -32600,
                "Invalid Request",
                Some(json!("message must be a JSON object")),
            ));
        };

        let id = object.get("id").cloned();
        let method = object.get("method").and_then(Value::as_str);
        let params = object.get("params").cloned().unwrap_or_else(|| json!({}));

        let Some(method) = method else {
            return id.map(|id| {
                jsonrpc_error(id, -32600, "Invalid Request", Some(json!("missing method")))
            });
        };

        let id = id?;

        let result = match method {
            "initialize" => self.initialize_result(&params),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(tools_list()),
            "tools/call" => self.call_tool(params),
            "prompts/list" => Ok(prompts_list()),
            "prompts/get" => get_prompt(params),
            _ => Err(McpError {
                code: -32601,
                message: "Method not found".to_string(),
                data: Some(json!(method)),
            }),
        };

        Some(match result {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
            Err(error) => jsonrpc_error(id, error.code, &error.message, error.data),
        })
    }

    fn initialize_result(&mut self, params: &Value) -> Result<Value, McpError> {
        self.client_capabilities = parse_client_capabilities(params);
        let protocol_version = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(PROTOCOL_VERSION);

        Ok(json!({
            "protocolVersion": protocol_version,
            "capabilities": {
                "tools": { "listChanged": false },
                "prompts": { "listChanged": false }
            },
            "serverInfo": {
                "name": "commit",
                "title": "commit",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "Use commit_staged for staged Git changes. The server will request sampling/createMessage to generate a Conventional Commit message, then elicitation/create to let the user review or edit it before executing git commit -F. Do not fall back to a multi-tool commit flow unless debug fallback tools are explicitly exposed."
        }))
    }

    fn call_tool(&mut self, params: Value) -> Result<Value, McpError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::invalid_params("tools/call params.name is required"))?
            .to_string();
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        match name.as_str() {
            "commit_staged" => {
                let request = parse_arguments::<CommitStagedRequest>(arguments)?;
                tool_result(self.commit_staged(request))
            }
            "prepare_commit" => {
                ensure_fallback_tools_enabled()?;
                let request = parse_arguments::<PrepareCommitRequest>(arguments)?;
                tool_result(prepare_commit(&self.default_cwd, request))
            }
            "execute_commit" => {
                ensure_fallback_tools_enabled()?;
                let request = parse_arguments::<ExecuteCommitRequest>(arguments)?;
                tool_result(execute_commit(&self.default_cwd, request))
            }
            _ => Err(McpError::invalid_params(format!("unknown tool `{name}`"))),
        }
    }

    fn commit_staged(
        &mut self,
        request: CommitStagedRequest,
    ) -> Result<CommitStagedOutput, String> {
        if !self.client_capabilities.sampling {
            return Err("client did not declare MCP sampling capability".to_string());
        }
        if !self.client_capabilities.elicitation {
            return Err("client did not declare MCP elicitation capability".to_string());
        }

        let cwd = request.cwd.unwrap_or_else(|| self.default_cwd.clone());
        let context = prepare_commit_context(&cwd)?;
        let generated_message = self.sample_commit_message(&context, None)?;
        let review = self.elicit_commit_confirmation(&generated_message)?;

        if review.action != "accept" {
            return Ok(CommitStagedOutput {
                committed: false,
                action: review.action,
                generated_message,
                final_message: None,
                execute: None,
            });
        }
        if !review.confirm {
            return Ok(CommitStagedOutput {
                committed: false,
                action: "decline".to_string(),
                generated_message,
                final_message: review.message,
                execute: None,
            });
        }

        let final_message = review
            .message
            .filter(|message| !message.trim().is_empty())
            .unwrap_or_else(|| generated_message.clone());
        let execute = execute_commit_message(
            &cwd,
            ExecuteOptions {
                message: final_message.clone(),
                generated_message: Some(generated_message.clone()),
            },
        )?;

        Ok(CommitStagedOutput {
            committed: true,
            action: "accept".to_string(),
            generated_message,
            final_message: Some(final_message),
            execute: Some(execute),
        })
    }

    fn sample_commit_message(
        &mut self,
        context: &PrepareOutput,
        validation_feedback: Option<&str>,
    ) -> Result<String, String> {
        let context_json = serde_json::to_string_pretty(context)
            .map_err(|error| format!("failed to serialize commit context: {error}"))?;
        let mut prompt = format!(
            "Generate exactly one commit message for the staged Git changes below.\n\nCommit context JSON:\n{context_json}"
        );
        if let Some(feedback) = validation_feedback {
            prompt.push_str("\n\nPrevious candidate failed validation:\n");
            prompt.push_str(feedback);
        }

        let result = self
            .request_client(
                "sampling/createMessage",
                json!({
                    "messages": [
                        {
                            "role": "user",
                            "content": {
                                "type": "text",
                                "text": prompt
                            }
                        }
                    ],
                    "systemPrompt": COMMIT_SYSTEM_PROMPT,
                    "modelPreferences": {
                        "costPriority": 0.2,
                        "speedPriority": 0.5,
                        "intelligencePriority": 0.7
                    },
                    "maxTokens": 512
                }),
            )
            .map_err(|error| format!("sampling request failed: {error}"))?;

        extract_text_content(&result)
            .map(|message| {
                commit_core::llm::strip_thinking_blocks(&message)
                    .trim()
                    .to_string()
            })
            .filter(|message| !message.is_empty())
            .ok_or_else(|| "sampling response did not contain text content".to_string())
    }

    fn elicit_commit_confirmation(
        &mut self,
        generated_message: &str,
    ) -> Result<ElicitationReview, String> {
        let result = self.request_client(
            "elicitation/create",
            json!({
                "message": "Review the generated commit message. Edit it if needed, then confirm whether to create the commit.",
                "requestedSchema": {
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "title": "Commit message",
                            "description": "Complete commit message to use with git commit -F.",
                            "default": generated_message
                        },
                        "confirm": {
                            "type": "boolean",
                            "title": "Create commit",
                            "description": "When true, commit will validate and create the Git commit.",
                            "default": true
                        }
                    },
                    "required": ["message", "confirm"]
                }
            }),
        )
        .map_err(|error| format!("elicitation request failed: {error}"))?;

        parse_elicitation_review(result)
    }

    fn request_client(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = Value::String(format!("commit-{}", self.next_request_id));
        self.next_request_id += 1;
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;

        loop {
            let message = self
                .read_json()?
                .ok_or_else(|| format!("client closed connection while waiting for {method}"))?;
            if let Some(result) = response_for_id(&message, &id) {
                return result;
            }
            if let Some(response) = self.busy_response(message) {
                self.write_message(&response)?;
            }
        }
    }

    fn busy_response(&self, value: Value) -> Option<Value> {
        let object = value.as_object()?;
        let id = object.get("id")?.clone();
        object.get("method")?;
        Some(jsonrpc_error(
            id,
            -32000,
            "Server is waiting for a client response",
            None,
        ))
    }

    fn write_message(&mut self, value: &Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.writer, value)
            .map_err(|error| format!("failed to serialize MCP response: {error}"))?;
        self.writer
            .write_all(b"\n")
            .map_err(|error| format!("failed to write MCP response: {error}"))?;
        self.writer
            .flush()
            .map_err(|error| format!("failed to flush MCP response: {error}"))
    }
}

#[derive(Debug)]
struct ElicitationReview {
    action: String,
    message: Option<String>,
    confirm: bool,
}

fn parse_client_capabilities(params: &Value) -> ClientCapabilities {
    let capabilities = params.get("capabilities").unwrap_or(&Value::Null);
    let sampling = capabilities.get("sampling").is_some();
    let elicitation = capabilities.get("elicitation");
    let elicitation = elicitation
        .and_then(Value::as_object)
        .map(|object| object.is_empty() || object.contains_key("form"))
        .unwrap_or(false);

    ClientCapabilities {
        sampling,
        elicitation,
    }
}

fn response_for_id(value: &Value, id: &Value) -> Option<Result<Value, String>> {
    let object = value.as_object()?;
    if object.get("id") != Some(id) {
        return None;
    }
    if let Some(result) = object.get("result") {
        return Some(Ok(result.clone()));
    }
    let error = object.get("error").cloned().unwrap_or_else(|| json!({}));
    Some(Err(format!("client request failed: {error}")))
}

fn extract_text_content(result: &Value) -> Option<String> {
    let content = result.get("content")?;
    if let Some(text) = text_content(content) {
        return Some(text.to_string());
    }
    let array = content.as_array()?;
    let text = array
        .iter()
        .filter_map(text_content)
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}

fn text_content(value: &Value) -> Option<&str> {
    if value.get("type").and_then(Value::as_str) == Some("text") {
        return value.get("text").and_then(Value::as_str);
    }
    None
}

fn parse_elicitation_review(value: Value) -> Result<ElicitationReview, String> {
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| "elicitation response missing action".to_string())?
        .to_string();
    let content = value.get("content").cloned().unwrap_or_else(|| json!({}));
    let message = content
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string);
    let confirm = content
        .get("confirm")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    Ok(ElicitationReview {
        action,
        message,
        confirm,
    })
}

fn tools_list() -> Value {
    let mut tools = vec![commit_staged_tool()];
    if fallback_tools_enabled() {
        tools.push(prepare_commit_tool());
        tools.push(execute_commit_tool());
    }

    json!({ "tools": tools })
}

fn commit_staged_tool() -> Value {
    json!({
        "name": "commit_staged",
        "title": "Commit Staged Changes",
        "description": "Default commit flow. Use this single tool call to collect staged context, request sampling/createMessage for a commit message, request elicitation/create for user review, then validate and create the commit.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "cwd": {
                    "type": "string",
                    "description": "Repository working directory. Defaults to the MCP server process cwd."
                }
            }
        }
    })
}

fn prepare_commit_tool() -> Value {
    json!({
        "name": "prepare_commit",
        "title": "Prepare Commit Context",
        "description": "Debug fallback tool. Collect staged Git context for a commit message without calling Sampling or creating a commit.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "cwd": {
                    "type": "string",
                    "description": "Repository working directory. Defaults to the MCP server process cwd."
                }
            }
        }
    })
}

fn execute_commit_tool() -> Value {
    json!({
        "name": "execute_commit",
        "title": "Execute Commit",
        "description": "Debug fallback tool. Validate a supplied commit message and run git commit -F. Prefer commit_staged.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "cwd": {
                    "type": "string",
                    "description": "Repository working directory. Defaults to the MCP server process cwd."
                },
                "message": {
                    "type": "string",
                    "description": "Complete commit message generated by the host Agent model."
                }
            },
            "required": ["message"]
        }
    })
}

fn fallback_tools_enabled() -> bool {
    std::env::var(ENABLE_FALLBACK_TOOLS_ENV)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn ensure_fallback_tools_enabled() -> Result<(), McpError> {
    if fallback_tools_enabled() {
        return Ok(());
    }

    Err(McpError::invalid_params(format!(
        "fallback commit tools are disabled; use commit_staged or set {ENABLE_FALLBACK_TOOLS_ENV}=1 for debugging"
    )))
}

fn prompts_list() -> Value {
    json!({
        "prompts": [
            {
                "name": "commit_staged",
                "title": "Commit Staged Changes",
                "description": "Commit staged changes with one MCP tool call when Sampling and Elicitation are available.",
                "arguments": [
                    {
                        "name": "cwd",
                        "description": "Repository working directory. Optional.",
                        "required": false
                    }
                ]
            }
        ]
    })
}

fn get_prompt(params: Value) -> Result<Value, McpError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::invalid_params("prompts/get params.name is required"))?;
    if name != "commit_staged" {
        return Err(McpError::invalid_params(format!("unknown prompt `{name}`")));
    }
    let cwd = params
        .get("arguments")
        .and_then(|arguments| arguments.get("cwd"))
        .and_then(Value::as_str);
    let cwd_line = cwd
        .map(|cwd| format!("Pass cwd `{cwd}` to commit_staged. "))
        .unwrap_or_default();
    Ok(json!({
        "description": "Commit staged Git changes using commit MCP.",
        "messages": [
            {
                "role": "user",
                "content": {
                    "type": "text",
                    "text": format!("{cwd_line}Call the commit_staged tool once. The MCP server will request Sampling to generate the commit message and Elicitation to let the user review or edit it before creating the commit.")
                }
            }
        ]
    }))
}

fn parse_arguments<T>(value: Value) -> Result<T, McpError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(value)
        .map_err(|error| McpError::invalid_params(format!("invalid tool arguments: {error}")))
}

fn tool_result<T>(result: Result<T, String>) -> Result<Value, McpError>
where
    T: Serialize,
{
    match result {
        Ok(output) => {
            let structured = serde_json::to_value(&output).map_err(|error| McpError {
                code: -32603,
                message: "Internal error".to_string(),
                data: Some(json!(error.to_string())),
            })?;
            let text = serde_json::to_string_pretty(&structured).map_err(|error| McpError {
                code: -32603,
                message: "Internal error".to_string(),
                data: Some(json!(error.to_string())),
            })?;
            Ok(json!({
                "content": [{ "type": "text", "text": text }],
                "structuredContent": structured,
                "isError": false
            }))
        }
        Err(error) => Ok(json!({
            "content": [{ "type": "text", "text": error }],
            "structuredContent": { "error": error },
            "isError": true
        })),
    }
}

#[derive(Debug)]
struct McpError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl McpError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }
}

fn jsonrpc_error(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({
        "code": code,
        "message": message,
    });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error,
    })
}

#[cfg(test)]
mod tests {
    use super::{ENABLE_FALLBACK_TOOLS_ENV, PROTOCOL_VERSION, Session};
    use serde_json::{Value, json};
    use std::fs;
    use std::io::Cursor;
    use std::path::Path;
    use std::process::Command;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn initialize_declares_tools_and_prompts_without_server_sampling_capability() {
        let output = run_session(
            Path::new("."),
            vec![json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {
                        "sampling": {},
                        "elicitation": {}
                    },
                    "clientInfo": { "name": "test", "version": "1" }
                }
            })],
        );
        let response = parse_lines(&output);
        let result = &response[0]["result"];
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert!(result["capabilities"]["tools"].is_object());
        assert!(result["capabilities"]["prompts"].is_object());
        assert!(result["capabilities"]["sampling"].is_null());
        assert!(result["capabilities"]["elicitation"].is_null());
    }

    #[test]
    fn tools_list_exposes_only_one_call_commit_by_default() {
        let output = {
            let _env_lock = lock_env();
            let _fallback_env = EnvVarGuard::remove(ENABLE_FALLBACK_TOOLS_ENV);
            run_session(
                Path::new("."),
                vec![json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/list"
                })],
            )
        };

        let response = parse_lines(&output);
        let tools = response[0]["result"]["tools"]
            .as_array()
            .expect("tools array");
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["commit_staged"]);

        let commit_staged = tools
            .iter()
            .find(|tool| tool["name"] == "commit_staged")
            .expect("commit_staged tool");
        assert!(
            commit_staged["description"]
                .as_str()
                .expect("description")
                .contains("sampling/createMessage")
        );
    }

    #[test]
    fn tools_list_exposes_fallback_tools_only_when_enabled() {
        let output = {
            let _env_lock = lock_env();
            let _fallback_env = EnvVarGuard::set(ENABLE_FALLBACK_TOOLS_ENV, "1");
            run_session(
                Path::new("."),
                vec![json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/list"
                })],
            )
        };

        let response = parse_lines(&output);
        let tools = response[0]["result"]["tools"]
            .as_array()
            .expect("tools array");
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec!["commit_staged", "prepare_commit", "execute_commit"]
        );
    }

    #[test]
    fn fallback_tool_calls_are_rejected_by_default() {
        let output = {
            let _env_lock = lock_env();
            let _fallback_env = EnvVarGuard::remove(ENABLE_FALLBACK_TOOLS_ENV);
            run_session(
                Path::new("."),
                vec![json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {
                        "name": "execute_commit",
                        "arguments": {
                            "message": "feat: 测试提交"
                        }
                    }
                })],
            )
        };

        let response = parse_lines(&output);
        assert_eq!(response[0]["id"], 2);
        assert_eq!(response[0]["error"]["code"], -32602);
        assert!(
            response[0]["error"]["message"]
                .as_str()
                .expect("error message")
                .contains("fallback commit tools are disabled")
        );
    }

    #[test]
    fn initialized_notification_has_no_response() {
        let output = run_session(
            Path::new("."),
            vec![json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            })],
        );

        assert!(output.trim().is_empty());
    }

    #[test]
    fn commit_staged_uses_sampling_and_elicitation_then_commits() {
        let _env_lock = lock_env();
        let commit_home = tempfile::tempdir().expect("commit home");
        let _commit_home_env = EnvVarGuard::set_os("COMMIT_HOME", commit_home.path());
        let repo = tempfile::tempdir().expect("repo");
        run_git(repo.path(), &["init"]);
        run_git(repo.path(), &["config", "user.name", "Test User"]);
        run_git(repo.path(), &["config", "user.email", "test@example.com"]);
        fs::write(repo.path().join("example.txt"), "hello\n").expect("write file");
        run_git(repo.path(), &["add", "example.txt"]);

        let output = run_session(
            repo.path(),
            vec![
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": PROTOCOL_VERSION,
                        "capabilities": {
                            "sampling": {},
                            "elicitation": {}
                        },
                        "clientInfo": { "name": "test", "version": "1" }
                    }
                }),
                json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {
                        "name": "commit_staged",
                        "arguments": {}
                    }
                }),
                json!({
                    "jsonrpc": "2.0",
                    "id": "commit-1",
                    "result": {
                        "role": "assistant",
                        "content": {
                            "type": "text",
                            "text": "feat: 添加示例文件"
                        },
                        "model": "test",
                        "stopReason": "endTurn"
                    }
                }),
                json!({
                    "jsonrpc": "2.0",
                    "id": "commit-2",
                    "result": {
                        "action": "accept",
                        "content": {
                            "message": "feat(test): 添加示例文件",
                            "confirm": true
                        }
                    }
                }),
            ],
        );

        let lines = parse_lines(&output);
        assert_eq!(lines[1]["method"], "sampling/createMessage");
        assert_eq!(lines[2]["method"], "elicitation/create");
        assert!(lines[2]["params"]["mode"].is_null());
        assert_eq!(lines[3]["id"], 2);
        assert_eq!(
            lines[3]["result"]["structuredContent"]["final_message"],
            "feat(test): 添加示例文件"
        );
        assert_eq!(commit_subject(repo.path()), "feat(test): 添加示例文件");
    }

    fn run_session(cwd: &Path, messages: Vec<Value>) -> String {
        let input = messages
            .into_iter()
            .map(|message| serde_json::to_string(&message).expect("serialize"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let reader = Cursor::new(input.into_bytes());
        let mut writer = Vec::new();
        {
            let mut session = Session::new(cwd.to_path_buf(), reader, &mut writer);
            session.run().expect("session run");
        }
        String::from_utf8(writer).expect("utf8")
    }

    fn parse_lines(output: &str) -> Vec<Value> {
        output
            .lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect()
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner())
    }

    struct EnvVarGuard {
        name: &'static str,
        old_value: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(name: &'static str, value: &str) -> Self {
            let guard = Self::save(name);
            unsafe {
                std::env::set_var(name, value);
            }
            guard
        }

        fn set_os(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let guard = Self::save(name);
            unsafe {
                std::env::set_var(name, value);
            }
            guard
        }

        fn remove(name: &'static str) -> Self {
            let guard = Self::save(name);
            unsafe {
                std::env::remove_var(name);
            }
            guard
        }

        fn save(name: &'static str) -> Self {
            Self {
                name,
                old_value: std::env::var_os(name),
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(value) = &self.old_value {
                    std::env::set_var(self.name, value);
                } else {
                    std::env::remove_var(self.name);
                }
            }
        }
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit_subject(cwd: &Path) -> String {
        let output = Command::new("git")
            .args(["log", "-1", "--pretty=%s"])
            .current_dir(cwd)
            .output()
            .expect("run git log");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}
