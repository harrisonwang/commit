mod common;

use commit_core::prepare_commit_context;
use std::process::Command;

#[test]
fn prepare_reports_groups_for_mixed_staged_changes() {
    let _guard = common::env_lock().lock().expect("lock env");
    let old_home = std::env::var_os("COMMIT_HOME");
    let commit_home = tempfile::tempdir().expect("create commit home");
    common::set_commit_home(commit_home.path());
    let repo = common::init_repo();
    common::stage_file(&repo, "src/main.rs", "fn main() {}\n");
    common::stage_file(&repo, "docs/usage.md", "usage\n");
    common::stage_file(&repo, ".github/workflows/ci.yml", "name: ci\n");

    let output = prepare_commit_context(repo.path()).expect("prepare should succeed");
    common::restore_commit_home(old_home);

    assert!(
        output
            .warnings
            .contains(&"multiple logical commits".to_string())
    );
    let labels: Vec<&str> = output.groups.iter().map(|group| group.label).collect();
    assert!(labels.contains(&"docs"));
    assert!(labels.contains(&"src"));
    assert!(labels.contains(&"ci"));
    let log = Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(repo.path())
        .output()
        .expect("run git log");
    assert!(!log.status.success());
}
