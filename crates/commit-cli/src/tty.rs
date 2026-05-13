use commit_core::{
    ExecuteOptions, PriorAttempt, TtyGenerateOptions, execute_commit_message,
    generate_tty_candidate,
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use tempfile::NamedTempFile;
use unicode_width::UnicodeWidthChar;

const HOTKEY_LINE: &str = "[Enter] 提交  [e] 编辑  [r] 换一条  [Ctrl-C] 取消";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Commit,
    Edit,
    Regenerate,
    Cancel,
}

enum CandidateSource {
    Fresh { feedback: Option<String> },
    PreservedEdit(String),
}

#[derive(Debug, PartialEq, Eq)]
enum FeedbackResult {
    Submitted(Option<String>),
    Aborted,
    Cancel,
}

pub fn run_interactive_commit(cwd: &Path) -> Result<(), String> {
    let mut source = CandidateSource::Fresh { feedback: None };
    let mut prior_attempts: Vec<PriorAttempt> = Vec::new();
    loop {
        let message = match &source {
            CandidateSource::Fresh { feedback } => {
                generate_tty_candidate(
                    cwd,
                    TtyGenerateOptions {
                        feedback: feedback.clone(),
                        prior_attempts: prior_attempts.clone(),
                    },
                )?
                .message
            }
            CandidateSource::PreservedEdit(message) => message.clone(),
        };

        print!("{}", format_card(&message));
        io::stdout()
            .flush()
            .map_err(|error| format!("failed to flush stdout: {error}"))?;

        match read_hotkey()? {
            Action::Commit => {
                execute_commit_message(
                    cwd,
                    ExecuteOptions {
                        generated_message: Some(message.clone()),
                        message,
                    },
                )?;
                return Ok(());
            }
            Action::Edit => {
                let edited = edit_message(&message)?;
                match execute_commit_message(
                    cwd,
                    ExecuteOptions {
                        generated_message: Some(message.clone()),
                        message: edited.clone(),
                    },
                ) {
                    Ok(_) => return Ok(()),
                    Err(error) => {
                        eprintln!("\n校验未通过：\n{error}\n");
                        source = CandidateSource::PreservedEdit(edited);
                    }
                }
            }
            Action::Regenerate => {
                source = match prompt_feedback()? {
                    FeedbackResult::Submitted(feedback) => {
                        prior_attempts.push(PriorAttempt {
                            message: message.clone(),
                            feedback: feedback.clone().unwrap_or_default(),
                        });
                        CandidateSource::Fresh { feedback }
                    }
                    FeedbackResult::Aborted => CandidateSource::PreservedEdit(message),
                    FeedbackResult::Cancel => return Ok(()),
                };
            }
            Action::Cancel => return Ok(()),
        }
    }
}

pub fn format_card(message: &str) -> String {
    let mut output = String::new();
    output.push('\n');
    for line in message.lines() {
        output.push_str(line);
        output.push('\n');
    }
    output.push('\n');
    output.push_str(HOTKEY_LINE);
    output.push('\n');
    output
}

fn read_hotkey() -> Result<Action, String> {
    terminal::enable_raw_mode().map_err(|error| format!("failed to enable raw mode: {error}"))?;
    let outcome = loop {
        match event::read() {
            Ok(Event::Key(KeyEvent {
                code,
                modifiers,
                kind,
                ..
            })) => {
                if kind != KeyEventKind::Press {
                    continue;
                }
                if let Some(action) = classify_key(code, modifiers) {
                    break Ok(action);
                }
            }
            Ok(_) => continue,
            Err(error) => break Err(format!("failed to read terminal event: {error}")),
        }
    };
    terminal::disable_raw_mode().map_err(|error| format!("failed to disable raw mode: {error}"))?;
    println!();
    outcome
}

fn classify_key(code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
    match code {
        KeyCode::Enter => Some(Action::Commit),
        KeyCode::Char('c') | KeyCode::Char('C') if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::Cancel)
        }
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            match c.to_ascii_lowercase() {
                'e' => Some(Action::Edit),
                'r' => Some(Action::Regenerate),
                _ => None,
            }
        }
        _ => None,
    }
}

fn prompt_feedback() -> Result<FeedbackResult, String> {
    print!("反馈（回车=换一条，Esc=返回，Ctrl-C=取消）: ");
    io::stdout()
        .flush()
        .map_err(|error| format!("failed to flush stdout: {error}"))?;

    terminal::enable_raw_mode().map_err(|error| format!("failed to enable raw mode: {error}"))?;
    let outcome = read_feedback_line();
    terminal::disable_raw_mode().map_err(|error| format!("failed to disable raw mode: {error}"))?;
    println!();
    outcome
}

