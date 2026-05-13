use crate::{
    CommitType, GitContext, PriorAttempt, confidence, config, git, history, llm, message, planner,
    semantic::{self, SemanticAnalysis},
};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct PrepareOutput {
    pub branch: String,
    pub status_short: String,
    pub staged_stat: String,
    pub staged_name_status: String,
    pub inferred_type: Option<String>,
    pub recent_subjects: Vec<String>,
    pub staged_diff: String,
    pub warnings: Vec<String>,
    pub groups: Vec<planner::FileGroup>,
    pub message_policy: MessagePolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic: Option<SemanticAnalysis>,
}

#[derive(Debug, Serialize)]
pub struct MessagePolicy {
    pub format: &'static str,
    pub language: &'static str,
    pub allowed_types: Vec<String>,
    pub max_subject_chars: usize,
    pub scope_examples: Vec<&'static str>,
}

#[derive(Debug)]
pub struct ExecuteOptions {
    pub message: String,
    pub generated_message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExecuteOutput {
    pub committed: bool,
    pub message: String,
    pub confidence: confidence::ConfidenceReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    pub post_status_short: String,
}

#[derive(Debug, Default)]
pub struct TtyGenerateOptions {
    pub feedback: Option<String>,
    pub prior_attempts: Vec<PriorAttempt>,
}

#[derive(Debug, Serialize)]
pub struct TtyGenerateOutput {
    pub message: String,
    pub confidence: confidence::ConfidenceReport,
}

pub fn prepare_commit_context(cwd: &Path) -> Result<PrepareOutput, String> {
    let config = config::Config::load()?;
    let context = collect_context(&config, cwd)?;
    let groups = planner::groups(&context);
    let mut warnings = Vec::new();
    if groups.len() > 1 {
        warnings.push("multiple logical commits".to_string());
    }
    let semantic = context.semantic.clone();
    Ok(PrepareOutput {
        branch: context.branch.clone(),
        status_short: context.status_short.clone(),
        staged_stat: context.staged_stat.clone(),
        staged_name_status: context.staged_name_status.clone(),
        inferred_type: context
            .inferred_type
            .map(CommitType::as_str)
            .map(str::to_string),
        recent_subjects: context.recent_subjects.clone(),
        staged_diff: context.staged_diff.clone(),
        warnings,
        groups,
        message_policy: MessagePolicy {
            format: "Conventional Commits",
            language: "English type/scope prefix; Simplified Chinese subject and body",
            allowed_types: config.allowed_types.clone(),
            max_subject_chars: config.max_subject_chars,
            scope_examples: vec!["cli", "core", "mcp", "message", "config"],
        },
        semantic,
    })
}

pub fn execute_commit_message(
    cwd: &Path,
    options: ExecuteOptions,
) -> Result<ExecuteOutput, String> {
    let config = config::Config::load()?;
    let context = collect_context(&config, cwd)?;
    let message = options.message.trim().to_string();
    validate_generated_message(&message, &config)?;
    let confidence = confidence::analyze(&context, &llm::LlmJudgement::ok(), true);

    git::commit(cwd, &message, false)?;
    history::append(
        &config.home,
        cwd,
        options.generated_message.as_deref().unwrap_or(&message),
        &message,
    )?;
    let sha = git::head_sha(cwd).ok();
    let post_status_short = git::status_short(cwd).unwrap_or_default();
    Ok(ExecuteOutput {
        committed: true,
        message,
        confidence,
        sha,
        post_status_short,
    })
}

pub fn generate_tty_candidate(
    cwd: &Path,
    options: TtyGenerateOptions,
) -> Result<TtyGenerateOutput, String> {
    let config = config::Config::load()?;
    let context = collect_context(&config, cwd)?;
    let generated = llm::generate_checked_message(
        &context,
        &config,
        options.feedback.as_deref(),
        &options.prior_attempts,
        false,
    )?;
    validate_generated_message(&generated.message, &config)?;
    let confidence = confidence::analyze(&context, &generated.judgement, true);
    Ok(TtyGenerateOutput {
        message: generated.message,
        confidence,
    })
}

fn validate_generated_message(message: &str, config: &config::Config) -> Result<(), String> {
    message::validate(message, config).map_err(|error| {
        format!(
            "generated commit message failed validation: {error}\n\nGenerated message:\n{message}"
        )
    })
}

fn collect_context(config: &config::Config, cwd: &Path) -> Result<GitContext, String> {
    let history = history::read_recent(&config.home, 5)?;
    let mut context = git::collect_context(cwd, history)?;
    if config.ast_enabled {
        let semantic = build_semantic_analysis(cwd, &context, config);
        if context.inferred_type.is_none() {
            context.inferred_type = infer_type_from_semantic(&semantic);
        }
        context.semantic = Some(semantic);
    }
    Ok(context)
}

fn infer_type_from_semantic(semantic: &SemanticAnalysis) -> Option<CommitType> {
    use crate::semantic::{ChangeKind, Visibility};

    let changes: Vec<&crate::semantic::SymbolChange> = semantic
        .files
        .iter()
        .flat_map(|file| &file.symbols)
        .collect();
    if changes.is_empty() {
        return None;
    }
    let all_added = changes
        .iter()
        .all(|change| matches!(change.change, ChangeKind::Added));
    let any_public = changes
        .iter()
        .any(|change| matches!(change.visibility, Visibility::Public | Visibility::PubCrate));
    if all_added && any_public {
        return Some(CommitType::Feat);
    }
    None
}

fn build_semantic_analysis(
    cwd: &Path,
    context: &GitContext,
    config: &config::Config,
) -> SemanticAnalysis {
    let mut files = Vec::new();
    let mut unsupported = Vec::new();
    for file in git::changed_files(context) {
        let path = file.path;
        if semantic::detect_language(&path).is_none() {
            unsupported.push(path);
            continue;
        }
        let new_source = git::file_content_at(cwd, "", &path).ok().flatten();
        let old_source = git::file_content_at(cwd, "HEAD", &path).ok().flatten();
        let oversized = new_source.as_deref().map(str::len).unwrap_or(0)
            > config.ast_max_file_bytes
            || old_source.as_deref().map(str::len).unwrap_or(0) > config.ast_max_file_bytes;
        if oversized {
            unsupported.push(path);
            continue;
        }
        // None = 解析失败或无符号级变化，静默忽略
        if let Some(file_sem) =
            semantic::analyze_file(&path, old_source.as_deref(), new_source.as_deref())
        {
            files.push(file_sem);
        }
    }
    SemanticAnalysis { files, unsupported }
}
