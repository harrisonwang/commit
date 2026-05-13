use crate::semantic::{ChangeKind, SymbolKind, Visibility};
use crate::{CommitType, GitContext, PriorAttempt, config};
use serde::Deserialize;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;

const SYSTEM_PROMPT: &str = r#"你是一位精通 Git 提交规范的专家，你的任务是写一条简短清晰、概括本次 staged diff 的 commit message。

本格式基于 Conventional Commits，但以以下项目约定为准：

- 必须使用指定 type
- subject 与 body 使用简体中文
- footer 沿用 Angular/Conventional Commits 规范

如果只用 subject 一行就能准确表达改动，那么 body 留空。仅在 body 能提供有用信息时才写 body。

body 中不要重复 subject 已经表达过的信息。

只返回 commit message 本身。不要包含任何关于任务的额外说明，不要把 diff 内容写进 commit message。输出必须直接以 type 开头，前面不要有任何空行或空白字符。

遵循良好的 Git 风格：

- subject 与 body 之间用一个空行分隔
- subject 行尽量控制在 50 个字符以内
- subject 末尾不加任何标点
- subject 使用简洁的中文动宾结构，例如“修复配置读取错误”“新增 MCP 工具入口”“重构消息解析器”
- body 每行不超过 72 个字符
- body 保持简短精炼（没有价值时整段省略）
- body 包含多个要点时，使用无序列表，每行以 "- " 开头

按照 Conventional Commits 组织 commit message：

<type>(<scope>): <subject>

<body>

<footer>

subject 与 body 必须使用简体中文。footer 沿用 Angular 规范：破坏性变更以 `BREAKING CHANGE:` 开头，issue 引用使用 `Closes #123` 或 `Fixes #456`。当 body 或 footer 没有有用信息时，整段省略。

如果有合适的 scope，scope 应使用英文模块名或功能名，例如 `cli`、`core`、`mcp`、`message`；没有合适 scope 时省略括号，写成 `<type>: <subject>`。

type 必须从以下选项中精确选择一项：

- init: 项目或模块初始化
- feat: 新功能
- fix: 修复 bug
- docs: 仅文档变更
- style: 不影响代码含义的改动（空白、格式化、缺少分号等）
- refactor: 既不修 bug 也不加功能的代码改动
- perf: 性能优化
- test: 补充缺失的测试或修正已有测试
- build: 影响构建系统的变更
- deps: 外部依赖、依赖锁定文件或依赖版本更新
- ci: CI 配置文件与脚本的变更
- chore: 其他不修改 src 或 test 文件的杂项改动
- revert: 回退之前的提交

优先使用上述固定 type。模块名、子系统名、目录名、包名或功能名应放进 scope，不要自造新的 type。例如写 `fix(runtime): ...`、`refactor(cli): ...`、`docs(config): ...`，不要写 `runtime: ...`、`cli: ...`、`config: ...`。"#;
const CRITIQUE_PROMPT: &str = "Critique this commit message. Prefer compact JSON: {\"verdict\":\"ok\",\"coverage\":\"complete\",\"risk\":\"low\",\"needs_feedback\":false,\"reason\":\"...\"}. Output REWRITE: followed by the issue when it must be rewritten.";

pub trait LlmBackend {
    fn invoke(&self, system_prompt: &str, prompt: &str) -> Result<String, String>;
}

pub struct CommandBackend {
    command: String,
    profile: Option<String>,
}

impl CommandBackend {
    pub fn from_config(config: &config::Config) -> Self {
        Self {
            command: config.llm_command.clone(),
            profile: config.llm_profile.clone(),
        }
    }
}

impl LlmBackend for CommandBackend {
    fn invoke(&self, system_prompt: &str, prompt: &str) -> Result<String, String> {
        invoke_command(
            &self.command,
            self.profile.as_deref(),
            system_prompt,
            prompt,
        )
    }
}

pub struct ExternalMessageBackend {
    message: Mutex<Option<String>>,
}

impl ExternalMessageBackend {
    pub fn new(message: String) -> Self {
        Self {
            message: Mutex::new(Some(message)),
        }
    }
}

impl LlmBackend for ExternalMessageBackend {
    fn invoke(&self, system_prompt: &str, _prompt: &str) -> Result<String, String> {
        if system_prompt == CRITIQUE_PROMPT {
            return Ok("OK".to_string());
        }
        self.message
            .lock()
            .map_err(|_| "external message backend lock poisoned".to_string())?
            .take()
            .ok_or_else(|| "external message already consumed".to_string())
    }
}

