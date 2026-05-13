mod cli;
mod tty;

use clap::Parser;
use cli::{Cli, Command};
use std::io::IsTerminal;
use std::path::Path;

fn main() {
    let cli = Cli::parse();

    if let Err(error) = run(cli) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let cwd = Path::new(".");
    match cli.command {
        None => {
            if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
                tty::run_interactive_commit(cwd)?;
            } else {
                return Err(
                    "non-interactive use is not supported; use `commit mcp` for Agent integration"
                        .to_string(),
                );
            }
        }
        Some(Command::Mcp) => commit_mcp::serve_stdio(cwd)?,
    }
    Ok(())
}
