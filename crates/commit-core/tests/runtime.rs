mod common;

use commit_core::{
    ExecuteOptions, PriorAttempt, TtyGenerateOptions, execute_commit_message,
    generate_tty_candidate, history, prepare_commit_context,
};
use std::fs;
use std::path::Path;

#[test]
fn prepare_commit_context_outputs_context_and_policy_without_calling_llm() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");
    let capture_dir = tempfile::tempdir().expect("create capture dir");
    let capture = capture_dir.path().join("stdin.txt");
    let (_bin_dir, old_path) = common::install_sequence_llm(&["feat: should not run"], &capture);

    let output = prepare_commit_context(repo.path()).expect("prepare should succeed");

    unsafe {
        std::env::set_var("PATH", old_path);
    }
    common::restore_commit_home(old_home);

    assert!(output.staged_diff.contains("+hello"));
    assert_eq!(output.message_policy.format, "Conventional Commits");
    assert!(
        output
            .message_policy
            .allowed_types
            .contains(&"feat".to_string())
    );
    assert!(!capture.exists());
    assert!(!common::git_log_exists(&repo));
}

#[test]
fn generate_tty_candidate_returns_message_without_committing() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");
    let capture_dir = tempfile::tempdir().expect("create capture dir");
    let capture = capture_dir.path().join("stdin.txt");
    let (_bin_dir, old_path) = common::install_sequence_llm(&["feat: add example file"], &capture);

    let result = generate_tty_candidate(repo.path(), TtyGenerateOptions::default());

    unsafe {
        std::env::set_var("PATH", old_path);
    }
    common::restore_commit_home(old_home);

    assert_eq!(
        result.expect("generate should succeed").message,
        "feat: add example file"
    );
    let captured = fs::read_to_string(capture).expect("read captured stdin");
    assert!(captured.contains("+hello"));
    assert!(!common::git_log_exists(&repo));
}

#[test]
fn execute_commit_message_commits_supplied_message_without_calling_llm() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");
    let capture_dir = tempfile::tempdir().expect("create capture dir");
    let capture = capture_dir.path().join("stdin.txt");
    let (_bin_dir, old_path) = common::install_sequence_llm(&["feat: should not run"], &capture);

    let output = execute_commit_message(
        repo.path(),
        ExecuteOptions {
            message: "feat: add example file".to_string(),
            generated_message: None,
        },
    )
    .expect("execute should succeed");

    unsafe {
        std::env::set_var("PATH", old_path);
    }
    common::restore_commit_home(old_home);

    assert!(output.committed);
    assert_eq!(output.message, "feat: add example file");
    assert_eq!(output.post_status_short.trim(), "");
    assert_eq!(common::commit_subject(&repo), "feat: add example file");
    assert!(!capture.exists());
    let entries = history::read_recent(commit_home.path(), 5).expect("read history");
    assert_eq!(entries[0].generated, "feat: add example file");
    assert_eq!(entries[0].final_message, "feat: add example file");
}

#[test]
fn execute_commit_message_records_generated_and_final_pair() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");

    execute_commit_message(
        repo.path(),
        ExecuteOptions {
            message: "feat(cli): 添加示例文件".to_string(),
            generated_message: Some("feat: add example file".to_string()),
        },
    )
    .expect("execute should succeed");
    common::restore_commit_home(old_home);

    let entries = history::read_recent(commit_home.path(), 5).expect("read history");
    assert_eq!(entries[0].generated, "feat: add example file");
    assert_eq!(entries[0].final_message, "feat(cli): 添加示例文件");
}

#[test]
fn execute_invalid_message_blocks_commit() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");

    let result = execute_commit_message(
        repo.path(),
        ExecuteOptions {
            message: "add example file".to_string(),
            generated_message: None,
        },
    );
    common::restore_commit_home(old_home);

    let error = result.unwrap_err();
    assert!(
        error.contains("generated commit message failed validation: commit subject must look like '<type>: <summary>'")
    );
    assert!(error.contains("Generated message:\nadd example file"));
    assert!(!common::git_log_exists(&repo));
}

#[test]
fn feedback_reaches_tty_llm_prompt() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");
    let capture_dir = tempfile::tempdir().expect("create capture dir");
    let capture = capture_dir.path().join("stdin.txt");
    let (_bin_dir, old_path) = common::install_sequence_llm(&["feat: add example file"], &capture);

    let result = generate_tty_candidate(
        repo.path(),
        TtyGenerateOptions {
            feedback: Some("make it shorter".to_string()),
            prior_attempts: Vec::new(),
        },
    );

    unsafe {
        std::env::set_var("PATH", old_path);
    }
    common::restore_commit_home(old_home);

    assert_eq!(
        result.expect("generate should succeed").message,
        "feat: add example file"
    );
    let captured = fs::read_to_string(capture).expect("read captured prompts");
    assert!(captured.contains("User feedback:\nmake it shorter"));
}

