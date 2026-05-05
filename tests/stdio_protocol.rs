use rmcp::{transport::TokioChildProcess, ServiceExt};
use std::path::PathBuf;
use tokio::process::Command;

#[tokio::test]
async fn stdio_server_lists_tools_after_initialize() -> anyhow::Result<()> {
    let transport = TokioChildProcess::new(Command::new(server_binary()))?;
    let client = ().serve(transport).await?;

    let tools = client.list_all_tools().await?;
    let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();

    assert!(tool_names.contains(&"inspect_status"));
    assert!(tool_names.contains(&"create_backlog_item"));
    assert!(tool_names.contains(&"doctor_snapshot"));

    client.cancel().await?;
    Ok(())
}

fn server_binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("current test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("platypus-mcp-rs");
    path
}
