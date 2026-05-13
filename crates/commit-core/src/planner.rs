use crate::{GitContext, git};
use std::collections::BTreeMap;

pub fn reject_multi_purpose(context: &GitContext) -> Result<(), String> {
    let groups = grouped_files(context);
    if groups.len() <= 1 {
        Ok(())
    } else {
        Err(format!(
            "staged changes look like multiple logical commits; run commit plan to inspect\n{}",
            render_groups(&groups)
        ))
    }
}

pub fn is_multi_purpose(context: &GitContext) -> bool {
    grouped_files(context).len() > 1
}

pub fn plan(context: &GitContext) -> String {
    let groups = grouped_files(context);
    if groups.len() <= 1 {
        "Staged changes look like a single logical commit.".to_string()
    } else {
        format!(
            "Staged changes look like multiple logical commits:\n\n{}\nUse git add -p to split them.",
            render_groups(&groups)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FileGroup {
    pub label: &'static str,
    pub files: Vec<String>,
}

pub fn groups(context: &GitContext) -> Vec<FileGroup> {
    grouped_files(context)
        .into_iter()
        .map(|(label, files)| FileGroup { label, files })
        .collect()
}

fn grouped_files(context: &GitContext) -> BTreeMap<&'static str, Vec<String>> {
    let mut groups = BTreeMap::new();
    for file in git::changed_files(context) {
        groups
            .entry(category(&file.path))
            .or_insert_with(Vec::new)
            .push(file.path);
    }
    groups
}

fn category(path: &str) -> &'static str {
    if path.ends_with(".md") || path.starts_with("docs/") {
        "docs"
    } else if path.starts_with(".github/") || path.contains("workflow") {
        "ci"
    } else if path.starts_with("tests/") || path.contains("/tests/") || path.ends_with("_test.rs") {
        "test"
    } else if matches!(
        path,
        "Cargo.toml" | "Cargo.lock" | "package.json" | "package-lock.json" | "pnpm-lock.yaml"
    ) {
        "build"
    } else {
        "src"
    }
}

fn render_groups(groups: &BTreeMap<&'static str, Vec<String>>) -> String {
    groups
        .iter()
        .enumerate()
        .map(|(index, (category, files))| {
            format!(
                "{}. {}\n   files: {}",
                index + 1,
                category,
                files.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
