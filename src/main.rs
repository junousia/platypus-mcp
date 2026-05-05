use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    platypus_mcp_rs::serve_stdio().await
}
