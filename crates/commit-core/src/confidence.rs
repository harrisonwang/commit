use crate::{GitContext, git, llm};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConfidenceReport {
    pub score: u8,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn analyze(
    context: &GitContext,
    judgement: &llm::LlmJudgement,
    validation_passed: bool,
) -> ConfidenceReport {
    let files = git::changed_files(context);
    let mut score = 3_i8;
    let mut reasons = Vec::new();
    let mut warnings = Vec::new();

    if files.len() == 1 {
        score += 1;
        reasons.push("single file change".to_string());
    } else if files.len() <= 3 {
        reasons.push("small file set".to_string());
    } else {
        score -= 1;
        warnings.push("many changed files".to_string());
    }

    if let Some(inferred_type) = context.inferred_type {
        score += 1;
        reasons.push(format!("inferred type: {}", inferred_type.as_str()));
    } else {
        score -= 1;
        warnings.push("unknown type".to_string());
    }

    if context.staged_diff.lines().count() <= 80 {
        reasons.push("small diff".to_string());
    } else {
        score -= 1;
        warnings.push("large diff".to_string());
    }

    if validation_passed {
        reasons.push("message validator passed".to_string());
    } else {
        score = 1;
        warnings.push("message validator failed".to_string());
    }

    match judgement.coverage {
        llm::Coverage::Complete => {
            score += 1;
            reasons.push("semantic coverage: complete".to_string());
        }
        llm::Coverage::Partial => {
            score -= 1;
            warnings.push("semantic coverage: partial".to_string());
        }
        llm::Coverage::Unclear => {
            score -= 2;
            warnings.push("semantic coverage: unclear".to_string());
        }
    }

    match judgement.risk {
        llm::Risk::Low => reasons.push("semantic risk: low".to_string()),
        llm::Risk::Medium => {
            score -= 1;
            warnings.push("semantic risk: medium".to_string());
        }
        llm::Risk::High => {
            score -= 2;
            warnings.push("semantic risk: high".to_string());
        }
    }

    if judgement.needs_feedback {
        score -= 2;
        warnings.push(format!("needs feedback: {}", judgement.reason));
    }

    ConfidenceReport {
        score: score.clamp(1, 5) as u8,
        reasons,
        warnings,
    }
}

pub fn score(context: &GitContext) -> u8 {
    analyze(context, &llm::LlmJudgement::ok(), true).score
}
