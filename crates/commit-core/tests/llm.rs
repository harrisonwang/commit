use commit_core::{CommitType, GitContext, history, llm};

#[test]
fn builds_prompt_with_staged_context() {
    let context = GitContext {
        branch: "main\n".to_string(),
        status_short: "A  example.txt\n".to_string(),
        staged_diff: "diff --git a/example.txt b/example.txt\n+hello\n".to_string(),
        staged_stat: " example.txt | 1 +\n".to_string(),
        staged_name_status: "A\texample.txt\n".to_string(),
        recent_subjects: vec!["feat: previous".to_string()],
        inferred_type: Some(CommitType::Docs),
        history: vec![history::HistoryEntry {
            repo: "/tmp/repo".to_string(),
            generated: "feat: old draft".to_string(),
            final_message: "feat: old final".to_string(),
        }],
        semantic: None,
    };

    let prompt = llm::build_prompt(&context);

    assert!(prompt.contains("Branch:\nmain"));
    assert!(prompt.contains("Status:\nA  example.txt"));
    assert!(prompt.contains("Staged name-status:\nA\texample.txt"));
    assert!(prompt.contains("Inferred type:\ndocs"));
    assert!(prompt.contains("Recent commits:\nfeat: previous"));
    assert!(prompt.contains("AI draft: feat: old draft"));
    assert!(prompt.contains("User final: feat: old final"));
    assert!(prompt.contains("+hello"));
}

#[test]
fn parses_structured_llm_judgement() {
    let judgement = llm::parse_judgement(
        "{\"coverage\":\"partial\",\"risk\":\"medium\",\"needs_feedback\":true,\"reason\":\"missing why\"}",
    );

    assert_eq!(judgement.coverage, llm::Coverage::Partial);
    assert_eq!(judgement.risk, llm::Risk::Medium);
    assert!(judgement.needs_feedback);
    assert_eq!(judgement.reason, "missing why");
}

#[test]
fn strips_thinking_blocks_from_llm_output() {
    let raw =
        "<think>reasoning that should not enter git history</think>\n\nfeat(cli): 新增生成命令";

    assert_eq!(
        llm::strip_thinking_blocks(raw).trim(),
        "feat(cli): 新增生成命令"
    );
}

#[test]
fn strips_multiple_thinking_blocks() {
    let raw = "<think>first</think>\nfix: 修复问题\n<think>second</think>";

    assert_eq!(llm::strip_thinking_blocks(raw).trim(), "fix: 修复问题");
}

#[test]
fn drops_unclosed_thinking_block() {
    let raw = "feat: 有效消息\n<think>unfinished reasoning";

    assert_eq!(llm::strip_thinking_blocks(raw).trim(), "feat: 有效消息");
}
