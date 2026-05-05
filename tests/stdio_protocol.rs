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
    assert!(tool_names.contains(&"init_project"));
    assert!(tool_names.contains(&"record_finding"));

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

#[tokio::test]
async fn stdio_server_initializes_project_scaffold() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let result = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "init_project".into(),
            arguments: Some(json_args(json!({ "project_name": "MCP Test" }))),
            task: None,
        })
        .await?;
    let response = result.structured_content.expect("structured content");

    assert_eq!(response["action"], "init_project");
    assert_eq!(response["status"], "completed");
    assert!(response["data"]["created"].as_u64().unwrap_or(0) > 0);
    assert!(project.path().join("platy.yaml").is_file());
    assert!(project.path().join("backlog/epics/general.md").is_file());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_records_lists_validates_and_dispositions_findings() -> anyhow::Result<()> {
    let project = project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let recorded = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "record_finding".into(),
            arguments: Some(json_args(json!({
                "source_item_id": "PROJ-001",
                "source_task_id": "task-1",
                "title": "Follow-up needed",
                "summary": "The worker found a required follow-up.",
                "severity": "medium",
                "required": true,
                "evidence_refs": ["output/summary.md"]
            }))),
            task: None,
        })
        .await?;
    let recorded = recorded.structured_content.expect("recorded content");
    let finding_id = recorded["data"]["finding"]["id"]
        .as_str()
        .expect("finding id")
        .to_string();
    assert_eq!(recorded["status"], "completed");

    let validation = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "validate_findings".into(),
            arguments: Some(json_args(json!({ "source_item_id": "PROJ-001" }))),
            task: None,
        })
        .await?;
    let validation = validation.structured_content.expect("validation content");
    assert_eq!(validation["status"], "failed");
    assert_eq!(validation["data"]["unresolved_required_count"], 1);

    let updated = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "update_finding_disposition".into(),
            arguments: Some(json_args(json!({
                "finding_id": finding_id,
                "status": "resolved",
                "owner": "manager",
                "disposition_reason": "Covered by follow-up work.",
                "evidence_refs": ["commit:abc123"]
            }))),
            task: None,
        })
        .await?;
    let updated = updated.structured_content.expect("updated content");
    assert_eq!(updated["status"], "completed");

    let listed = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "list_findings".into(),
            arguments: Some(json_args(json!({
                "source_item_id": "PROJ-001",
                "status": "resolved"
            }))),
            task: None,
        })
        .await?;
    let listed = listed.structured_content.expect("listed content");
    assert_eq!(listed["data"]["returned"], 1);
    assert_eq!(listed["data"]["findings"][0]["status"], "resolved");

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