#[test]
fn config_changes_llm_profile_and_validation() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    fs::write(
        commit_home.path().join("config.toml"),
        "[llm]\nprofile = \"deepseek\"\n\n[message]\nmax_subject_chars = 10\n\n[validation]\nbanned_phrases = [\"forbidden\"]\n",
    )
    .expect("write config");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");
    let capture_dir = tempfile::tempdir().expect("create capture dir");
    let capture = capture_dir.path().join("stdin.txt");
    let (_bin_dir, old_path) = common::install_sequence_llm(&["feat: forbidden"], &capture);

    let result = generate_tty_candidate(repo.path(), TtyGenerateOptions::default());

    unsafe {
        std::env::set_var("PATH", old_path);
    }
    common::restore_commit_home(old_home);

    let error = result.unwrap_err();
    assert!(error.contains(
        "generated commit message failed validation: commit subject is longer than 10 characters"
    ));
    assert!(error.contains("Generated message:\nfeat: forbidden"));
    let captured = fs::read_to_string(capture).expect("read captured prompt");
    assert!(captured.contains("+hello"));
}

#[test]
fn previous_history_appears_in_later_prompt() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    history::append(
        commit_home.path(),
        Path::new("/tmp/repo"),
        "feat: previous draft",
        "feat: previous final",
    )
    .expect("write history");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");
    let capture_dir = tempfile::tempdir().expect("create capture dir");
    let capture = capture_dir.path().join("stdin.txt");
    let (_bin_dir, old_path) = common::install_sequence_llm(&["feat: add example file"], &capture);

    let result = generate_tty_candidate(repo.path(), TtyGenerateOptions::default());

    unsafe {
        std::env::set_var("PATH", old_path);
    }
    common::restore_commit_home(old_home);

    assert_eq!(
        result.expect("generate should succeed").message,
        "feat: add example file"
    );
    let captured = fs::read_to_string(capture).expect("read captured prompt");
    assert!(captured.contains("AI draft: feat: previous draft"));
    assert!(captured.contains("User final: feat: previous final"));
}

#[test]
fn generated_message_with_body_and_footer_can_commit() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");

    let result = execute_commit_message(
        repo.path(),
        ExecuteOptions {
            message: "feat: add example file\n\nExplain why.\n\nCloses #12".to_string(),
            generated_message: None,
        },
    );
    common::restore_commit_home(old_home);

    assert!(result.is_ok());
    assert_eq!(common::commit_subject(&repo), "feat: add example file");
}

#[test]
fn generated_breaking_message_can_commit() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");

    let result = execute_commit_message(
        repo.path(),
        ExecuteOptions {
            message: "feat!: change public API".to_string(),
            generated_message: None,
        },
    );
    common::restore_commit_home(old_home);

    assert!(result.is_ok());
    assert_eq!(common::commit_subject(&repo), "feat!: change public API");
}

#[test]
fn tty_generation_allows_mixed_staged_changes() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "src/main.rs", "fn main() {}\n");
    common::stage_file(&repo, "docs/usage.md", "usage\n");
    common::stage_file(&repo, "tests/example.rs", "#[test] fn t() {}\n");
    let capture_dir = tempfile::tempdir().expect("create capture dir");
    let capture = capture_dir.path().join("stdin.txt");
    let (_bin_dir, old_path) =
        common::install_sequence_llm(&["feat: ship feature with docs and tests"], &capture);

    let result = generate_tty_candidate(repo.path(), TtyGenerateOptions::default());

    unsafe {
        std::env::set_var("PATH", old_path);
    }
    common::restore_commit_home(old_home);

    assert_eq!(
        result.expect("generate should succeed").message,
        "feat: ship feature with docs and tests"
    );
}

#[test]
fn tty_generation_makes_single_llm_call() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");
    let capture_dir = tempfile::tempdir().expect("create capture dir");
    let capture = capture_dir.path().join("stdin.txt");
    let (_bin_dir, old_path) = common::install_sequence_llm(&["feat: add example file"], &capture);

    let result = generate_tty_candidate(repo.path(), TtyGenerateOptions::default());

    unsafe {
        std::env::set_var("PATH", old_path);
    }
    common::restore_commit_home(old_home);

    assert_eq!(
        result.expect("generate should succeed").message,
        "feat: add example file"
    );
    let captured = fs::read_to_string(capture).expect("read captured stdin");
    let call_count = captured.matches("---CALL---").count();
    assert_eq!(
        call_count, 1,
        "TTY generation should issue exactly 1 LLM call"
    );
}

