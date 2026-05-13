use anyhow::Result;
use clap::{Parser, Subcommand};

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Some(Command::Bootstrap { command }) => {
            let code = platypus_mcp::bootstrap::run_command(command)?;
            std::process::exit(code);
        }
        Some(Command::Tool(command)) => {
            let code = platypus_mcp::cli::run_tool_command(command).await?;
            std::process::exit(code);
        }
        None => platypus_mcp::serve_stdio().await,
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "platypus-mcp",
    about = "Local-first MCP server for deterministic Platypus project orchestration",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Configure an MCP host to launch Platypus MCP.
    Bootstrap {
        #[command(subcommand)]
        command: platypus_mcp::bootstrap::BootstrapCommand,
    },
    /// Invoke one MCP tool through the stdio contract.
    Tool(platypus_mcp::cli::ToolCli),
}
