use rmcp::{
    model::{CallToolRequestParams, JsonObject},
    transport::TokioChildProcess,
    ServiceExt,
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use tempfile::TempDir;
use tokio::process::Command;

#[tokio::test]
async fn stdio_server_lists_tools_after_initialize() -> anyhow::Result<()> {
    let client = start_client(None).await?;

    let tools = client.list_all_tools().await?;
    let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();

    assert!(tool_names.contains(&"inspect_status"));
    assert!(tool_names.contains(&"create_backlog_item"));
    assert!(tool_names.contains(&"doctor_snapshot"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_calls_structured_ping_tool() -> anyhow::Result<()> {
    let client = start_client(None).await?;

    let result = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "ping".into(),
            arguments: Some(json_args(json!({ "message": "hello" }))),
            task: None,
        })
        .await?;
    let response = result.structured_content.expect("structured content");

    assert_eq!(response["action"], "ping");
    assert_eq!(response["status"], "completed");
    assert_eq!(response["data"]["echo"], "hello");

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_calls_project_doctor_with_configured_root() -> anyhow::Result<()> {
    let project = project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let result = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "doctor_snapshot".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let response = result.structured_content.expect("structured content");

    assert_eq!(response["action"], "doctor_snapshot");
    assert_eq!(response["status"], "completed");
    assert_eq!(response["data"]["ok"], true);

    client.cancel().await?;
    Ok(())
}

async fn start_client(
    project_root: Option<&str>,
) -> anyhow::Result<rmcp::service::RunningService<rmcp::RoleClient, ()>> {
    let mut command = Command::new(server_binary());
    if let Some(root) = project_root {
        command.env("PLATYPUS_MCP_ROOT", root);
    }
    let transport = TokioChildProcess::new(command)?;
    Ok(().serve(transport).await?)
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

fn json_args(value: Value) -> JsonObject {
    value.as_object().expect("JSON object").clone()
}

fn project_fixture() -> TempDir {
    let temp = TempDir::new().expect("temp dir");
    fs::write(temp.path().join("platy.yaml"), "project: test\n").expect("config");
    fs::create_dir(temp.path().join(".git")).expect("git metadata");
    fs::create_dir_all(temp.path().join("backlog/items")).expect("items dir");
    fs::create_dir_all(temp.path().join("backlog/epics")).expect("epics dir");
    fs::write(temp.path().join("backlog/items/PROJ-001.md"), "# item\n").expect("item");
    temp
}
