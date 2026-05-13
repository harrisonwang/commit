use clap::{Parser, Subcommand};

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "commit",
    version,
    about = "基于 staged diff 生成并校验高质量 Git commit message"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// 启动 MCP stdio server
    Mcp,
}
