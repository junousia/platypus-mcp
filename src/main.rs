use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("runner") {
        return platypus_mcp::runner::run_cli(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("bootstrap") {
        let code = platypus_mcp::bootstrap::run_cli(&args[1..])?;
        std::process::exit(code);
    }
    if args.first().map(String::as_str) == Some("tool") {
        let code = platypus_mcp::cli::run_tool_cli(&args[1..]).await?;
        std::process::exit(code);
    }
    platypus_mcp::serve_stdio().await
}
