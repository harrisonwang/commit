use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub home: PathBuf,
    pub llm_command: String,
    pub llm_profile: Option<String>,
    pub max_subject_chars: usize,
    pub direct_commit_threshold: u8,
    pub editor_threshold: u8,
    pub banned_phrases: Vec<String>,
    pub allowed_types: Vec<String>,
    pub ast_enabled: bool,
    pub ast_max_file_bytes: usize,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    llm: Option<LlmConfig>,
    message: Option<MessageConfig>,
    validation: Option<ValidationConfig>,
    ast: Option<AstConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct AstConfig {
    enabled: Option<bool>,
    max_file_bytes: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct LlmConfig {
    command: Option<String>,
    profile: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct MessageConfig {
    max_subject_chars: Option<usize>,
    confidence_auto_threshold: Option<u8>,
    direct_commit_threshold: Option<u8>,
    editor_threshold: Option<u8>,
}

#[derive(Debug, Deserialize, Default)]
struct ValidationConfig {
    banned_phrases: Option<Vec<String>>,
    allowed_types: Option<Vec<String>>,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let home = commit_home()?;
        let mut config = Self::default_with_home(home);
        let config_path = config.home.join("config.toml");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|error| format!("failed to read config: {error}"))?;
            let file_config = toml::from_str::<FileConfig>(&content)
                .map_err(|error| format!("failed to parse config: {error}"))?;
            config.apply(file_config);
        }
        Ok(config)
    }

    pub fn default_with_home(home: PathBuf) -> Self {
        Self {
            home,
            llm_command: "llm".to_string(),
            llm_profile: None,
            max_subject_chars: 80,
            direct_commit_threshold: 5,
            editor_threshold: 3,
            banned_phrases: vec![
                "增强灵活性".to_string(),
                "优化代码结构".to_string(),
                "提升用户体验".to_string(),
                "更新文档以反映".to_string(),
            ],
            allowed_types: vec![
                "feat".to_string(),
                "fix".to_string(),
                "docs".to_string(),
                "test".to_string(),
                "ci".to_string(),
                "build".to_string(),
                "deps".to_string(),
                "chore".to_string(),
                "refactor".to_string(),
                "perf".to_string(),
                "style".to_string(),
            ],
            ast_enabled: false,
            ast_max_file_bytes: 200_000,
        }
    }

    fn apply(&mut self, file_config: FileConfig) {
        if let Some(llm) = file_config.llm {
            if let Some(command) = llm.command {
                self.llm_command = command;
            }
            self.llm_profile = llm.profile;
        }
        if let Some(message) = file_config.message {
            if let Some(max_subject_chars) = message.max_subject_chars {
                self.max_subject_chars = max_subject_chars;
            }
            if let Some(confidence_auto_threshold) = message.confidence_auto_threshold {
                self.direct_commit_threshold = confidence_auto_threshold;
            }
            if let Some(direct_commit_threshold) = message.direct_commit_threshold {
                self.direct_commit_threshold = direct_commit_threshold;
            }
            if let Some(editor_threshold) = message.editor_threshold {
                self.editor_threshold = editor_threshold;
            }
        }
        if let Some(validation) = file_config.validation {
            if let Some(banned_phrases) = validation.banned_phrases {
                self.banned_phrases = banned_phrases;
            }
            if let Some(allowed_types) = validation.allowed_types {
                self.allowed_types = allowed_types;
            }
        }
        if let Some(ast) = file_config.ast {
            if let Some(enabled) = ast.enabled {
                self.ast_enabled = enabled;
            }
            if let Some(max_file_bytes) = ast.max_file_bytes {
                self.ast_max_file_bytes = max_file_bytes;
            }
        }
    }
}

fn commit_home() -> Result<PathBuf, String> {
    if let Some(home) = std::env::var_os("COMMIT_HOME") {
        return Ok(PathBuf::from(home));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home).join(".commit"))
}