#[test]
fn prepare_populates_semantic_when_ast_enabled() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    fs::write(
        commit_home.path().join("config.toml"),
        "[ast]\nenabled = true\n",
    )
    .expect("write config");
    common::set_commit_home(commit_home.path());

    let repo = common::init_repo();
    let file_path = repo.path().join("lib.rs");
    fs::write(&file_path, "pub fn keep() {}\n").expect("write v1");
    common::run_command(repo.path(), "git", &["add", "lib.rs"]);
    common::run_command(repo.path(), "git", &["commit", "-m", "init"]);

    fs::write(
        &file_path,
        "pub fn keep() { let _ = 1; }\npub fn fresh() {}\n",
    )
    .expect("write v2");
    common::run_command(repo.path(), "git", &["add", "lib.rs"]);

    let output = prepare_commit_context(repo.path()).expect("prepare should succeed");
    common::restore_commit_home(old_home);

    let semantic = output
        .semantic
        .as_ref()
        .expect("ast_enabled=true should populate semantic");
    assert_eq!(semantic.files.len(), 1);
    let file = &semantic.files[0];
    assert_eq!(file.path, "lib.rs");
    let names: Vec<&str> = file.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"keep"), "expected keep in {names:?}");
    assert!(names.contains(&"fresh"), "expected fresh in {names:?}");

    let fresh_change = file
        .symbols
        .iter()
        .find(|s| s.name == "fresh")
        .expect("fresh symbol");
    assert!(matches!(
        fresh_change.change,
        commit_core::ChangeKind::Added
    ));
    let keep_change = file
        .symbols
        .iter()
        .find(|s| s.name == "keep")
        .expect("keep symbol");
    assert!(matches!(
        keep_change.change,
        commit_core::ChangeKind::BodyModified
    ));
}

#[test]
fn ast_max_file_bytes_skips_oversized_files() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    fs::write(
        commit_home.path().join("config.toml"),
        "[ast]\nenabled = true\nmax_file_bytes = 32\n",
    )
    .expect("write config");
    common::set_commit_home(commit_home.path());

    let repo = common::init_repo();
    let bulky = "pub fn one() {}\npub fn two() {}\npub fn three() {}\n".to_string();
    common::stage_file(&repo, "lib.rs", &bulky);

    let output = prepare_commit_context(repo.path()).expect("prepare should succeed");
    common::restore_commit_home(old_home);

    let semantic = output
        .semantic
        .as_ref()
        .expect("ast_enabled=true should populate semantic");
    assert!(semantic.files.is_empty());
    assert!(semantic.unsupported.contains(&"lib.rs".to_string()));
}

#[test]
fn ast_lists_non_rust_files_as_unsupported() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    fs::write(
        commit_home.path().join("config.toml"),
        "[ast]\nenabled = true\n",
    )
    .expect("write config");
    common::set_commit_home(commit_home.path());

    let repo = common::init_repo();
    common::stage_file(&repo, "README.md", "hello\n");

    let output = prepare_commit_context(repo.path()).expect("prepare should succeed");
    common::restore_commit_home(old_home);

    let semantic = output
        .semantic
        .as_ref()
        .expect("ast_enabled=true should populate semantic");
    assert!(semantic.files.is_empty());
    assert_eq!(semantic.unsupported, vec!["README.md".to_string()]);
}

#[test]
fn infers_feat_from_only_public_additions() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    fs::write(
        commit_home.path().join("config.toml"),
        "[ast]\nenabled = true\n",
    )
    .expect("write config");
    common::set_commit_home(commit_home.path());

    let repo = common::init_repo();
    let file_path = repo.path().join("lib.rs");
    fs::write(&file_path, "fn unrelated() {}\n").expect("write v1");
    common::run_command(repo.path(), "git", &["add", "lib.rs"]);
    common::run_command(repo.path(), "git", &["commit", "-m", "init"]);
    fs::write(
        &file_path,
        "fn unrelated() {}\npub fn new_thing() {}\npub fn another() {}\n",
    )
    .expect("write v2");
    common::run_command(repo.path(), "git", &["add", "lib.rs"]);

    let output = prepare_commit_context(repo.path()).expect("prepare should succeed");
    common::restore_commit_home(old_home);

    assert_eq!(output.inferred_type.as_deref(), Some("feat"));
}