fn read_feedback_line() -> Result<FeedbackResult, String> {
    let mut buffer = String::new();
    loop {
        match event::read() {
            Ok(Event::Key(KeyEvent {
                code,
                modifiers,
                kind,
                ..
            })) => {
                if kind != KeyEventKind::Press {
                    continue;
                }
                if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
                    return Ok(FeedbackResult::Cancel);
                }
                match code {
                    KeyCode::Enter => {
                        let trimmed = buffer.trim();
                        return Ok(FeedbackResult::Submitted(if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_string())
                        }));
                    }
                    KeyCode::Esc => return Ok(FeedbackResult::Aborted),
                    KeyCode::Backspace => {
                        if let Some(c) = buffer.pop() {
                            erase_char_visual(c)?;
                        }
                    }
                    KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                        buffer.push(c);
                        print!("{c}");
                        io::stdout()
                            .flush()
                            .map_err(|error| format!("failed to flush stdout: {error}"))?;
                    }
                    _ => continue,
                }
            }
            Ok(_) => continue,
            Err(error) => return Err(format!("failed to read terminal event: {error}")),
        }
    }
}

fn erase_char_visual(c: char) -> Result<(), String> {
    let width = UnicodeWidthChar::width(c).unwrap_or(1);
    let mut out = io::stdout().lock();
    for _ in 0..width {
        out.write_all(b"\x08 \x08")
            .map_err(|error| format!("failed to erase char: {error}"))?;
    }
    out.flush()
        .map_err(|error| format!("failed to flush stdout: {error}"))?;
    Ok(())
}

fn edit_message(initial: &str) -> Result<String, String> {
    let editor = env::var("GIT_EDITOR")
        .or_else(|_| env::var("VISUAL"))
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    let mut file =
        NamedTempFile::new().map_err(|error| format!("failed to create temp file: {error}"))?;
    file.write_all(initial.as_bytes())
        .map_err(|error| format!("failed to write temp file: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("failed to write temp file: {error}"))?;
    file.flush()
        .map_err(|error| format!("failed to flush temp file: {error}"))?;

    let mut parts = editor.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| "editor command is empty".to_string())?;
    let extra_args: Vec<&str> = parts.collect();

    let status = Command::new(program)
        .args(&extra_args)
        .arg(file.path())
        .status()
        .map_err(|error| format!("failed to run editor `{editor}`: {error}"))?;
    if !status.success() {
        return Err(format!("editor `{editor}` exited with status {status}"));
    }

    fs::read_to_string(file.path())
        .map_err(|error| format!("failed to read edited message: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{Action, classify_key, format_card};
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn format_card_renders_single_line_subject() {
        let card = format_card("feat(cli): 添加 commit message 校验");

        assert_eq!(
            card,
            "\nfeat(cli): 添加 commit message 校验\n\n\
             [Enter] 提交  [e] 编辑  [r] 换一条  [Ctrl-C] 取消\n",
        );
    }

    #[test]
    fn format_card_preserves_body_lines() {
        let message = "docs: 完善文档合规和使用说明\n\n\
                       - 增加 slim 和 bundled 版本差异说明\n\
                       - 更新 FAQ 中依赖查找和 Firefox 登录注意事项";
        let card = format_card(message);

        assert!(card.contains("docs: 完善文档合规和使用说明\n"));
        assert!(card.contains("- 增加 slim 和 bundled 版本差异说明\n"));
        assert!(card.contains("- 更新 FAQ 中依赖查找和 Firefox 登录注意事项\n"));
        assert!(card.ends_with("[Enter] 提交  [e] 编辑  [r] 换一条  [Ctrl-C] 取消\n"));
    }

    #[test]
    fn format_card_preserves_breaking_change_footer() {
        let message = "feat(config)!: 调整配置文件格式\n\n\
                       - 将 message 配置移动到 [message] 表\n\n\
                       BREAKING CHANGE: 旧版 .commit.toml 配置需要迁移到新的分组格式。";
        let card = format_card(message);

        assert!(card.contains("feat(config)!: 调整配置文件格式\n"));
        assert!(card.contains("BREAKING CHANGE: 旧版 .commit.toml 配置需要迁移到新的分组格式。\n"));
    }

    #[test]
    fn classify_key_maps_enter_to_commit() {
        assert_eq!(
            classify_key(KeyCode::Enter, KeyModifiers::NONE),
            Some(Action::Commit),
        );
    }

    #[test]
    fn classify_key_maps_e_to_edit_case_insensitively() {
        assert_eq!(
            classify_key(KeyCode::Char('e'), KeyModifiers::NONE),
            Some(Action::Edit),
        );
        assert_eq!(
            classify_key(KeyCode::Char('E'), KeyModifiers::SHIFT),
            Some(Action::Edit),
        );
    }

    #[test]
    fn classify_key_maps_r_to_regenerate() {
        assert_eq!(
            classify_key(KeyCode::Char('r'), KeyModifiers::NONE),
            Some(Action::Regenerate),
        );
    }

    #[test]
    fn classify_key_maps_ctrl_c_to_cancel() {
        assert_eq!(
            classify_key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Some(Action::Cancel),
        );
    }

    #[test]
    fn classify_key_ignores_unrelated_keys() {
        assert_eq!(classify_key(KeyCode::Char('q'), KeyModifiers::NONE), None);
        assert_eq!(classify_key(KeyCode::Tab, KeyModifiers::NONE), None);
        assert_eq!(
            classify_key(KeyCode::Char('e'), KeyModifiers::CONTROL),
            None,
        );
    }
}
