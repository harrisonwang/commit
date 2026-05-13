use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub repo: String,
    pub generated: String,
    pub final_message: String,
}

pub fn read_recent(home: &Path, limit: usize) -> Result<Vec<HistoryEntry>, String> {
    let path = home.join("history.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read history: {error}"))?;
    let entries = content
        .lines()
        .rev()
        .take(limit)
        .map(|line| {
            serde_json::from_str::<HistoryEntry>(line)
                .map_err(|error| format!("failed to parse history: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(entries.into_iter().rev().collect())
}

pub fn append(home: &Path, cwd: &Path, generated: &str, final_message: &str) -> Result<(), String> {
    std::fs::create_dir_all(home)
        .map_err(|error| format!("failed to create commit home: {error}"))?;
    let entry = HistoryEntry {
        repo: cwd.display().to_string(),
        generated: generated.to_string(),
        final_message: final_message.to_string(),
    };
    let mut line = serde_json::to_string(&entry)
        .map_err(|error| format!("failed to serialize history: {error}"))?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("history.jsonl"))
        .map_err(|error| format!("failed to open history: {error}"))?;
    file.write_all(line.as_bytes())
        .map_err(|error| format!("failed to write history: {error}"))
}
