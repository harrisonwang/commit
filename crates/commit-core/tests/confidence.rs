use commit_core::{CommitType, GitContext, confidence, llm};

#[test]
fn confidence_report_explains_score() {
    let context = GitContext {
        branch: "main".to_string(),
        status_short: "A  docs/usage.md".to_string(),
        staged_diff: "diff --git a/docs/usage.md b/docs/usage.md\n+usage\n".to_string(),
        staged_stat: "docs/usage.md | 1 +".to_string(),
        staged_name_status: "A\tdocs/usage.md\n".to_string(),
        recent_subjects: Vec::new(),
        inferred_type: Some(CommitType::Docs),
        history: Vec::new(),
        semantic: None,
    };

    let report = confidence::analyze(&context, &llm::LlmJudgement::ok(), true);

    assert_eq!(report.score, 5);
    assert!(report.reasons.contains(&"single file change".to_string()));
    assert!(report.reasons.contains(&"inferred type: docs".to_string()));
    assert!(
        report
            .reasons
            .contains(&"semantic coverage: complete".to_string())
    );
    assert!(report.warnings.is_empty());
}
