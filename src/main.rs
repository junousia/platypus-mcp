use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("runner") {
        return platypus_mcp_rs::runner::run_cli(&args[1..]);
    }
    platypus_mcp_rs::serve_stdio().await
}
