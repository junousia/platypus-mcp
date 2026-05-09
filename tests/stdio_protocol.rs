use rmcp::{
    model::{
        CallToolRequestParams, GetPromptRequestParams, JsonObject, PromptMessageContent,
        ReadResourceRequestParams, ResourceContents,
    },
    transport::TokioChildProcess,
    ServiceExt,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tokio::process::Command;

use platypus_mcp::tasks::{record_task_event, NewTaskEvent};

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
    assert!(tool_names.contains(&"inspect_work_queue"));
    assert!(tool_names.contains(&"classify_planning_needs"));
    assert!(tool_names.contains(&"classify_workflow_fit"));
    assert!(tool_names.contains(&"record_finding"));
    assert!(tool_names.contains(&"draft_external_backlog_items"));
    assert!(tool_names.contains(&"import_github_issues"));
    assert!(tool_names.contains(&"draft_external_report"));
    assert!(tool_names.contains(&"request_external_report_approval"));
    assert!(tool_names.contains(&"record_external_report_dispatch"));
    assert!(tool_names.contains(&"draft_task_plan"));
    assert!(tool_names.contains(&"inspect_task_plan"));
    assert!(tool_names.contains(&"list_task_plans"));
    assert!(tool_names.contains(&"validate_task_plan"));
    assert!(tool_names.contains(&"write_task_plan"));
    assert!(tool_names.contains(&"inspect_task"));
    assert!(tool_names.contains(&"claim_next_task"));
    assert!(tool_names.contains(&"worktree_create"));
    assert!(tool_names.contains(&"worktree_status"));
    assert!(tool_names.contains(&"worktree_diff"));
    assert!(tool_names.contains(&"inspect_worktree_changes"));
    assert!(tool_names.contains(&"worktree_cleanup"));
    assert!(tool_names.contains(&"integrate_worker_result"));
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
    assert!(tool_names.contains(&"acquire_lease"));
    assert!(tool_names.contains(&"list_leases"));
    assert!(tool_names.contains(&"renew_lease"));
    assert!(tool_names.contains(&"release_lease"));
    assert!(tool_names.contains(&"events_replay"));
    assert!(tool_names.contains(&"storage_capability_probe"));
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
async fn tool_cli_invokes_read_only_mcp_tool() -> anyhow::Result<()> {
    let project = assignment_project_fixture();
    let output = Command::new(server_binary())
        .args([
            "tool",
            "--root",
            project.path().to_string_lossy().as_ref(),
            "inspect_work_queue",
            r#"{"limit":5,"require_task_plan":false}"#,
        ])
        .output()
        .await?;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(response["action"], "inspect_work_queue");
    assert_eq!(response["status"], "completed");
    assert_eq!(
        response["data"]["items"][0]["candidate"]["item_id"],
        "PROJ-001"
    );

    Ok(())
}

#[tokio::test]
async fn tool_cli_invokes_mutating_mcp_tool_and_exits_nonzero_on_failed_result(
) -> anyhow::Result<()> {
    let project = TempDir::new().expect("temp dir");
    let initialized = Command::new(server_binary())
        .args([
            "tool",
            "init_project",
            &serde_json::json!({
                "root": project.path(),
                "project_name": "CLI Smoke"
            })
            .to_string(),
        ])
        .output()
        .await?;

    assert!(
        initialized.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&initialized.stdout),
        String::from_utf8_lossy(&initialized.stderr)
    );
    let response: Value = serde_json::from_slice(&initialized.stdout)?;
    assert_eq!(response["action"], "init_project");
    assert_eq!(response["status"], "completed");
    assert!(project.path().join("platy.yaml").is_file());

    let failed = Command::new(server_binary())
        .args([
            "tool",
            "validate_backlog",
            &serde_json::json!({ "root": project.path().join("missing") }).to_string(),
        ])
        .output()
        .await?;

    assert!(!failed.status.success());
    let response: Value = serde_json::from_slice(&failed.stdout)?;
    assert_eq!(response["action"], "validate_backlog");
    assert_eq!(response["status"], "failed");

    Ok(())
}