#[test]
fn does_not_infer_feat_when_only_bodies_modified() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    fs::write(
        commit_home.path().join("config.toml"),
        "[ast]\nenabled = true\n",
    )
    .expect("write config");
    common::set_commit_home(commit_home.path());

    let repo = common::init_repo();
    let file_path = repo.path().join("lib.rs");
    fs::write(&file_path, "pub fn keep() { let _ = 1; }\n").expect("write v1");
    common::run_command(repo.path(), "git", &["add", "lib.rs"]);
    common::run_command(repo.path(), "git", &["commit", "-m", "init"]);
    fs::write(&file_path, "pub fn keep() { let _ = 2; }\n").expect("write v2");
    common::run_command(repo.path(), "git", &["add", "lib.rs"]);

    let output = prepare_commit_context(repo.path()).expect("prepare should succeed");
    common::restore_commit_home(old_home);

    assert_eq!(
        output.inferred_type, None,
        "body-only modifications should not trigger feat inference"
    );
}

#[test]
fn prepare_skips_semantic_when_ast_disabled_by_default() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "lib.rs", "pub fn new_one() {}\n");

    let output = prepare_commit_context(repo.path()).expect("prepare should succeed");
    common::restore_commit_home(old_home);

    assert!(
        output.semantic.is_none(),
        "ast_enabled defaults to false; semantic should be None"
    );
}

#[test]
fn semantic_changes_appear_in_tty_llm_prompt() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    fs::write(
        commit_home.path().join("config.toml"),
        "[ast]\nenabled = true\n",
    )
    .expect("write config");
    common::set_commit_home(commit_home.path());

    let repo = common::init_repo();
    let file_path = repo.path().join("lib.rs");
    fs::write(&file_path, "pub fn keep() {}\n").expect("write v1");
    common::run_command(repo.path(), "git", &["add", "lib.rs"]);
    common::run_command(repo.path(), "git", &["commit", "-m", "init"]);
    fs::write(&file_path, "pub fn keep() {}\npub fn fresh() {}\n").expect("write v2");
    common::run_command(repo.path(), "git", &["add", "lib.rs"]);

    let capture_dir = tempfile::tempdir().expect("create capture dir");
    let capture = capture_dir.path().join("stdin.txt");
    let (_bin_dir, old_path) = common::install_sequence_llm(&["feat: add fresh fn"], &capture);

    let _ = generate_tty_candidate(repo.path(), TtyGenerateOptions::default());

    unsafe {
        std::env::set_var("PATH", old_path);
    }
    common::restore_commit_home(old_home);

    let captured = fs::read_to_string(capture).expect("read captured prompt");
    assert!(
        captured.contains("Semantic changes (top-level symbols)"),
        "prompt missing semantic section:\n{captured}"
    );
    assert!(
        captured.contains("+ pub fn fresh (added)"),
        "prompt missing fresh symbol:\n{captured}"
    );
}

#[test]
fn prior_attempts_appear_in_subsequent_prompt() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "example.txt", "hello\n");
    let capture_dir = tempfile::tempdir().expect("create capture dir");
    let capture = capture_dir.path().join("stdin.txt");
    let (_bin_dir, old_path) = common::install_sequence_llm(&["feat: add example"], &capture);

    let result = generate_tty_candidate(
        repo.path(),
        TtyGenerateOptions {
            feedback: Some("再短一点".to_string()),
            prior_attempts: vec![
                PriorAttempt {
                    message: "feat: add example tracked text file with greeting".to_string(),
                    feedback: "简短一点".to_string(),
                },
                PriorAttempt {
                    message: "feat: add example tracked file".to_string(),
                    feedback: String::new(),
                },
            ],
        },
    );

    unsafe {
        std::env::set_var("PATH", old_path);
    }
    common::restore_commit_home(old_home);

    assert!(result.is_ok());
    let captured = fs::read_to_string(capture).expect("read captured prompt");
    assert!(captured.contains("Session attempts so far"));
    assert!(captured.contains("Attempt 1:\nfeat: add example tracked text file with greeting"));
    assert!(captured.contains("User feedback after attempt 1: 简短一点"));
    assert!(captured.contains("Attempt 2:\nfeat: add example tracked file"));
    assert!(
        captured.contains("User feedback after attempt 2: (no specific feedback, just regenerate)")
    );
    assert!(captured.contains("User feedback:\n再短一点"));
}
