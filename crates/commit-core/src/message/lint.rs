use crate::config;

use super::ast::CommitMessage;
use super::error::{LintError, LintErrorKind};

pub fn lint(
    commit: &CommitMessage<'_>,
    raw: &str,
    config: &config::Config,
) -> Result<(), LintError> {
    let subject = raw
        .lines()
        .next()
        .unwrap_or_default()
        .trim_end_matches('\r');
    let actual = subject.chars().count();
    if actual > config.max_subject_chars {
        return Err(LintError::new(
            LintErrorKind::SubjectTooLong {
                max: config.max_subject_chars,
                actual,
            },
            format!(
                "commit subject is longer than {} characters",
                config.max_subject_chars
            ),
        ));
    }

    if commit
        .header
        .description
        .ends_with(['.', '。', '!', '！', '?', '？'])
    {
        return Err(LintError::new(
            LintErrorKind::SubjectEndsWithPunctuation,
            "commit subject must not end with punctuation",
        ));
    }

    if let Some(phrase) = config
        .banned_phrases
        .iter()
        .find(|phrase| raw.contains(phrase.as_str()))
    {
        return Err(LintError::new(
            LintErrorKind::BannedPhrase {
                phrase: phrase.clone(),
            },
            format!("commit message contains banned phrase: {phrase}"),
        ));
    }

    if !config
        .allowed_types
        .iter()
        .any(|allowed_type| allowed_type.eq_ignore_ascii_case(commit.header.ty))
    {
        return Err(LintError::new(
            LintErrorKind::DisallowedType {
                ty: commit.header.ty.to_string(),
            },
            format!(
                "commit type '{}' is not allowed; allowed types: {}",
                commit.header.ty,
                config.allowed_types.join(", ")
            ),
        ));
    }

    Ok(())
}
