use rmcp::{
    model::{CallToolRequestParams, JsonObject},
    transport::TokioChildProcess,
    ServiceExt,
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use tempfile::TempDir;
use tokio::process::Command;

use platypus_mcp_rs::tasks::{record_task_event, NewTaskEvent};

#[tokio::test]
async fn stdio_server_lists_tools_after_initialize() -> anyhow::Result<()> {
    let client = start_client(None).await?;

    let tools = client.list_all_tools().await?;
    let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();

    assert!(tool_names.contains(&"inspect_status"));
    assert!(tool_names.contains(&"create_backlog_item"));
    assert!(tool_names.contains(&"doctor_snapshot"));
    assert!(tool_names.contains(&"init_project"));
    assert!(tool_names.contains(&"next_safe_action"));
    assert!(tool_names.contains(&"record_finding"));
    assert!(tool_names.contains(&"inspect_task"));
    assert!(tool_names.contains(&"claim_next_task"));
    assert!(tool_names.contains(&"worktree_create"));
    assert!(tool_names.contains(&"worktree_status"));
    assert!(tool_names.contains(&"worktree_diff"));
    assert!(tool_names.contains(&"inspect_worktree_changes"));
    assert!(tool_names.contains(&"worktree_cleanup"));
    assert!(tool_names.contains(&"generate_task_bundle"));
    assert!(tool_names.contains(&"prepare_worker_assignment"));
    assert!(tool_names.contains(&"prepare_worker_handoff"));
    assert!(tool_names.contains(&"inspect_worker_assignment"));
    assert!(tool_names.contains(&"start_worker_execution"));
    assert!(tool_names.contains(&"start_worker_task"));
    assert!(tool_names.contains(&"record_worker_event"));
    assert!(tool_names.contains(&"record_worker_progress"));
    assert!(tool_names.contains(&"complete_worker_execution"));
    assert!(tool_names.contains(&"complete_worker_task"));
    assert!(tool_names.contains(&"runner_prepare_next"));
    assert!(tool_names.contains(&"approval_list"));
    assert!(tool_names.contains(&"approval_respond"));
    assert!(tool_names.contains(&"events_replay"));
    assert!(tool_names.contains(&"record_evidence"));
    assert!(tool_names.contains(&"record_verification_evidence"));
    assert!(tool_names.contains(&"list_evidence"));
    assert!(tool_names.contains(&"reconcile_project"));
    assert!(tool_names.contains(&"list_agent_profiles"));
    assert!(tool_names.contains(&"configure_agent_profile"));
    assert!(tool_names.contains(&"inspect_workflow_config"));
    assert!(tool_names.contains(&"send_worker_guidance"));

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
async fn stdio_server_inspects_workflow_config() -> anyhow::Result<()> {
    let project = project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let result = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_workflow_config".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let response = result.structured_content.expect("workflow content");

    assert_eq!(response["action"], "inspect_workflow_config");
    assert_eq!(response["status"], "completed");
    assert_eq!(
        response["data"]["integration"]["merge_style"],
        "merge_commit"
    );
    assert_eq!(
        response["data"]["integration"]["require_clean_manager_workspace"],
        true
    );

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

#[tokio::test]
async fn stdio_server_replays_task_events() -> anyhow::Result<()> {
    let project = project_fixture();
    record_task_event(
        project.path(),
        None,
        NewTaskEvent {
            task_id: "task-1".to_string(),
            sequence: None,
            event_type: "worker_started".to_string(),
            summary: "Worker started.".to_string(),
            payload: Some(json!({ "worker": "coder" })),
        },
    )
    .map_err(anyhow::Error::msg)?;
    record_task_event(
        project.path(),
        None,
        NewTaskEvent {
            task_id: "task-1".to_string(),
            sequence: None,
            event_type: "worker_result".to_string(),
            summary: "Worker completed.".to_string(),
            payload: Some(json!({ "status": "completed" })),
        },
    )
    .map_err(anyhow::Error::msg)?;

    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;
    let result = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_task_events".into(),
            arguments: Some(json_args(json!({ "task_id": "task-1" }))),
            task: None,
        })
        .await?;
    let response = result.structured_content.expect("task event content");

    assert_eq!(response["status"], "completed");
    assert_eq!(response["data"]["returned"], 2);
    assert_eq!(response["data"]["events"][0]["sequence"], 1);
    assert_eq!(response["data"]["events"][1]["event_type"], "worker_result");

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_returns_skipped_for_unknown_task_events() -> anyhow::Result<()> {
    let project = project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let result = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_task_events".into(),
            arguments: Some(json_args(json!({ "task_id": "missing" }))),
            task: None,
        })
        .await?;
    let response = result.structured_content.expect("task event content");

    assert_eq!(response["status"], "skipped");
    assert_eq!(response["data"]["returned"], 0);

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_dispatches_next_work_without_running_worker() -> anyhow::Result<()> {
    let project = dispatch_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let result = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "dispatch_next_work".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let response = result.structured_content.expect("dispatch content");
    let task_id = response["data"]["task"]["id"].as_str().expect("task id");

    assert_eq!(response["status"], "completed");
    assert_eq!(response["data"]["candidate"]["item_id"], "PROJ-001");
    assert_eq!(response["data"]["task"]["status"], "queued");

    let inspected = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_task".into(),
            arguments: Some(json_args(json!({ "task_id": task_id }))),
            task: None,
        })
        .await?;
    let inspected = inspected.structured_content.expect("inspect content");

    assert_eq!(inspected["status"], "completed");
    assert_eq!(inspected["data"]["task"]["id"], task_id);

    let events = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_task_events".into(),
            arguments: Some(json_args(json!({ "task_id": task_id }))),
            task: None,
        })
        .await?;
    let events = events.structured_content.expect("events content");

    assert_eq!(events["status"], "completed");
    assert_eq!(events["data"]["events"][0]["event_type"], "task_queued");

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_rejects_duplicate_active_dispatch_for_same_item() -> anyhow::Result<()> {
    let project = dispatch_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    for expected_status in ["completed", "failed"] {
        let result = client
            .call_tool(CallToolRequestParams {
                meta: None,
                name: "dispatch_next_work".into(),
                arguments: Some(JsonObject::new()),
                task: None,
            })
            .await?;
        let response = result.structured_content.expect("dispatch content");
        assert_eq!(response["status"], expected_status);
        if expected_status == "failed" {
            assert!(response["error"]
                .as_str()
                .expect("error")
                .contains("active task already exists"));
        }
    }

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_claims_and_inspects_queued_task() -> anyhow::Result<()> {
    let project = dispatch_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "dispatch_next_work".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let claimed = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "claim_next_task".into(),
            arguments: Some(json_args(json!({
                "worker": "coder",
                "claimant": "runner-1"
            }))),
            task: None,
        })
        .await?;
    let claimed = claimed.structured_content.expect("claim content");
    let task_id = claimed["data"]["task"]["id"].as_str().expect("task id");

    assert_eq!(claimed["status"], "completed");
    assert_eq!(claimed["data"]["task"]["status"], "claimed");
    assert_eq!(claimed["data"]["task"]["claimed_by"], "runner-1");

    let inspected = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_task".into(),
            arguments: Some(json_args(json!({ "task_id": task_id }))),
            task: None,
        })
        .await?;
    let inspected = inspected.structured_content.expect("inspect content");

    assert_eq!(inspected["data"]["task"]["status"], "claimed");
    assert!(inspected["data"]["task"]["claimed_at"].is_string());

    let events = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_task_events".into(),
            arguments: Some(json_args(json!({ "task_id": task_id }))),
            task: None,
        })
        .await?;
    let events = events.structured_content.expect("events content");

    assert_eq!(events["data"]["events"][1]["event_type"], "task_claimed");

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_runs_worker_assignment_lifecycle() -> anyhow::Result<()> {
    let project = assignment_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let dispatched = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "dispatch_next_work".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let dispatched = dispatched.structured_content.expect("dispatch content");
    let task_id = dispatched["data"]["task"]["id"]
        .as_str()
        .expect("task id")
        .to_string();

    let prepared = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "prepare_worker_assignment".into(),
            arguments: Some(json_args(json!({
                "task_id": task_id,
                "worker": "coder",
                "claimant": "stdio-test",
                "verification_command": ["make", "check"]
            }))),
            task: None,
        })
        .await?;
    let prepared = prepared.structured_content.expect("prepared content");
    let assignment_id = prepared["data"]["assignment"]["id"]
        .as_str()
        .expect("assignment id")
        .to_string();

    assert_eq!(prepared["status"], "completed");
    assert_eq!(prepared["data"]["assignment"]["status"], "prepared");
    assert!(prepared["data"]["assignment"]["worktree_path"].is_string());

    let started = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "start_worker_execution".into(),
            arguments: Some(json_args(json!({
                "assignment_id": assignment_id,
                "worker_session": "subagent-stdio"
            }))),
            task: None,
        })
        .await?;
    let started = started.structured_content.expect("started content");

    assert_eq!(started["status"], "completed");
    assert_eq!(started["data"]["assignment"]["status"], "running");

    let progress = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "record_worker_event".into(),
            arguments: Some(json_args(json!({
                "assignment_id": assignment_id,
                "event_type": "worker_progress",
                "summary": "README changed."
            }))),
            task: None,
        })
        .await?;
    let progress = progress.structured_content.expect("progress content");
    assert_eq!(progress["status"], "completed");
    assert_eq!(progress["data"]["event"]["event_type"], "worker_progress");

    let completed = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "complete_worker_execution".into(),
            arguments: Some(json_args(json!({
                "assignment_id": assignment_id,
                "status": "completed",
                "summary": "README updated.",
                "changed_files": ["README.md"],
                "verification_status": "passed"
            }))),
            task: None,
        })
        .await?;
    let completed = completed.structured_content.expect("completed content");

    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["data"]["assignment"]["status"], "completed");
    assert_eq!(
        completed["data"]["assignment"]["result_status"],
        "completed"
    );

    let inspected = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_task".into(),
            arguments: Some(json_args(json!({ "task_id": task_id }))),
            task: None,
        })
        .await?;
    let inspected = inspected.structured_content.expect("inspect content");
    assert_eq!(inspected["data"]["task"]["status"], "completed");

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_guides_friendly_worker_assignment_lifecycle() -> anyhow::Result<()> {
    let project = assignment_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let initial = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "next_safe_action".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let initial = initial.structured_content.expect("initial guidance");
    assert_eq!(initial["data"]["recommended_tool"], "dispatch_next_work");

    let dispatched = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "dispatch_next_work".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let dispatched = dispatched.structured_content.expect("dispatch content");
    let task_id = dispatched["data"]["task"]["id"]
        .as_str()
        .expect("task id")
        .to_string();

    let queued = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "next_safe_action".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let queued = queued.structured_content.expect("queued guidance");
    assert_eq!(queued["data"]["recommended_tool"], "prepare_worker_handoff");
    assert_eq!(queued["data"]["params"]["task_id"], task_id);

    let prepared = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "prepare_worker_handoff".into(),
            arguments: Some(json_args(json!({
                "task_id": task_id,
                "worker": "coder",
                "claimant": "stdio-test",
                "verification_command": ["make", "check"]
            }))),
            task: None,
        })
        .await?;
    let prepared = prepared.structured_content.expect("prepared content");
    let assignment_id = prepared["data"]["assignment"]["id"]
        .as_str()
        .expect("assignment id")
        .to_string();
    let worktree_path = PathBuf::from(
        prepared["data"]["assignment"]["worktree_path"]
            .as_str()
            .expect("worktree path"),
    );

    assert_eq!(prepared["status"], "completed");
    assert_eq!(prepared["action"], "prepare_worker_handoff");
    assert_eq!(prepared["data"]["assignment"]["status"], "prepared");

    let prepared_guidance = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "next_safe_action".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let prepared_guidance = prepared_guidance
        .structured_content
        .expect("prepared guidance");
    assert_eq!(
        prepared_guidance["data"]["recommended_tool"],
        "start_worker_task"
    );
    assert_eq!(
        prepared_guidance["data"]["params"]["assignment_id"],
        assignment_id
    );

    let started = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "start_worker_task".into(),
            arguments: Some(json_args(json!({
                "assignment_id": assignment_id,
                "worker_session": "subagent-stdio"
            }))),
            task: None,
        })
        .await?;
    let started = started.structured_content.expect("started content");
    assert_eq!(started["status"], "completed");
    assert_eq!(started["action"], "start_worker_task");
    assert_eq!(started["data"]["assignment"]["status"], "running");

    let running_guidance = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "next_safe_action".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let running_guidance = running_guidance
        .structured_content
        .expect("running guidance");
    assert_eq!(
        running_guidance["data"]["recommended_tool"],
        "record_worker_progress"
    );

    fs::write(worktree_path.join("README.md"), "# Test\n\nImplemented.\n")?;

    let changes = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_worktree_changes".into(),
            arguments: Some(json_args(json!({ "task_id": task_id }))),
            task: None,
        })
        .await?;
    let changes = changes.structured_content.expect("changes content");
    assert_eq!(changes["status"], "completed");
    assert_eq!(changes["action"], "inspect_worktree_changes");
    assert_eq!(changes["data"]["dirty"], true);

    let progress = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "record_worker_progress".into(),
            arguments: Some(json_args(json!({
                "assignment_id": assignment_id,
                "event_type": "worker_progress",
                "summary": "README changed."
            }))),
            task: None,
        })
        .await?;
    let progress = progress.structured_content.expect("progress content");
    assert_eq!(progress["status"], "completed");
    assert_eq!(progress["action"], "record_worker_progress");

    let rejected_completion = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "complete_worker_task".into(),
            arguments: Some(json_args(json!({
                "assignment_id": assignment_id,
                "status": "completed",
                "summary": "README updated.",
                "changed_files": ["README.md"]
            }))),
            task: None,
        })
        .await?;
    let rejected_completion = rejected_completion
        .structured_content
        .expect("rejected completion content");
    assert_eq!(rejected_completion["status"], "failed");
    assert_eq!(rejected_completion["action"], "complete_worker_task");
    assert!(rejected_completion["error"]
        .as_str()
        .expect("error")
        .contains("verification_status is required"));

    let completed = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "complete_worker_task".into(),
            arguments: Some(json_args(json!({
                "assignment_id": assignment_id,
                "status": "completed",
                "summary": "README updated.",
                "changed_files": ["README.md"],
                "verification_status": "not_run"
            }))),
            task: None,
        })
        .await?;
    let completed = completed.structured_content.expect("completed content");
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["action"], "complete_worker_task");
    assert_eq!(completed["data"]["assignment"]["status"], "completed");
    assert!(completed["next_action"]
        .as_str()
        .expect("next action")
        .contains("record_verification_evidence"));

    let verification_guidance = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "next_safe_action".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let verification_guidance = verification_guidance
        .structured_content
        .expect("verification guidance");
    assert_eq!(
        verification_guidance["data"]["recommended_tool"],
        "record_verification_evidence"
    );
    assert_eq!(
        verification_guidance["data"]["params"]["source_task_id"],
        task_id
    );

    let evidence = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "record_verification_evidence".into(),
            arguments: Some(json_args(json!({
                "source_item_id": "PROJ-001",
                "source_task_id": task_id,
                "summary": "make check was not configured for this smoke fixture.",
                "refs": ["task-events"]
            }))),
            task: None,
        })
        .await?;
    let evidence = evidence.structured_content.expect("evidence content");
    assert_eq!(evidence["status"], "completed");
    assert_eq!(evidence["action"], "record_verification_evidence");
    assert_eq!(evidence["data"]["evidence"]["kind"], "verification");

    let reconciled = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "reconcile_project".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let reconciled = reconciled.structured_content.expect("reconcile content");
    assert_eq!(reconciled["status"], "completed");
    assert_eq!(reconciled["data"]["ok"], true);

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_skips_dispatch_when_backlog_has_no_runnable_items() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    fs::write(project.path().join("platy.yaml"), "project: test\n")?;
    fs::create_dir_all(project.path().join("backlog/items"))?;
    fs::create_dir_all(project.path().join("backlog/epics"))?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let result = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "dispatch_next_work".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let response = result.structured_content.expect("dispatch content");

    assert_eq!(response["status"], "skipped");

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

