use crate::{CommitType, GitContext, history};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::NamedTempFile;

pub fn collect_context(
    cwd: &Path,
    history: Vec<history::HistoryEntry>,
) -> Result<GitContext, String> {
    ensure_repo(cwd)?;

    let staged_diff = git_output(cwd, &["diff", "--cached"])?;
    if staged_diff.trim().is_empty() {
        return Err("no staged changes; stage changes with git add or git add -p".to_string());
    }

    let staged_name_status =
        git_output(cwd, &["diff", "--cached", "--name-status"]).unwrap_or_default();
    let changed_files = parse_name_status(&staged_name_status);

    Ok(GitContext {
        branch: git_output(cwd, &["branch", "--show-current"]).unwrap_or_default(),
        status_short: git_output(cwd, &["status", "--short"]).unwrap_or_default(),
        staged_stat: git_output(cwd, &["diff", "--cached", "--stat"]).unwrap_or_default(),
        inferred_type: infer_type(&changed_files),
        staged_name_status,
        recent_subjects: recent_subjects(cwd),
        staged_diff,
        history,
        semantic: None,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChangedFile {
    pub status: String,
    pub path: String,
}

pub fn infer_type(files: &[ChangedFile]) -> Option<CommitType> {
    if files.is_empty() {
        return None;
    }

    if files.iter().all(|file| is_docs_path(&file.path)) {
        Some(CommitType::Docs)
    } else if files.iter().all(|file| is_ci_path(&file.path)) {
        Some(CommitType::Ci)
    } else if files.iter().all(|file| is_test_path(&file.path)) {
        Some(CommitType::Test)
    } else if files.iter().all(|file| is_build_path(&file.path)) {
        Some(CommitType::Build)
    } else if files.iter().all(|file| file.status == "D") {
        Some(CommitType::Chore)
    } else {
        None
    }
}

fn parse_name_status(name_status: &str) -> Vec<ChangedFile> {
    name_status
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some(ChangedFile {
                status: fields.next()?.to_string(),
                path: fields.last()?.to_string(),
            })
        })
        .collect()
}

fn is_docs_path(path: &str) -> bool {
    path.ends_with(".md") || path.starts_with("docs/")
}

fn is_ci_path(path: &str) -> bool {
    path.starts_with(".github/")
        || path.ends_with(".yml") && path.contains("workflow")
        || path.ends_with(".yaml") && path.contains("workflow")
}

fn is_test_path(path: &str) -> bool {
    path.starts_with("tests/") || path.contains("/tests/") || path.ends_with("_test.rs")
}

fn is_build_path(path: &str) -> bool {
    matches!(
        path,
        "Cargo.toml" | "Cargo.lock" | "package.json" | "package-lock.json" | "pnpm-lock.yaml"
    )
}

pub fn changed_files(context: &GitContext) -> Vec<ChangedFile> {
    parse_name_status(&context.staged_name_status)
}

pub fn stage_exact(cwd: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    git_output(cwd, &args).map(|_| ())
}

pub fn unstage_exact(cwd: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["reset", "HEAD", "--"];
    args.extend(paths.iter().map(String::as_str));
    git_output(cwd, &args).map(|_| ())
}

pub fn head_sha(cwd: &Path) -> Result<String, String> {
    git_output(cwd, &["rev-parse", "HEAD"]).map(|sha| sha.trim().to_string())
}

pub fn status_short(cwd: &Path) -> Result<String, String> {
    git_output(cwd, &["status", "--short"])
}

/// Return the file content at `revspec:path`, or `Ok(None)` if the file does not
/// exist at that revspec (e.g. newly added file when revspec="HEAD", or deleted
/// file when revspec="" which means the index/staged version).
///
/// `revspec=""` reads the staged copy (index entry stage 0).
pub fn file_content_at(cwd: &Path, revspec: &str, path: &str) -> Result<Option<String>, String> {
    let key = format!("{revspec}:{path}");
    let exists_check = Command::new("git")
        .args(["cat-file", "-e", &key])
        .current_dir(cwd)
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !exists_check.status.success() {
        return Ok(None);
    }
    git_output(cwd, &["show", &key]).map(Some)
}

pub fn commit(cwd: &Path, message: &str, edit: bool) -> Result<(), String> {
    let mut message_file = NamedTempFile::new()
        .map_err(|error| format!("failed to create commit message file: {error}"))?;
    message_file
        .write_all(message.as_bytes())
        .map_err(|error| format!("failed to write commit message: {error}"))?;
    message_file
        .write_all(b"\n")
        .map_err(|error| format!("failed to write commit message: {error}"))?;

    let path = message_file
        .path()
        .to_str()
        .ok_or_else(|| "commit message path is not valid UTF-8".to_string())?;
    let mut args = vec!["commit", "-F", path];
    if edit {
        args.push("-e");
    }
    git_output(cwd, &args).map(|_| ())
}

fn ensure_repo(cwd: &Path) -> Result<(), String> {
    git_output(cwd, &["rev-parse", "--is-inside-work-tree"])
        .and_then(|output| {
            if output.trim() == "true" {
                Ok(())
            } else {
                Err("not inside a Git repository".to_string())
            }
        })
        .map_err(|_| "not inside a Git repository".to_string())
}

fn recent_subjects(cwd: &Path) -> Vec<String> {
    git_output(cwd, &["log", "-8", "--pretty=format:%s"])
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            stderr
        })
    }
}