#[derive(Clone, Copy)]
pub struct GenerateOptions {
    pub critique: bool,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self { critique: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedMessage {
    pub message: String,
    pub judgement: LlmJudgement,
    pub rewritten: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmJudgement {
    pub coverage: Coverage,
    pub risk: Risk,
    pub needs_feedback: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coverage {
    Complete,
    Partial,
    Unclear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Risk {
    Low,
    Medium,
    High,
}

impl LlmJudgement {
    pub fn ok() -> Self {
        Self {
            coverage: Coverage::Complete,
            risk: Risk::Low,
            needs_feedback: false,
            reason: "critique passed".to_string(),
        }
    }
}

pub fn generate_checked_message(
    context: &GitContext,
    config: &config::Config,
    feedback: Option<&str>,
    prior_attempts: &[PriorAttempt],
    critique: bool,
) -> Result<GeneratedMessage, String> {
    let backend = CommandBackend::from_config(config);
    generate_checked_message_with_backend(
        context,
        feedback,
        prior_attempts,
        &backend,
        GenerateOptions { critique },
    )
}

pub fn generate_checked_message_with_backend(
    context: &GitContext,
    feedback: Option<&str>,
    prior_attempts: &[PriorAttempt],
    backend: &dyn LlmBackend,
    options: GenerateOptions,
) -> Result<GeneratedMessage, String> {
    let prompt = build_prompt_with_options(context, feedback, None, prior_attempts);
    let message = backend.invoke(SYSTEM_PROMPT, &prompt)?;
    if !options.critique {
        return Ok(GeneratedMessage {
            message,
            judgement: LlmJudgement::ok(),
            rewritten: false,
        });
    }
    let critique = critique_message(context, backend, &message)?;

    if let Some(issue) = critique.strip_prefix("REWRITE:") {
        let rewrite_prompt =
            build_prompt_with_options(context, feedback, Some(issue.trim()), prior_attempts);
        let message = backend.invoke(SYSTEM_PROMPT, &rewrite_prompt)?;
        Ok(GeneratedMessage {
            message,
            judgement: LlmJudgement {
                coverage: Coverage::Partial,
                risk: Risk::Medium,
                needs_feedback: false,
                reason: issue.trim().to_string(),
            },
            rewritten: true,
        })
    } else {
        Ok(GeneratedMessage {
            message,
            judgement: parse_judgement(&critique),
            rewritten: false,
        })
    }
}

fn critique_message(
    context: &GitContext,
    backend: &dyn LlmBackend,
    message: &str,
) -> Result<String, String> {
    let prompt = format!(
        "{}\n\nCandidate message:\n{}",
        build_prompt(context),
        message.trim()
    );
    backend.invoke(CRITIQUE_PROMPT, &prompt)
}

pub fn generate_message(context: &GitContext, config: &config::Config) -> Result<String, String> {
    let prompt = build_prompt(context);
    let backend = CommandBackend::from_config(config);
    backend.invoke(SYSTEM_PROMPT, &prompt)
}

fn invoke_command(
    command_name: &str,
    profile: Option<&str>,
    system_prompt: &str,
    prompt: &str,
) -> Result<String, String> {
    let mut command = Command::new(command_name);
    command.args(["--no-render", "--no-stream"]);
    if let Some(profile) = profile {
        command.args(["-p", profile]);
    }
    let mut child = command
        .args(["-s", system_prompt, prompt])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("failed to run llm: {error}"))?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| "failed to open llm stdin".to_string())?
        .write_all(prompt.as_bytes())
        .map_err(|error| format!("failed to write llm stdin: {error}"))?;

    let output = child
        .wait_with_output()
        .map_err(|error| format!("failed to wait for llm: {error}"))?;

    if output.status.success() {
        Ok(
            strip_thinking_blocks(&String::from_utf8_lossy(&output.stdout))
                .trim()
                .to_string(),
        )
    } else {
        Err(format!("llm exited with status {}", output.status))
    }
}

pub fn strip_thinking_blocks(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("<think>") {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + "<think>".len()..];
        let Some(end) = after_start.find("</think>") else {
            rest = "";
            break;
        };
        rest = &after_start[end + "</think>".len()..];
    }
    output.push_str(rest);
    output
}

#[derive(Debug, Deserialize)]
struct JsonJudgement {
    coverage: Option<String>,
    risk: Option<String>,
    needs_feedback: Option<bool>,
    reason: Option<String>,
}

pub fn parse_judgement(raw: &str) -> LlmJudgement {
    let trimmed = raw.trim();
    if trimmed == "OK" {
        return LlmJudgement::ok();
    }
    if let Ok(json) = serde_json::from_str::<JsonJudgement>(trimmed) {
        return LlmJudgement {
            coverage: match json.coverage.as_deref() {
                Some("complete") => Coverage::Complete,
                Some("partial") => Coverage::Partial,
                _ => Coverage::Unclear,
            },
            risk: match json.risk.as_deref() {
                Some("low") => Risk::Low,
                Some("medium") => Risk::Medium,
                Some("high") => Risk::High,
                _ => Risk::Medium,
            },
            needs_feedback: json.needs_feedback.unwrap_or(false),
            reason: json
                .reason
                .unwrap_or_else(|| "structured critique".to_string()),
        };
    }
    LlmJudgement {
        coverage: Coverage::Unclear,
        risk: Risk::Medium,
        needs_feedback: true,
        reason: trimmed.to_string(),
    }
}

pub fn build_prompt(context: &GitContext) -> String {
    build_prompt_with_options(context, None, None, &[])
}

fn render_symbol_change(symbol: &crate::semantic::SymbolChange) -> String {
    let marker = match symbol.change {
        ChangeKind::Added => "+",
        ChangeKind::Removed => "-",
        ChangeKind::BodyModified => "M",
        ChangeKind::SignatureChanged => "~",
    };
    let visibility = match symbol.visibility {
        Visibility::Public => "pub ",
        Visibility::PubCrate => "pub(crate) ",
        Visibility::Private => "",
    };
    let kind = match symbol.kind {
        SymbolKind::Function => "fn",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Union => "union",
        SymbolKind::Trait => "trait",
        SymbolKind::Impl => "impl",
        SymbolKind::Mod => "mod",
        SymbolKind::Const => "const",
        SymbolKind::Static => "static",
        SymbolKind::TypeAlias => "type",
        SymbolKind::Macro => "macro",
    };
    let change_label = match symbol.change {
        ChangeKind::Added => "added",
        ChangeKind::Removed => "removed",
        ChangeKind::BodyModified => "body modified",
        ChangeKind::SignatureChanged => "signature changed",
    };
    format!(
        "{marker} {visibility}{kind} {name} ({change_label})",
        name = symbol.name
    )
}

fn build_prompt_with_options(
    context: &GitContext,
    feedback: Option<&str>,
    critique: Option<&str>,
    prior_attempts: &[PriorAttempt],
) -> String {
    let mut prompt = format!(
        "Commit message rule:
Follow the system prompt's Conventional Commits format. Use the inferred type as a hint, but choose the final type/scope from the staged diff. Write subject and body in Simplified Chinese. Use module names as scope, not as custom types.

Branch:
{branch}

Status:
{status}

Staged stat:
{stat}

Staged name-status:
{name_status}

Inferred type:
{inferred_type}

Recent commits:
{recent}

Staged diff:
{diff}
",
        branch = context.branch.trim(),
        status = context.status_short.trim(),
        stat = context.staged_stat.trim(),
        name_status = context.staged_name_status.trim(),
        inferred_type = context
            .inferred_type
            .map(CommitType::as_str)
            .unwrap_or("unknown"),
        recent = context.recent_subjects.join("\n"),
        diff = context.staged_diff,
    );
    if let Some(semantic) = &context.semantic
        && !semantic.files.is_empty()
    {
        prompt.push_str("\nSemantic changes (top-level symbols):\n");
        for file in &semantic.files {
            prompt.push_str(&format!("\n{}:\n", file.path));
            for symbol in &file.symbols {
                prompt.push_str(&format!("  {}\n", render_symbol_change(symbol)));
            }
        }
    }
    if !prior_attempts.is_empty() {
        prompt.push_str(
            "\nSession attempts so far (do not repeat the wording from earlier attempts; respond to the latest user feedback below):\n",
        );
        for (index, attempt) in prior_attempts.iter().enumerate() {
            let number = index + 1;
            prompt.push_str(&format!("\nAttempt {number}:\n"));
            prompt.push_str(attempt.message.trim());
            prompt.push('\n');
            if attempt.feedback.is_empty() {
                prompt.push_str(&format!(
                    "User feedback after attempt {number}: (no specific feedback, just regenerate)\n"
                ));
            } else {
                prompt.push_str(&format!(
                    "User feedback after attempt {number}: {}\n",
                    attempt.feedback
                ));
            }
        }
    }
    if let Some(feedback) = feedback {
        prompt.push_str("\nUser feedback:\n");
        prompt.push_str(feedback);
        prompt.push('\n');
    }
    if let Some(critique) = critique {
        prompt.push_str("\nCritique to address:\n");
        prompt.push_str(critique);
        prompt.push('\n');
    }
    if !context.history.is_empty() {
        prompt.push_str("\nRecent generated/final pairs:\n");
        for entry in &context.history {
            prompt.push_str("AI draft: ");
            prompt.push_str(&entry.generated);
            prompt.push_str("\nUser final: ");
            prompt.push_str(&entry.final_message);
            prompt.push('\n');
        }
    }
    prompt
}
