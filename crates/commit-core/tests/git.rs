mod common;

use commit_core::{CommitType, git};
use std::fs;

#[test]
fn rejects_non_git_directory() {
    let temp = tempfile::tempdir().expect("create temp dir");

    assert_eq!(
        git::collect_context(temp.path(), Vec::new()).unwrap_err(),
        "not inside a Git repository"
    );
}

#[test]
fn rejects_empty_staged_diff() {
    let repo = common::init_repo();

    assert_eq!(
        git::collect_context(repo.path(), Vec::new()).unwrap_err(),
        "no staged changes; stage changes with git add or git add -p"
    );
}

#[test]
fn reads_staged_diff_without_unstaged_changes() {
    let repo = common::init_repo();
    let file = repo.path().join("example.txt");
    fs::write(&file, "staged\n").expect("write staged content");
    common::run_command(repo.path(), "git", &["add", "example.txt"]);
    fs::write(&file, "staged\nunstaged\n").expect("write unstaged content");

    let context = git::collect_context(repo.path(), Vec::new()).expect("collect git context");

    assert!(context.staged_diff.contains("+staged"));
    assert!(!context.staged_diff.contains("+unstaged"));
    assert!(context.status_short.contains("AM example.txt"));
    assert!(context.staged_name_status.contains("A\texample.txt"));
    assert!(context.staged_stat.contains("example.txt"));
}

#[test]
fn infers_type_from_changed_files() {
    use git::ChangedFile;

    assert_eq!(
        git::infer_type(&[ChangedFile {
            status: "M".to_string(),
            path: "docs/design.md".to_string(),
        }]),
        Some(CommitType::Docs)
    );
    assert_eq!(
        git::infer_type(&[ChangedFile {
            status: "M".to_string(),
            path: ".github/workflows/ci.yml".to_string(),
        }]),
        Some(CommitType::Ci)
    );
    assert_eq!(
        git::infer_type(&[ChangedFile {
            status: "A".to_string(),
            path: "tests/cli.rs".to_string(),
        }]),
        Some(CommitType::Test)
    );
    assert_eq!(
        git::infer_type(&[ChangedFile {
            status: "M".to_string(),
            path: "Cargo.toml".to_string(),
        }]),
        Some(CommitType::Build)
    );
    assert_eq!(
        git::infer_type(&[ChangedFile {
            status: "D".to_string(),
            path: "old.txt".to_string(),
        }]),
        Some(CommitType::Chore)
    );
}

#[test]
fn collects_inferred_type_from_staged_files() {
    let repo = common::init_repo();
    common::stage_file(&repo, "docs/usage.md", "hello\n");

    let context = git::collect_context(repo.path(), Vec::new()).expect("collect git context");

    assert_eq!(context.inferred_type, Some(CommitType::Docs));
}

#[test]
fn file_content_at_reads_head_and_staged_versions() {
    let repo = common::init_repo();
    let file_path = repo.path().join("example.txt");
    fs::write(&file_path, "version_one\n").expect("write v1");
    common::run_command(repo.path(), "git", &["add", "example.txt"]);
    common::run_command(repo.path(), "git", &["commit", "-m", "init"]);

    fs::write(&file_path, "version_two\n").expect("write v2");
    common::run_command(repo.path(), "git", &["add", "example.txt"]);

    let head = git::file_content_at(repo.path(), "HEAD", "example.txt").expect("read HEAD");
    let staged = git::file_content_at(repo.path(), "", "example.txt").expect("read staged");

    assert_eq!(head.as_deref(), Some("version_one\n"));
    assert_eq!(staged.as_deref(), Some("version_two\n"));
}

#[test]
fn file_content_at_returns_none_for_new_file_at_head() {
    let repo = common::init_repo();
    common::stage_file(&repo, "fresh.txt", "fresh\n");

    let head = git::file_content_at(repo.path(), "HEAD", "fresh.txt").expect("read HEAD");
    let staged = git::file_content_at(repo.path(), "", "fresh.txt").expect("read staged");

    assert!(head.is_none());
    assert_eq!(staged.as_deref(), Some("fresh\n"));
}

#[test]
fn file_content_at_returns_none_for_deleted_file_in_staged() {
    let repo = common::init_repo();
    let file_path = repo.path().join("doomed.txt");
    fs::write(&file_path, "doomed\n").expect("write doomed");
    common::run_command(repo.path(), "git", &["add", "doomed.txt"]);
    common::run_command(repo.path(), "git", &["commit", "-m", "init"]);
    common::run_command(repo.path(), "git", &["rm", "doomed.txt"]);

    let head = git::file_content_at(repo.path(), "HEAD", "doomed.txt").expect("read HEAD");
    let staged = git::file_content_at(repo.path(), "", "doomed.txt").expect("read staged");

    assert_eq!(head.as_deref(), Some("doomed\n"));
    assert!(staged.is_none());
}