fn dispatch_project_fixture() -> TempDir {
    let temp = TempDir::new().expect("temp dir");
    fs::write(temp.path().join("platy.yaml"), "project: test\n").expect("config");
    fs::create_dir(temp.path().join(".git")).expect("git metadata");
    fs::create_dir_all(temp.path().join("backlog/items")).expect("items dir");
    fs::create_dir_all(temp.path().join("backlog/epics")).expect("epics dir");
    fs::write(
        temp.path().join("backlog/epics/general.md"),
        r#"---
id: general
title: General
status: active
priority: P1
area: general
---

# General
"#,
    )
    .expect("epic");
    fs::write(
        temp.path().join("backlog/items/PROJ-001.md"),
        r#"---
id: PROJ-001
title: First runnable work
priority: P1
type: foundation
area: general
epic: general
depends_on: []
suggested_worker: coder
owned_surfaces: []
---

# PROJ-001 First runnable work

## Goal

Create the first task.

## Implementation Contract

Queue work without running a worker.

## Acceptance

- Dispatch creates a task record.
"#,
    )
    .expect("item");
    temp
}

fn assignment_project_fixture() -> TempDir {
    let temp = TempDir::new().expect("temp dir");
    git(temp.path(), &["init"]);
    git(temp.path(), &["config", "user.name", "Platypus Test"]);
    git(
        temp.path(),
        &["config", "user.email", "platypus@example.invalid"],
    );
    fs::write(temp.path().join("README.md"), "# Test\n").expect("readme");
    git(temp.path(), &["add", "README.md"]);
    git(temp.path(), &["commit", "-m", "Initial commit"]);
    fs::write(temp.path().join("platy.yaml"), "project: test\n").expect("config");
    fs::create_dir_all(temp.path().join("backlog/items")).expect("items dir");
    fs::create_dir_all(temp.path().join("backlog/epics")).expect("epics dir");
    fs::write(
        temp.path().join("backlog/epics/general.md"),
        r#"---
id: general
title: General
status: active
priority: P1
area: general
---

# General
"#,
    )
    .expect("epic");
    fs::write(
        temp.path().join("backlog/items/PROJ-001.md"),
        r#"---
id: PROJ-001
title: First assignment work
priority: P1
type: foundation
area: general
epic: general
depends_on: []
suggested_worker: coder
owned_surfaces:
- README.md
---

# PROJ-001 First assignment work

## Goal

Create the first assignment.

## Implementation Contract

Keep changes in README.md.

## Acceptance

- Assignment completes.
"#,
    )
    .expect("item");
    temp
}

fn git(root: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git command");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
