use crate::history;
use crate::semantic::SemanticAnalysis;

#[derive(Debug, PartialEq, Eq)]
pub struct GitContext {
    pub branch: String,
    pub status_short: String,
    pub staged_diff: String,
    pub staged_stat: String,
    pub staged_name_status: String,
    pub recent_subjects: Vec<String>,
    pub inferred_type: Option<CommitType>,
    pub history: Vec<history::HistoryEntry>,
    pub semantic: Option<SemanticAnalysis>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PriorAttempt {
    pub message: String,
    pub feedback: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitType {
    Docs,
    Ci,
    Test,
    Build,
    Chore,
    Feat,
}

impl CommitType {
    pub fn as_str(self) -> &'static str {
        match self {
            CommitType::Docs => "docs",
            CommitType::Ci => "ci",
            CommitType::Test => "test",
            CommitType::Build => "build",
            CommitType::Chore => "chore",
            CommitType::Feat => "feat",
        }
    }
}