#[tokio::test]
async fn stdio_server_lists_and_reads_host_guidance_resources() -> anyhow::Result<()> {
    let client = start_client(None).await?;

    let resources = client.list_all_resources().await?;
    let resource_uris: Vec<&str> = resources
        .iter()
        .map(|resource| resource.uri.as_str())
        .collect();

    assert!(resource_uris.contains(&"platypus://guidance/workflow"));
    assert!(resource_uris.contains(&"platypus://guidance/spec-driven-development"));
    assert!(resource_uris.contains(&"platypus://guidance/project-status"));
    assert!(resource_uris.contains(&"platypus://guidance/backlog-authoring"));
    assert!(resource_uris.contains(&"platypus://guidance/worker-handoff"));
    assert!(resource_uris.contains(&"platypus://guidance/integration-review"));
    assert!(resource_uris.contains(&"platypus://guidance/recovery"));

    let workflow = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "platypus://guidance/workflow".to_string(),
        })
        .await?;
    let text = resource_text(&workflow.contents[0]);

    assert!(text.contains("next_safe_action"));
    assert!(text.contains("prepare_worker_handoff"));
    assert!(text.contains("integrate_worker_result"));
    assert!(text.contains("reconcile_project"));
    assert!(text.contains("verification evidence"));

    let spec = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "platypus://guidance/spec-driven-development".to_string(),
        })
        .await?;
    let text = resource_text(&spec.contents[0]);
    assert!(text.contains("coding host"));
    assert!(text.contains("draft_backlog_items"));
    assert!(text.contains("write_task_plan"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_lists_and_returns_host_guidance_prompts() -> anyhow::Result<()> {
    let client = start_client(None).await?;

    let prompts = client.list_all_prompts().await?;
    let prompt_names: Vec<&str> = prompts.iter().map(|prompt| prompt.name.as_str()).collect();

    assert!(prompt_names.contains(&"platypus-workflow"));
    assert!(prompt_names.contains(&"platypus-spec-driven-development"));
    assert!(prompt_names.contains(&"platypus-project-status"));
    assert!(prompt_names.contains(&"platypus-backlog-authoring"));
    assert!(prompt_names.contains(&"platypus-worker-handoff"));
    assert!(prompt_names.contains(&"platypus-integration-review"));
    assert!(prompt_names.contains(&"platypus-recovery"));

    let prompt = client
        .get_prompt(GetPromptRequestParams {
            meta: None,
            name: "platypus-integration-review".to_string(),
            arguments: None,
        })
        .await?;
    let text = prompt_text(&prompt.messages[0]);

    assert!(text.contains("inspect_worktree_changes"));
    assert!(text.contains("record_verification_evidence"));
    assert!(text.contains("integrate_worker_result"));
    assert!(text.contains("Platypus-Closes"));

    let prompt = client
        .get_prompt(GetPromptRequestParams {
            meta: None,
            name: "platypus-spec-driven-development".to_string(),
            arguments: None,
        })
        .await?;
    let text = prompt_text(&prompt.messages[0]);
    assert!(text.contains("controlled loop"));
    assert!(text.contains("validate_backlog"));
    assert!(text.contains("reconcile_project"));

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
async fn stdio_server_drafts_writes_and_validates_task_plan() -> anyhow::Result<()> {
    let project = assignment_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let drafted =
        call_tool_json(&client, "draft_task_plan", json!({ "item_id": "PROJ-001" })).await?;
    assert_stage_status("draft_task_plan", &drafted, "completed");
    assert_eq!(drafted["data"]["plan"]["item_id"], "PROJ-001");
    assert_eq!(drafted["data"]["plan"]["tasks"][0]["id"], "PROJ-001-T01");

    let written = call_tool_json(
        &client,
        "write_task_plan",
        json!({
            "item_id": "PROJ-001",
            "plan": drafted["data"]["plan"].clone()
        }),
    )
    .await?;
    assert_stage_status("write_task_plan", &written, "completed");

    let validated = call_tool_json(
        &client,
        "validate_task_plan",
        json!({ "item_id": "PROJ-001", "include_errors": true }),
    )
    .await?;
    assert_stage_status("validate_task_plan", &validated, "completed");
    assert_eq!(validated["data"]["ok"], true);

    let listed = call_tool_json(&client, "list_task_plans", json!({})).await?;
    assert_stage_status("list_task_plans", &listed, "completed");
    assert_eq!(listed["data"]["returned"], 1);

    let inspected = call_tool_json(
        &client,
        "inspect_task_plan",
        json!({ "item_id": "PROJ-001" }),
    )
    .await?;
    assert_stage_status("inspect_task_plan", &inspected, "completed");
    assert_eq!(
        inspected["data"]["plan"]["tasks"][0]["verification"][0],
        "make check"
    );

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_inspects_work_queue_with_task_plan_state() -> anyhow::Result<()> {
    let project = assignment_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let missing = call_tool_json(
        &client,
        "inspect_work_queue",
        json!({ "limit": 5, "require_task_plan": true }),
    )
    .await?;
    assert_stage_status("inspect_work_queue missing plan", &missing, "completed");
    assert_eq!(missing["data"]["recommended_tool"], "draft_task_plan");
    assert_eq!(
        missing["data"]["items"][0]["candidate"]["item_id"],
        "PROJ-001"
    );
    assert_eq!(missing["data"]["items"][0]["plan"]["status"], "missing");
    assert_eq!(missing["data"]["items"][0]["ready_to_dispatch"], false);

    let drafted =
        call_tool_json(&client, "draft_task_plan", json!({ "item_id": "PROJ-001" })).await?;
    let written = call_tool_json(
        &client,
        "write_task_plan",
        json!({
            "item_id": "PROJ-001",
            "plan": drafted["data"]["plan"].clone()
        }),
    )
    .await?;
    assert_stage_status("write_task_plan", &written, "completed");

    let ready = call_tool_json(
        &client,
        "inspect_work_queue",
        json!({ "limit": 5, "require_task_plan": true }),
    )
    .await?;
    assert_stage_status("inspect_work_queue ready", &ready, "completed");
    assert_eq!(ready["data"]["recommended_tool"], "dispatch_ready_work");
    assert_eq!(ready["data"]["items"][0]["plan"]["status"], "valid");
    assert_eq!(
        ready["data"]["items"][0]["planning"]["required_mode"],
        "standard"
    );
    assert_eq!(ready["data"]["items"][0]["plan"]["task_count"], 1);
    assert_eq!(ready["data"]["items"][0]["ready_to_dispatch"], true);

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_classifies_planning_needs() -> anyhow::Result<()> {
    let project = assignment_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let classified = call_tool_json(
        &client,
        "classify_planning_needs",
        json!({ "item_id": "PROJ-001", "limit": 5 }),
    )
    .await?;
    assert_stage_status("classify_planning_needs", &classified, "completed");
    assert_eq!(classified["data"]["returned"], 1);
    assert_eq!(
        classified["data"]["classifications"][0]["required_mode"],
        "standard"
    );
    assert_eq!(
        classified["data"]["classifications"][0]["required_artifact"],
        "backlog/plans/PROJ-001.yaml"
    );

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
async fn stdio_server_inspects_backlog_inventory() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    git(project.path(), &["init"]);
    git(project.path(), &["config", "user.name", "Platypus Test"]);
    git(
        project.path(),
        &["config", "user.email", "platypus@example.invalid"],
    );
    fs::write(project.path().join("platy.yaml"), "project: test\n")?;
    fs::create_dir_all(project.path().join("backlog/items"))?;
    fs::create_dir_all(project.path().join("backlog/epics"))?;
    fs::write(
        project.path().join("backlog/epics/general.md"),
        r#"---
id: general
title: General
status: active
priority: P1
area: general
---

# General
"#,
    )?;
    write_backlog_item(project.path(), "PROJ-001", "First item", &[])?;
    write_backlog_item(project.path(), "PROJ-002", "Second item", &["PROJ-001"])?;
    write_backlog_item(project.path(), "PROJ-003", "Third item", &["PROJ-002"])?;
    git(project.path(), &["add", "--all"]);
    git(
        project.path(),
        &[
            "commit",
            "-m",
            "Complete first item",
            "-m",
            "Platypus-Closes: PROJ-001",
            "-m",
            "Platypus-Verification: make check",
        ],
    );
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let result = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_backlog_inventory".into(),
            arguments: Some(json_args(json!({ "limit": 10 }))),
            task: None,
        })
        .await?;
    let response = result.structured_content.expect("inventory content");

    assert_eq!(response["action"], "inspect_backlog_inventory");
    assert_eq!(response["status"], "completed");
    assert_eq!(response["data"]["total"], 3);
    assert_eq!(response["data"]["returned"], 3);
    assert_eq!(response["data"]["truncated"], false);
    assert_eq!(response["data"]["closed"], 1);
    assert_eq!(response["data"]["runnable"], 1);
    assert_eq!(response["data"]["blocked"], 1);
    let items = response["data"]["items"].as_array().expect("items");
    let closed = items
        .iter()
        .find(|item| item["item_id"] == "PROJ-001")
        .expect("closed item");
    assert_eq!(closed["closed"], true);
    assert!(closed["reason"]
        .as_str()
        .expect("closed reason")
        .contains("Platypus-Closes"));
    let blocked = items
        .iter()
        .find(|item| item["item_id"] == "PROJ-003")
        .expect("blocked item");
    assert_eq!(blocked["open_dependencies"][0], "PROJ-002");

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
    assert!(project.path().join("AGENTS.md").is_file());
    assert!(project.path().join("CLAUDE.md").is_file());
    assert!(project.path().join("backlog/epics/general.md").is_file());
    let claude = fs::read_to_string(project.path().join("CLAUDE.md"))?;
    assert!(claude.contains("spec-driven development"));
    assert!(claude.contains("draft_task_plan"));

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
async fn stdio_server_drafts_external_backlog_items_and_dedupes_refs() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;
    let initialized = call_tool_json(
        &client,
        "init_project",
        json!({ "project_name": "External Intake" }),
    )
    .await?;
    assert_stage_status("init_project", &initialized, "completed");

    let created = call_tool_json(
        &client,
        "create_backlog_item",
        json!({
            "id": "PROJ-001",
            "title": "Imported issue",
            "priority": "P1",
            "type": "feature",
            "area": "integrations",
            "epic": "general",
            "suggested_worker": "coder",
            "owned_surfaces": ["src"],
            "external_refs": [{
                "provider": "github",
                "kind": "issue",
                "id": "owner/repo#1",
                "url": "https://github.com/owner/repo/issues/1",
                "source_hash": "sha256:one"
            }],
            "goal": "Import one issue.",
            "implementation_contract": "Keep the item local.",
            "acceptance": ["The item validates."]
        }),
    )
    .await?;
    assert_stage_status("create_backlog_item", &created, "completed");

    let drafted = call_tool_json(
        &client,
        "draft_external_backlog_items",
        json!({
            "provider": "github",
            "owned_surfaces": ["src"],
            "records": [
                {
                    "kind": "issue",
                    "id": "owner/repo#1",
                    "title": "Already imported",
                    "body": "Existing work.",
                    "url": "https://github.com/owner/repo/issues/1",
                    "labels": ["p0"],
                    "source_hash": "sha256:one"
                },
                {
                    "kind": "issue",
                    "id": "owner/repo#2",
                    "title": "New imported work",
                    "body": "New work.",
                    "url": "https://github.com/owner/repo/issues/2",
                    "labels": ["docs", "area:integrations"],
                    "source_hash": "sha256:two"
                }
            ]
        }),
    )
    .await?;

    assert_stage_status("draft_external_backlog_items", &drafted, "completed");
    assert_eq!(drafted["data"]["returned"], 2);
    assert_eq!(drafted["data"]["skipped"], 1);
    assert_eq!(drafted["data"]["drafts"][0]["skipped"], true);
    assert_eq!(drafted["data"]["drafts"][1]["type"], "docs");
    assert_eq!(
        drafted["data"]["drafts"][1]["external_ref"]["id"],
        "owner/repo#2"
    );

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_imports_github_issues_as_backlog_snapshots() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;
    let initialized = call_tool_json(
        &client,
        "init_project",
        json!({ "project_name": "GitHub Import" }),
    )
    .await?;
    assert_stage_status("init_project", &initialized, "completed");

    let imported = call_tool_json(
        &client,
        "import_github_issues",
        json!({
            "owner": "owner",
            "repo": "repo",
            "id_prefix": "GH",
            "owned_surfaces": ["src"],
            "issues": [
                {
                    "number": 1,
                    "title": "Build imported feature",
                    "body": "Implement this from GitHub.",
                    "state": "open",
                    "url": "https://github.com/owner/repo/issues/1",
                    "labels": ["p0", "feature", "area:integrations"],
                    "updated_at": "2026-05-08T00:00:00Z"
                },
                {
                    "number": 2,
                    "title": "Closed issue",
                    "body": "Do not import by default.",
                    "state": "closed",
                    "url": "https://github.com/owner/repo/issues/2",
                    "labels": ["docs"]
                }
            ]
        }),
    )
    .await?;

    assert_stage_status("import_github_issues", &imported, "completed");
    assert_eq!(imported["data"]["imported_count"], 1);
    assert_eq!(imported["data"]["skipped_count"], 1);
    assert_eq!(imported["data"]["imported"][0]["item_id"], "GH-001");

    let validation = call_tool_json(&client, "validate_backlog", json!({})).await?;
    assert_stage_status("validate_backlog", &validation, "completed");

    let listed = call_tool_json(&client, "list_backlog", json!({})).await?;
    assert_stage_status("list_backlog", &listed, "completed");
    assert_eq!(
        listed["data"]["candidates"][0]["external_refs"][0]["provider"],
        "github"
    );
    assert_eq!(
        listed["data"]["candidates"][0]["external_refs"][0]["id"],
        "owner/repo#1"
    );

    let skipped = call_tool_json(
        &client,
        "import_github_issues",
        json!({
            "owner": "owner",
            "repo": "repo",
            "id_prefix": "GH",
            "issues": [{
                "number": 1,
                "title": "Build imported feature",
                "body": "Implement this from GitHub.",
                "state": "open",
                "url": "https://github.com/owner/repo/issues/1",
                "labels": ["p0"]
            }]
        }),
    )
    .await?;
    assert_stage_status("import_github_issues duplicate", &skipped, "skipped");
    assert_eq!(skipped["data"]["imported_count"], 0);
    assert_eq!(
        skipped["data"]["skipped"][0]["reason"],
        "external reference already imported"
    );

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_drafts_approves_and_records_external_report() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;
    let initialized = call_tool_json(
        &client,
        "init_project",
        json!({ "project_name": "External Report" }),
    )
    .await?;
    assert_stage_status("init_project", &initialized, "completed");

    let imported = call_tool_json(
        &client,
        "import_github_issues",
        json!({
            "owner": "owner",
            "repo": "repo",
            "id_prefix": "GH",
            "owned_surfaces": ["src"],
            "issues": [{
                "number": 1,
                "title": "Build external report feature",
                "body": "Track progress back to GitHub.",
                "state": "open",
                "url": "https://github.com/owner/repo/issues/1",
                "labels": ["feature"]
            }]
        }),
    )
    .await?;
    assert_stage_status("import_github_issues", &imported, "completed");

    let evidence = call_tool_json(
        &client,
        "record_evidence",
        json!({
            "source_item_id": "GH-001",
            "kind": "note",
            "summary": "Implementation has a safe local draft.",
            "refs": ["backlog:GH-001"]
        }),
    )
    .await?;
    assert_stage_status("record_evidence", &evidence, "completed");

    let drafted = call_tool_json(
        &client,
        "draft_external_report",
        json!({
            "source_item_id": "GH-001",
            "report_type": "status",
            "evidence_limit": 5
        }),
    )
    .await?;
    assert_stage_status("draft_external_report", &drafted, "completed");
    assert_eq!(drafted["data"]["draft"]["provider"], "github");
    assert_eq!(drafted["data"]["draft"]["kind"], "issue");
    assert_eq!(drafted["data"]["draft"]["external_id"], "owner/repo#1");
    assert_eq!(drafted["data"]["draft"]["requires_approval"], true);
    assert_eq!(
        drafted["data"]["evidence"]
            .as_array()
            .expect("evidence")
            .len(),
        1
    );

    let approval = call_tool_json(
        &client,
        "request_external_report_approval",
        json!({ "draft": drafted["data"]["draft"].clone(), "requested_by": "manager" }),
    )
    .await?;
    assert_stage_status("request_external_report_approval", &approval, "completed");
    let approval_id = string_at(&approval, &["data", "approval", "id"], "approval id");

    let pending_dispatch = call_tool_json(
        &client,
        "record_external_report_dispatch",
        json!({
            "approval_id": approval_id,
            "provider": "github",
            "kind": "issue",
            "external_id": "owner/repo#1",
            "report_type": "status",
            "status": "sent",
            "summary": "Posted status update.",
            "outbound_ref": "https://github.com/owner/repo/issues/1#issuecomment-1"
        }),
    )
    .await?;
    assert_stage_status(
        "record_external_report_dispatch pending",
        &pending_dispatch,
        "skipped",
    );

    let approved = call_tool_json(
        &client,
        "approval_respond",
        json!({
            "approval_id": string_at(&approval, &["data", "approval", "id"], "approval id"),
            "decision": "approved",
            "responder": "user"
        }),
    )
    .await?;
    assert_stage_status("approval_respond", &approved, "completed");

    let dispatched = call_tool_json(
        &client,
        "record_external_report_dispatch",
        json!({
            "approval_id": string_at(&approval, &["data", "approval", "id"], "approval id"),
            "provider": "github",
            "kind": "issue",
            "external_id": "owner/repo#1",
            "report_type": "status",
            "status": "sent",
            "summary": "Posted status update.",
            "outbound_ref": "https://github.com/owner/repo/issues/1#issuecomment-1",
            "metadata": {
                "request_token": "secret-value",
                "provider_request_id": "req-1"
            }
        }),
    )
    .await?;
    assert_stage_status("record_external_report_dispatch", &dispatched, "completed");
    assert_eq!(dispatched["data"]["status"], "sent");
    assert_eq!(
        dispatched["data"]["event"]["event_type"],
        "external_report_dispatch"
    );
    assert_eq!(dispatched["data"]["evidence"]["kind"], "external_report");
    assert_eq!(
        dispatched["data"]["evidence"]["metadata"]["request_token"],
        "[redacted]"
    );

    let denied_approval = call_tool_json(
        &client,
        "request_external_report_approval",
        json!({ "draft": drafted["data"]["draft"].clone(), "requested_by": "manager" }),
    )
    .await?;
    assert_stage_status(
        "request_external_report_approval denied",
        &denied_approval,
        "completed",
    );
    let denied_id = string_at(
        &denied_approval,
        &["data", "approval", "id"],
        "denied approval id",
    );
    let denied = call_tool_json(
        &client,
        "approval_respond",
        json!({
            "approval_id": denied_id,
            "decision": "denied",
            "responder": "user",
            "reason": "Needs review."
        }),
    )
    .await?;
    assert_stage_status("approval_respond denied", &denied, "completed");
    let denied_dispatch = call_tool_json(
        &client,
        "record_external_report_dispatch",
        json!({
            "approval_id": string_at(&denied_approval, &["data", "approval", "id"], "denied approval id"),
            "provider": "github",
            "kind": "issue",
            "external_id": "owner/repo#1",
            "report_type": "status",
            "status": "sent",
            "summary": "Posted status update."
        }),
    )
    .await?;
    assert_stage_status(
        "record_external_report_dispatch denied",
        &denied_dispatch,
        "skipped",
    );

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_runs_storage_capability_probe() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let probe = call_tool_json(&client, "storage_capability_probe", json!({})).await?;
    assert_stage_status("storage_capability_probe", &probe, "completed");
    assert_eq!(probe["data"]["backend"], "sqlite");
    assert_eq!(probe["data"]["ok"], true);
    let checks = probe["data"]["checks"].as_array().expect("checks");
    assert!(checks
        .iter()
        .any(|check| check["name"] == "schema_repeatability"));
    assert!(checks.iter().any(|check| check["name"] == "lease_conflict"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_dispatches_ready_work_with_handoffs() -> anyhow::Result<()> {
    let project = dispatch_project_fixture();
    write_backlog_item(project.path(), "PROJ-002", "Second independent work", &[])?;
    git(project.path(), &["add", "backlog/items/PROJ-002.md"]);
    git(project.path(), &["commit", "-m", "Add second backlog item"]);
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let result = call_tool_json(
        &client,
        "dispatch_ready_work",
        json!({
            "max_tasks": 2,
            "worker": "coder",
            "claimant": "stdio-batch",
            "verification_command": ["make", "check"]
        }),
    )
    .await?;

    assert_stage_status("dispatch_ready_work", &result, "completed");
    assert_eq!(result["data"]["dispatched"], 2);
    assert_eq!(result["data"]["prepared"], 2);
    let items = result["data"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["status"], "prepared");
    assert_eq!(items[1]["status"], "prepared");
    assert_eq!(items[0]["item_id"], "PROJ-001");
    assert_eq!(items[1]["item_id"], "PROJ-002");
    assert!(items[0]["assignment"]["worktree_path"]
        .as_str()
        .expect("worktree path")
        .contains(".platy/worktrees"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_blocks_dispatch_with_active_project_lease() -> anyhow::Result<()> {
    let project = dispatch_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let lease = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "acquire_lease".into(),
            arguments: Some(json_args(json!({
                "scope": "project",
                "target_id": "root",
                "owner": "manager",
                "ttl_seconds": 60
            }))),
            task: None,
        })
        .await?;
    let lease = lease.structured_content.expect("lease content");
    assert_eq!(lease["status"], "completed");
    let lease_id = lease["data"]["lease"]["id"].as_str().expect("lease id");

    let blocked = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "dispatch_next_work".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let blocked = blocked.structured_content.expect("dispatch content");
    assert_eq!(blocked["status"], "skipped");
    assert!(blocked["summary"]
        .as_str()
        .expect("summary")
        .contains("leased by `manager`"));

    let released = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "release_lease".into(),
            arguments: Some(json_args(json!({
                "lease_id": lease_id,
                "owner": "manager"
            }))),
            task: None,
        })
        .await?;
    assert_eq!(
        released.structured_content.expect("release")["status"],
        "completed"
    );

    let dispatched = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "dispatch_next_work".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    assert_eq!(
        dispatched.structured_content.expect("dispatch")["status"],
        "completed"
    );

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_rejects_duplicate_active_dispatch_for_same_item() -> anyhow::Result<()> {
    let project = dispatch_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    for expected_status in ["completed", "skipped"] {
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
        if expected_status == "skipped" {
            assert!(response["summary"]
                .as_str()
                .expect("summary")
                .contains("No runnable backlog items"));
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
async fn stdio_server_recovers_after_failed_worker_handoff() -> anyhow::Result<()> {
    let project = assignment_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let dispatched = call_tool_json(&client, "dispatch_next_work", json!({})).await?;
    assert_stage_status("dispatch_next_work", &dispatched, "completed");
    let task_id = string_at(&dispatched, &["data", "task", "id"], "task id");

    let claimed = call_tool_json(
        &client,
        "claim_next_task",
        json!({
            "worker": "coder",
            "claimant": "runner-that-crashed-before-handoff"
        }),
    )
    .await?;
    assert_stage_status("claim_next_task", &claimed, "completed");
    assert_eq!(claimed["data"]["task"]["id"], task_id);

    let failed = call_tool_json(
        &client,
        "prepare_worker_handoff",
        json!({
            "task_id": task_id,
            "worker": "coder",
            "claimant": "stdio-test",
            "base_ref": "missing-ref-for-recovery-test",
            "verification_command": ["make", "check"]
        }),
    )
    .await?;
    assert_stage_status("prepare_worker_handoff failed", &failed, "failed");
    assert!(failed["summary"]
        .as_str()
        .expect("summary")
        .contains("worktree"));

    let inspected = call_tool_json(
        &client,
        "inspect_task",
        json!({
            "task_id": task_id
        }),
    )
    .await?;
    assert_stage_status("inspect_task", &inspected, "completed");
    assert_eq!(inspected["data"]["task"]["status"], "queued");
    assert!(inspected["data"]["task"]["claimed_by"].is_null());
    assert!(inspected["data"]["task"]["claimed_at"].is_null());

    let events = call_tool_json(
        &client,
        "inspect_task_events",
        json!({
            "task_id": task_id
        }),
    )
    .await?;
    assert_stage_status("inspect_task_events", &events, "completed");
    let event_types = events["data"]["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|event| event["event_type"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(event_types.contains(&"worker_assignment_failed"));

    let guidance = call_tool_json(&client, "next_safe_action", json!({})).await?;
    assert_stage_status("next_safe_action", &guidance, "completed");
    assert_eq!(
        guidance["data"]["recommended_tool"],
        "prepare_worker_handoff"
    );
    assert_eq!(guidance["data"]["params"]["task_id"], task_id);

    let prepared = call_tool_json(
        &client,
        "prepare_worker_handoff",
        json!({
            "task_id": task_id,
            "worker": "coder",
            "claimant": "stdio-test",
            "verification_command": ["make", "check"]
        }),
    )
    .await?;
    assert_stage_status("prepare_worker_handoff retry", &prepared, "completed");
    assert_eq!(prepared["data"]["assignment"]["status"], "prepared");

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_runner_prepare_next_persists_worker_assignment() -> anyhow::Result<()> {
    let project = assignment_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let dispatched = call_tool_json(&client, "dispatch_next_work", json!({})).await?;
    assert_stage_status("dispatch_next_work", &dispatched, "completed");
    let task_id = string_at(&dispatched, &["data", "task", "id"], "task id");

    let prepared = call_tool_json(
        &client,
        "runner_prepare_next",
        json!({
            "worker": "coder",
            "claimant": "runner-stdio",
            "max_tasks": 1,
            "verification_command": ["make", "check"]
        }),
    )
    .await?;
    assert_stage_status("runner_prepare_next", &prepared, "completed");
    let assignment_id = prepared["data"]["tasks"][0]["assignment_id"]
        .as_str()
        .unwrap_or_else(|| panic!("assignment id missing: {prepared:#}"))
        .to_string();

    let inspected = call_tool_json(
        &client,
        "inspect_worker_assignment",
        json!({ "assignment_id": assignment_id }),
    )
    .await?;
    assert_stage_status("inspect_worker_assignment", &inspected, "completed");
    assert_eq!(inspected["data"]["assignment"]["task_id"], task_id);
    assert_eq!(inspected["data"]["assignment"]["status"], "prepared");

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
    assert_eq!(initial["data"]["recommended_tool"], "dispatch_ready_work");

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
                "source_task_id": task_id.clone(),
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

    let integration_guidance = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "next_safe_action".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let integration_guidance = integration_guidance
        .structured_content
        .expect("integration guidance");
    assert_eq!(
        integration_guidance["data"]["recommended_tool"],
        "integrate_worker_result"
    );
    assert_eq!(integration_guidance["data"]["params"]["task_id"], task_id);

    let reconciled = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "reconcile_project".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let reconciled = reconciled.structured_content.expect("reconcile content");
    assert_eq!(reconciled["status"], "failed");
    assert_eq!(reconciled["data"]["ok"], false);
    assert!(reconciled["data"]["gaps"]
        .as_array()
        .expect("gaps")
        .iter()
        .any(|gap| gap["kind"] == "missing_integration_evidence"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_runs_full_lifecycle_smoke_with_fake_worker() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let initialized = call_tool_json(
        &client,
        "init_project",
        json!({ "project_name": "Lifecycle Smoke" }),
    )
    .await?;
    assert_stage_status("init_project", &initialized, "completed");
    assert!(project.path().join("backlog/items").is_dir());

    git(project.path(), &["init"]);
    git(project.path(), &["config", "user.name", "Platypus Test"]);
    git(
        project.path(),
        &["config", "user.email", "platypus@example.invalid"],
    );

    let created = call_tool_json(
        &client,
        "create_backlog_item",
        json!({
            "id": "PROJ-001",
            "title": "Create smoke output",
            "priority": "P1",
            "type": "feature",
            "area": "verification",
            "epic": "general",
            "suggested_worker": "coder",
            "owned_surfaces": ["README.md"],
            "goal": "Create a visible smoke-test output file.",
            "implementation_contract": "Only write README.md in the assigned worktree.",
            "acceptance": ["README.md exists with smoke-test content."]
        }),
    )
    .await?;
    assert_stage_status("create_backlog_item", &created, "completed");

    git(project.path(), &["add", "--all"]);
    git(
        project.path(),
        &["commit", "-m", "Initialize smoke project"],
    );

    let initial = call_tool_json(&client, "next_safe_action", json!({})).await?;
    assert_eq!(
        initial["data"]["recommended_tool"], "dispatch_ready_work",
        "stage next_safe_action before dispatch: {initial:#}"
    );

    let dispatched = call_tool_json(&client, "dispatch_next_work", json!({})).await?;
    assert_stage_status("dispatch_next_work", &dispatched, "completed");
    let task_id = string_at(&dispatched, &["data", "task", "id"], "task id");

    let prepared = call_tool_json(
        &client,
        "prepare_worker_handoff",
        json!({
            "task_id": task_id,
            "worker": "coder",
            "claimant": "stdio-smoke",
            "verification_command": ["cargo", "test"]
        }),
    )
    .await?;
    assert_stage_status("prepare_worker_handoff", &prepared, "completed");
    let assignment_id = string_at(&prepared, &["data", "assignment", "id"], "assignment id");
    let worktree_path = PathBuf::from(string_at(
        &prepared,
        &["data", "assignment", "worktree_path"],
        "worktree path",
    ));

    let started = call_tool_json(
        &client,
        "start_worker_task",
        json!({
            "assignment_id": assignment_id,
            "worker_session": "fake-worker-stdio"
        }),
    )
    .await?;
    assert_stage_status("start_worker_task", &started, "completed");

    fs::write(
        worktree_path.join("README.md"),
        "# Lifecycle Smoke\n\nFake worker completed the MCP lifecycle.\n",
    )?;

    let changed = call_tool_json(
        &client,
        "inspect_worktree_changes",
        json!({ "task_id": task_id }),
    )
    .await?;
    assert_stage_status("inspect_worktree_changes", &changed, "completed");
    assert_eq!(changed["data"]["dirty"], true);

    let progress = call_tool_json(
        &client,
        "record_worker_progress",
        json!({
            "assignment_id": assignment_id,
            "event_type": "worker_progress",
            "summary": "Fake worker wrote README.md."
        }),
    )
    .await?;
    assert_stage_status("record_worker_progress", &progress, "completed");

    let completed = call_tool_json(
        &client,
        "complete_worker_task",
        json!({
            "assignment_id": assignment_id,
            "status": "completed",
            "summary": "README.md contains lifecycle smoke output.",
            "changed_files": ["README.md"],
            "verification_status": "passed"
        }),
    )
    .await?;
    assert_stage_status("complete_worker_task", &completed, "completed");

    let verification = call_tool_json(
        &client,
        "record_verification_evidence",
        json!({
            "source_item_id": "PROJ-001",
            "source_task_id": task_id,
            "summary": "Fake worker lifecycle smoke verification passed.",
            "refs": ["local:fake-worker"]
        }),
    )
    .await?;
    assert_stage_status("record_verification_evidence", &verification, "completed");

    let integration_guidance = call_tool_json(&client, "next_safe_action", json!({})).await?;
    assert_eq!(
        integration_guidance["data"]["recommended_tool"], "integrate_worker_result",
        "stage next_safe_action before integration: {integration_guidance:#}"
    );

    let integrated = call_tool_json(
        &client,
        "integrate_worker_result",
        json!({ "task_id": task_id }),
    )
    .await?;
    assert_stage_status("integrate_worker_result", &integrated, "completed");
    let integration_commit = string_at(&integrated, &["data", "commit"], "integration commit");

    assert_eq!(
        fs::read_to_string(project.path().join("README.md"))?,
        "# Lifecycle Smoke\n\nFake worker completed the MCP lifecycle.\n"
    );
    let commit_message = git_stdout(project.path(), &["show", "-s", "--format=%B", "HEAD"]);
    assert!(commit_message.contains("Platypus-Closes: PROJ-001"));
    assert!(commit_message
        .contains("Platypus-Verification: Fake worker lifecycle smoke verification passed."));

    let evidence = call_tool_json(
        &client,
        "list_evidence",
        json!({
            "source_item_id": "PROJ-001",
            "source_task_id": task_id,
            "kind": "commit"
        }),
    )
    .await?;
    assert_stage_status("list_evidence commit", &evidence, "completed");
    assert_eq!(evidence["data"]["returned"], 1);
    assert_eq!(
        evidence["data"]["evidence"][0]["refs"][0],
        format!("commit:{integration_commit}")
    );

    let reconciled = call_tool_json(&client, "reconcile_project", json!({})).await?;
    assert_stage_status("reconcile_project", &reconciled, "completed");
    assert_eq!(reconciled["data"]["ok"], true);
    assert!(reconciled["data"]["gaps"]
        .as_array()
        .expect("gaps")
        .is_empty());

    let events = call_tool_json(
        &client,
        "inspect_task_events",
        json!({ "task_id": task_id, "limit": 50 }),
    )
    .await?;
    assert_stage_status("inspect_task_events", &events, "completed");
    assert!(events["data"]["events"]
        .as_array()
        .expect("events")
        .iter()
        .any(|event| event["event_type"] == "worker_result_integrated"));

    let cleaned =
        call_tool_json(&client, "worktree_cleanup", json!({ "task_id": task_id })).await?;
    assert_stage_status("worktree_cleanup", &cleaned, "completed");
    assert!(!worktree_path.exists());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_skips_dispatch_when_backlog_has_no_runnable_items() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    fs::write(project.path().join("platy.yaml"), "project: test\n")?;
    fs::create_dir_all(project.path().join("backlog/items"))?;
    fs::create_dir_all(project.path().join("backlog/epics"))?;
    git(project.path(), &["init"]);
    git(project.path(), &["config", "user.name", "Platypus Test"]);
    git(
        project.path(),
        &["config", "user.email", "platypus@example.invalid"],
    );
    git(project.path(), &["add", "--all"]);
    git(project.path(), &["commit", "-m", "Initial empty backlog"]);
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
    path.push("platypus-mcp");
    path
}

fn json_args(value: Value) -> JsonObject {
    value.as_object().expect("JSON object").clone()
}

async fn call_tool_json(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    let result = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: name.to_string().into(),
            arguments: Some(json_args(arguments)),
            task: None,
        })
        .await?;
    Ok(result
        .structured_content
        .unwrap_or_else(|| panic!("stage {name}: missing structured content")))
}

fn assert_stage_status(stage: &str, value: &Value, expected: &str) {
    assert_eq!(
        value["status"], expected,
        "stage {stage}: expected status {expected}, got:\n{value:#}"
    );
}

fn string_at(value: &Value, path: &[&str], label: &str) -> String {
    let mut current = value;
    for segment in path {
        current = &current[*segment];
    }
    current
        .as_str()
        .unwrap_or_else(|| panic!("{label} missing at {path:?}: {value:#}"))
        .to_string()
}

fn resource_text(contents: &ResourceContents) -> &str {
    match contents {
        ResourceContents::TextResourceContents { text, .. } => text,
        ResourceContents::BlobResourceContents { .. } => panic!("expected text resource"),
    }
}

fn prompt_text(message: &rmcp::model::PromptMessage) -> &str {
    match &message.content {
        PromptMessageContent::Text { text } => text,
        _ => panic!("expected text prompt"),
    }
}

fn project_fixture() -> TempDir {
    let temp = TempDir::new().expect("temp dir");
    fs::write(temp.path().join("platy.yaml"), "project: test\n").expect("config");
    git(temp.path(), &["init"]);
    git(temp.path(), &["config", "user.name", "Platypus Test"]);
    git(
        temp.path(),
        &["config", "user.email", "platypus@example.invalid"],
    );
    fs::write(temp.path().join("README.md"), "# Test\n").expect("readme");
    git(temp.path(), &["add", "README.md"]);
    git(temp.path(), &["commit", "-m", "Initial commit"]);
    fs::create_dir_all(temp.path().join("backlog/items")).expect("items dir");
    fs::create_dir_all(temp.path().join("backlog/epics")).expect("epics dir");
    fs::write(temp.path().join("backlog/items/PROJ-001.md"), "# item\n").expect("item");
    temp
}

fn write_backlog_item(
    root: &std::path::Path,
    id: &str,
    title: &str,
    depends_on: &[&str],
) -> anyhow::Result<()> {
    let depends = if depends_on.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "\n{}",
            depends_on
                .iter()
                .map(|dependency| format!("  - {}", dependency))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    fs::write(
        root.join(format!("backlog/items/{id}.md")),
        format!(
            r#"---
id: {id}
title: {title}
priority: P1
type: feature
area: general
epic: general
depends_on: {depends}
suggested_worker: coder
owned_surfaces: []
---

# {id} {title}

## Goal

Goal.

## Implementation Contract

Contract.

## Acceptance

- Done.
"#
        ),
    )?;
    Ok(())
}

fn dispatch_project_fixture() -> TempDir {
    let temp = TempDir::new().expect("temp dir");
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
    git(temp.path(), &["init"]);
    git(temp.path(), &["config", "user.name", "Platypus Test"]);
    git(
        temp.path(),
        &["config", "user.email", "platypus@example.invalid"],
    );
    fs::write(temp.path().join("README.md"), "# Test\n").expect("readme");
    git(temp.path(), &["add", "--all"]);
    git(temp.path(), &["commit", "-m", "Initial commit"]);
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
    git(temp.path(), &["add", "platy.yaml", "backlog"]);
    git(temp.path(), &["commit", "-m", "Add Platypus scaffold"]);
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

fn git_stdout(root: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
