#![allow(dead_code)]

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use tempfile::TempDir;

pub fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn set_commit_home(home: &Path) {
    unsafe {
        std::env::set_var("COMMIT_HOME", home);
    }
}

pub fn restore_commit_home(old_home: Option<std::ffi::OsString>) {
    unsafe {
        if let Some(old_home) = old_home {
            std::env::set_var("COMMIT_HOME", old_home);
        } else {
            std::env::remove_var("COMMIT_HOME");
        }
    }
}

pub fn run_command(cwd: &Path, command: &str, args: &[&str]) {
    let output = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("failed to run {command}: {error}"));

    assert!(
        output.status.success(),
        "{command} {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn init_repo() -> TempDir {
    let temp = tempfile::tempdir().expect("create temp dir");
    run_command(temp.path(), "git", &["init"]);
    run_command(temp.path(), "git", &["config", "user.name", "Test User"]);
    run_command(
        temp.path(),
        "git",
        &["config", "user.email", "test@example.com"],
    );
    temp
}

pub fn stage_file(repo: &TempDir, path: &str, content: &str) {
    let full_path = repo.path().join(path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(full_path, content).expect("write file");
    run_command(repo.path(), "git", &["add", path]);
}

pub fn install_sequence_llm(outputs: &[&str], capture: &Path) -> (TempDir, std::ffi::OsString) {
    let old_path = std::env::var_os("PATH").expect("PATH is set");
    let bin_dir = tempfile::tempdir().expect("create fake bin dir");
    let state_path = bin_dir.path().join("state");
    let script_path = bin_dir.path().join("llm");
    let cases = outputs
        .iter()
        .enumerate()
        .map(|(index, output)| format!("{index}) cat <<'EOF'\n{output}\nEOF\n;;"))
        .collect::<Vec<_>>()
        .join("\n");
    let script = format!(
        "#!/bin/sh\ncount=$(cat {state} 2>/dev/null || printf 0)\nnext=$((count + 1))\nprintf '%s' \"$next\" > {state}\nwhile [ \"$#\" -gt 1 ]; do\n  if [ \"$1\" = \"-s\" ]; then\n    shift\n    shift\n  else\n    shift\n  fi\ndone\ncat >> {capture}\nprintf '\\n---CALL---\\n' >> {capture}\ncase \"$count\" in\n{cases}\n*) exit 1;;\nesac\n",
        state = state_path.display(),
        capture = capture.display()
    );
    fs::write(script_path, script).expect("write sequence llm");
    run_command(bin_dir.path(), "chmod", &["+x", "llm"]);
    let new_path = format!(
        "{}:{}",
        bin_dir.path().display(),
        old_path.to_string_lossy()
    );
    unsafe {
        std::env::set_var("PATH", new_path);
    }
    (bin_dir, old_path)
}

pub fn commit_subject(repo: &TempDir) -> String {
    let output = Command::new("git")
        .args(["log", "-1", "--pretty=%s"])
        .current_dir(repo.path())
        .output()
        .expect("run git log");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub fn git_log_exists(repo: &TempDir) -> bool {
    Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(repo.path())
        .output()
        .expect("run git log")
        .status
        .success()
}
