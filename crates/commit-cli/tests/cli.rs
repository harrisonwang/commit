#[path = "../src/cli.rs"]
mod cli;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};

#[test]
fn defaults_to_tty_commit_flow() {
    let cli = Cli::parse_from(["commit"]);

    assert_eq!(cli.command, None);
}

#[test]
fn parses_mcp() {
    let cli = Cli::parse_from(["commit", "mcp"]);

    assert_eq!(cli.command, Some(Command::Mcp));
}

#[test]
fn rejects_removed_subcommands() {
    for subcommand in [
        "commit", "prepare", "execute", "generate", "plan", "feedback",
    ] {
        let result = Cli::try_parse_from(["commit", subcommand]);

        assert!(
            result.is_err(),
            "{subcommand} subcommand should not be exposed"
        );
    }
}

#[test]
fn help_mentions_only_tty_and_mcp() {
    let mut command = Cli::command();
    let mut help = Vec::new();
    command.write_long_help(&mut help).expect("write help");
    let help = String::from_utf8(help).expect("help is utf8");

    assert!(help.contains("基于 staged diff 生成并校验高质量 Git commit message"));
    assert!(help.contains("mcp"));
    assert!(!help.contains("\nprepare "));
    assert!(!help.contains("\nexecute "));
    assert!(!help.contains("\ngenerate "));
    assert!(!help.contains("\nplan "));
    assert!(!help.contains("\nfeedback "));
}
