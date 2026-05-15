use rmcp::{
    model::{
        CallToolRequestParams, GetPromptRequestParams, JsonObject, PromptMessageContent,
        ReadResourceRequestParams, ResourceContents,
    },
    transport::TokioChildProcess,
    ClientHandler, ServiceExt,
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

    assert!(tool_names.contains(&"inspect_toolsets"));
    assert!(tool_names.contains(&"inspect_status"));
    assert!(tool_names.contains(&"create_backlog_item"));
    assert!(tool_names.contains(&"quick_create_backlog_item"));
    assert!(tool_names.contains(&"create_backlog_items"));
    assert!(tool_names.contains(&"update_backlog_item"));
    assert!(tool_names.contains(&"create_epic"));
    assert!(tool_names.contains(&"list_epics"));
    assert!(tool_names.contains(&"doctor_snapshot"));
    assert!(tool_names.contains(&"init_project"));
    assert!(!tool_names.contains(&"next_safe_action"));
    assert!(tool_names.contains(&"inspect_session"));
    assert!(tool_names.contains(&"inspect_work_queue"));
    assert!(tool_names.contains(&"inspect_queue_status"));
    assert!(tool_names.contains(&"inspect_item"));
    assert!(tool_names.contains(&"get_backlog_item"));
    assert!(!tool_names.contains(&"classify_planning_needs"));
    assert!(!tool_names.contains(&"classify_workflow_fit"));
    assert!(!tool_names.contains(&"classify_goal_workflow"));
    assert!(!tool_names.contains(&"plan_goal_work"));
    assert!(!tool_names.contains(&"start_goal_work"));
    assert!(tool_names.contains(&"record_finding"));
    assert!(tool_names.contains(&"inspect_dependency_graph"));
    assert!(tool_names.contains(&"draft_external_backlog_items"));
    assert!(tool_names.contains(&"import_github_issues"));
    assert!(tool_names.contains(&"draft_external_report"));
    assert!(tool_names.contains(&"request_external_report_approval"));
    assert!(tool_names.contains(&"request_planning_approval"));
    assert!(tool_names.contains(&"record_external_report_dispatch"));
    assert!(!tool_names.contains(&"draft_backlog_items"));
    assert!(!tool_names.contains(&"draft_task_plan"));
    assert!(tool_names.contains(&"inspect_task_plan"));
    assert!(tool_names.contains(&"list_task_plans"));
    assert!(tool_names.contains(&"validate_task_plan"));
    assert!(tool_names.contains(&"write_task_plan"));
    assert!(tool_names.contains(&"prepare_work"));
    assert!(tool_names.contains(&"inspect_task"));
    assert!(tool_names.contains(&"claim_next_task"));
    assert!(tool_names.contains(&"worktree_create"));
    assert!(tool_names.contains(&"worktree_status"));
    assert!(tool_names.contains(&"worktree_diff"));
    assert!(tool_names.contains(&"inspect_worktree_changes"));
    assert!(tool_names.contains(&"worktree_cleanup"));
    assert!(tool_names.contains(&"integrate_worker_result"));
    assert!(tool_names.contains(&"inspect_integration_gates"));
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
    assert!(tool_names.contains(&"finish_work"));
    assert!(tool_names.contains(&"complete_backlog_item"));
    assert!(tool_names.contains(&"run_task_verification"));
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
    assert!(tool_names.contains(&"inspect_workflow_config"));
    assert!(tool_names.contains(&"send_worker_guidance"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_creates_and_lists_epics() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;
    let initialized = call_tool_json(
        &client,
        "init_project",
        json!({ "project_name": "Epic Smoke" }),
    )
    .await?;
    assert_stage_status("init_project", &initialized, "completed");

    let created = call_tool_json(
        &client,
        "create_epic",
        json!({
            "id": "webapp",
            "title": "Web Application",
            "priority": "P0",
            "description": "Web application work."
        }),
    )
    .await?;
    assert_stage_status("create_epic", &created, "completed");
    assert_eq!(created["data"]["epic"]["id"], "webapp");

    let listed = call_tool_json(&client, "list_epics", json!({})).await?;
    assert_stage_status("list_epics", &listed, "completed");
    assert_eq!(listed["data"]["returned"], 2);
    assert_eq!(listed["data"]["epics"][1]["id"], "webapp");

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_inspects_session_snapshot() -> anyhow::Result<()> {
    let project = assignment_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let session = call_tool_json(
        &client,
        "inspect_session",
        json!({ "limit": 5, "detail": "verbose", "require_task_plan": false }),
    )
    .await?;

    assert_stage_status("inspect_session", &session, "completed");
    assert_eq!(session["data"]["ok"], true);
    assert_eq!(session["data"]["recommended_tool"], "write_task_plan");
    assert!(session["data"]["doctor"]["ok"].as_bool().unwrap_or(false));
    assert_eq!(session["data"]["status"]["backlog_items"], 1);
    assert_eq!(
        session["data"]["queue"]["items"][0]["execution_path"],
        "blocked"
    );
    assert!(session["data"]["schemas_likely_needed_next"]
        .as_array()
        .expect("session schema hints")
        .iter()
        .any(|hint| hint["tool_name"] == "write_task_plan"
            && hint["usage"] == "required"
            && hint["host_neutral_query"] == "platypus tool write_task_plan"
            && hint["codex_tool_search_query"]
                == "mcp__platypus__write_task_plan platypus write_task_plan"
            && hint["claude_toolsearch_selector"] == "select:mcp__platypus__write_task_plan"));
    assert_eq!(
        session["data"]["claude_toolsearch_batch_selector"],
        "select:mcp__platypus__write_task_plan,mcp__platypus__validate_task_plan,mcp__platypus__inspect_item"
    );
    assert_eq!(
        session["data"]["host_neutral_tool_search_query"],
        "platypus tools write_task_plan validate_task_plan inspect_item"
    );
    assert!(session["data"]["workflow"]["integration"]["merge_style"].is_string());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_creates_backlog_items_atomically() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;
    let initialized = call_tool_json(
        &client,
        "init_project",
        json!({ "project_name": "Batch Smoke" }),
    )
    .await?;
    assert_stage_status("init_project", &initialized, "completed");

    let created = call_tool_json(
        &client,
        "create_backlog_items",
        json!({
            "id_prefix": "WEB",
            "preview": true,
            "items": [
                {
                    "client_key": "shape",
                    "title": "Shape web app",
                    "type": "feature",
                    "goal": "Shape the web app.",
                    "implementation_contract": "Define the initial web app structure.",
                    "acceptance": ["The web app shape is documented."]
                },
                {
                    "client_key": "implement",
                    "depends_on_keys": ["shape"],
                    "title": "Implement web app",
                    "type": "feature",
                    "goal": "Implement the web app.",
                    "implementation_contract": "Create the initial web app.",
                    "acceptance": ["The web app implementation validates."]
                }
            ]
        }),
    )
    .await?;
    assert_stage_status("create_backlog_items preview", &created, "completed");
    assert!(created["data"]["preview"].as_bool().unwrap_or(false));
    assert_eq!(created["data"]["created"], 0);
    assert_eq!(created["data"]["items"][0]["item_id"], "WEB-001");
    assert_eq!(created["data"]["items"][0]["created"], false);
    assert!(created["data"]["items"][0]["preview"]["markdown"]
        .as_str()
        .unwrap_or("")
        .contains("# WEB-001 Shape web app"));
    assert!(!project.path().join("backlog/items/WEB-001.md").exists());

    let created = call_tool_json(
        &client,
        "create_backlog_items",
        json!({
            "id_prefix": "WEB",
            "items": [
                {
                    "client_key": "shape",
                    "title": "Shape web app",
                    "type": "feature",
                    "goal": "Shape the web app.",
                    "implementation_contract": "Define the initial web app structure.",
                    "acceptance": ["The web app shape is documented."]
                },
                {
                    "client_key": "implement",
                    "depends_on_keys": ["shape"],
                    "title": "Implement web app",
                    "type": "feature",
                    "goal": "Implement the web app.",
                    "implementation_contract": "Create the initial web app.",
                    "acceptance": ["The web app implementation validates."]
                }
            ]
        }),
    )
    .await?;
    assert_stage_status("create_backlog_items", &created, "completed");
    assert_eq!(created["data"]["created"], 2);
    assert_eq!(created["data"]["preview"], false);
    assert_eq!(created["data"]["items"][0]["item_id"], "WEB-001");
    assert_eq!(created["data"]["items"][1]["item_id"], "WEB-002");
    assert_eq!(created["data"]["items"][1]["depends_on"][0], "WEB-001");

    let failed = call_tool_json(
        &client,
        "create_backlog_items",
        json!({
            "items": [
                {
                    "client_key": "ok",
                    "title": "Would be valid",
                    "goal": "Would be valid."
                },
                {
                    "client_key": "bad",
                    "title": "Bad epic",
                    "epic": "missing",
                    "goal": "Bad epic."
                }
            ]
        }),
    )
    .await?;
    assert_stage_status("create_backlog_items", &failed, "failed");
    assert!(!project.path().join("backlog/items/PROJ-001.md").exists());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_quick_creates_backlog_item() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;
    let initialized = call_tool_json(
        &client,
        "init_project",
        json!({ "project_name": "Quick Create Smoke" }),
    )
    .await?;
    assert_stage_status("init_project", &initialized, "completed");

    let created = call_tool_json(
        &client,
        "quick_create_backlog_item",
        json!({
            "id": "PROJ-001",
            "title": "Document quick path",
            "goal": "Document the quick backlog path.",
            "priority": "P2",
            "type": "docs",
            "area": "docs",
            "owned_surfaces": ["docs/tools.md"],
            "acceptance": ["The quick path is documented."]
        }),
    )
    .await?;
    assert_stage_status("quick_create_backlog_item", &created, "completed");
    assert_eq!(created["data"]["item_id"], "PROJ-001");
    let text = fs::read_to_string(project.path().join("backlog/items/PROJ-001.md"))?;
    assert!(text.contains("priority: P2"));
    assert!(text.contains("type: docs"));
    assert!(text.contains("- docs/tools.md"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_updates_backlog_item_with_validation() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;
    let initialized = call_tool_json(
        &client,
        "init_project",
        json!({ "project_name": "Update Smoke" }),
    )
    .await?;
    assert_stage_status("init_project", &initialized, "completed");

    let created = call_tool_json(
        &client,
        "create_backlog_item",
        json!({
            "id": "PROJ-001",
            "title": "Original item",
            "type": "feature",
            "goal": "Original goal.",
            "implementation_contract": "Original contract.",
            "acceptance": ["Original acceptance."]
        }),
    )
    .await?;
    assert_stage_status("create_backlog_item", &created, "completed");

    let updated = call_tool_json(
        &client,
        "update_backlog_item",
        json!({
            "item_id": "PROJ-001",
            "title": "Updated item",
            "priority": "P0",
            "type": "docs",
            "owned_surfaces": ["README.md"],
            "execution_path": "worker_handoff",
            "planning_gate": "task_plan",
            "goal": "Updated goal.",
            "implementation_contract": "Updated contract.",
            "acceptance": ["Updated acceptance."]
        }),
    )
    .await?;
    assert_stage_status("update_backlog_item", &updated, "completed");
    assert_eq!(updated["data"]["item_id"], "PROJ-001");
    assert!(updated["data"]["changed_fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|field| field == "title"));

    let text = fs::read_to_string(project.path().join("backlog/items/PROJ-001.md"))?;
    assert!(text.contains("title: Updated item"));
    assert!(text.contains("priority: P0"));
    assert!(text.contains("type: docs"));
    assert!(text.contains("execution_path: worker_handoff"));
    assert!(text.contains("- Updated acceptance."));

    let before = text;
    let failed = call_tool_json(
        &client,
        "update_backlog_item",
        json!({
            "item_id": "PROJ-001",
            "depends_on": ["PROJ-999"]
        }),
    )
    .await?;
    assert_stage_status("update_backlog_item", &failed, "failed");
    assert!(failed["error"]
        .as_str()
        .unwrap_or("")
        .contains("unknown dependency `PROJ-999`"));
    assert_eq!(
        fs::read_to_string(project.path().join("backlog/items/PROJ-001.md"))?,
        before
    );

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_inspects_toolsets() -> anyhow::Result<()> {
    let client = start_client(None).await?;

    let all = call_tool_json(&client, "inspect_toolsets", json!({})).await?;
    assert_stage_status("inspect_toolsets all", &all, "completed");
    assert_eq!(all["data"]["total"], 6);
    assert_eq!(all["data"]["returned"], 6);
    let toolsets = all["data"]["toolsets"].as_array().expect("toolsets");
    assert!(toolsets.iter().any(|toolset| toolset["name"] == "startup"
        && toolset["title"] == "Startup Inspection"
        && toolset["recommended_first_tool"] == "inspect_session"
        && toolset["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .any(|tool| tool == "inspect_toolsets")
        && toolset["claude_selector"]
            .as_str()
            .unwrap_or("")
            .contains("mcp__platypus__inspect_session")
        && toolset["codex_query"]
            .as_str()
            .unwrap_or("")
            .contains("inspect_session")));

    let filtered = call_tool_json(
        &client,
        "inspect_toolsets",
        json!({ "toolset": "direct-execution" }),
    )
    .await?;
    assert_stage_status("inspect_toolsets filtered", &filtered, "completed");
    assert_eq!(filtered["data"]["returned"], 1);
    assert_eq!(filtered["data"]["toolsets"][0]["name"], "direct_execution");
    assert_eq!(
        filtered["data"]["toolsets"][0]["recommended_first_tool"],
        "inspect_work_queue"
    );

    let unknown =
        call_tool_json(&client, "inspect_toolsets", json!({ "toolset": "missing" })).await?;
    assert_stage_status("inspect_toolsets unknown", &unknown, "failed");
    assert!(unknown["next_action"]
        .as_str()
        .unwrap_or("")
        .contains("startup"));

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
async fn stdio_server_returns_contextual_validation_next_actions() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;
    call_tool_json(
        &client,
        "init_project",
        json!({ "project_name": "Validation Next Actions" }),
    )
    .await?;

    call_tool_json(
        &client,
        "create_backlog_item",
        json!({
            "id": "PROJ-001",
            "title": "Direct item",
            "goal": "Exercise direct validation guidance.",
            "implementation_contract": "Keep direct work in the manager workspace.",
            "acceptance": ["Direct guidance is clear."]
        }),
    )
    .await?;
    let direct = call_tool_json(&client, "validate_backlog", json!({})).await?;
    assert_stage_status("validate_backlog direct", &direct, "completed");
    let direct_next = direct["next_action"].as_str().expect("direct next");
    assert!(direct_next.contains("prepare_work"));
    assert!(direct_next.contains("complete_backlog_item"));
    assert!(!direct_next.contains("Commit backlog artifacts before dispatching"));

    call_tool_json(
        &client,
        "create_backlog_item",
        json!({
            "id": "PROJ-002",
            "title": "Worker item",
            "goal": "Exercise worker validation guidance.",
            "implementation_contract": "Keep worker work in a handoff worktree.",
            "acceptance": ["Worker guidance is clear."],
            "execution_path": "worker_handoff",
            "planning_gate": "task_plan"
        }),
    )
    .await?;
    let blocked_worker = call_tool_json(&client, "validate_backlog", json!({})).await?;
    assert_stage_status(
        "validate_backlog blocked worker",
        &blocked_worker,
        "completed",
    );
    let blocked_worker_next = blocked_worker["next_action"]
        .as_str()
        .expect("blocked worker next");
    assert!(blocked_worker_next.contains("need valid task plans"));
    assert!(blocked_worker_next.contains("Direct-ready"));
    assert!(!blocked_worker_next.contains("commit_planning_artifacts"));

    call_tool_json(
        &client,
        "write_task_plan",
        json!({
            "item_id": "PROJ-002",
            "plan": {
                "item_id": "PROJ-002",
                "version": 1,
                "mode": "standard",
                "requirements": [
                    { "id": "R1", "text": "Deliver the worker backlog item." }
                ],
                "design": {
                    "summary": "Implement a focused worker fixture slice.",
                    "owned_surfaces": ["src/lib.rs"],
                    "notes": null
                },
                "tasks": [
                    {
                        "id": "PROJ-002-T001",
                        "title": "Implement worker fixture",
                        "goal": "Complete the worker fixture behavior.",
                        "requirement_refs": ["R1"],
                        "depends_on": [],
                        "owned_surfaces": ["src/lib.rs"],
                        "verification": ["make check"],
                        "acceptance": ["Worker backlog item acceptance is satisfied."],
                        "notes": null
                    }
                ]
            }
        }),
    )
    .await?;
    let mixed = call_tool_json(&client, "validate_backlog", json!({})).await?;
    assert_stage_status("validate_backlog mixed", &mixed, "completed");
    let mixed_next = mixed["next_action"].as_str().expect("mixed next");
    assert!(mixed_next.contains("Direct-ready"));
    assert!(mixed_next.contains("Worker-handoff"));
    assert!(mixed_next.contains("commit_planning_artifacts"));

    client.cancel().await?;
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
    assert!(resource_uris.contains(&"platypus://guidance/tool-preload"));
    assert!(resource_uris.contains(&"platypus://tools/core-schemas"));
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

    assert!(text.contains("inspect_work_queue"));
    assert!(text.contains("Exact Decision Table"));
    assert!(text.contains("The first matching state wins"));
    assert!(text.contains("queue_state == \"direct_ready\""));
    assert!(text.contains("completed_pending_integration"));
    assert!(text.contains("approval_blocked"));
    assert!(text.contains("config_blocked"));
    assert!(text.contains("workspace_blocked"));
    assert!(text.contains("direct_guidance"));
    assert!(text.contains("worktree_prepared"));
    assert!(text.contains("not_prepared"));
    assert!(text.contains("direct_edit"));
    assert!(text.contains("run_in_worktree"));
    assert!(text.contains("verify_or_record_risk"));
    assert!(text.contains("resolve_findings"));
    assert!(text.contains("prepare_work"));
    assert!(text.contains("integrate_worker_result"));
    assert!(text.contains("reconcile_project"));
    assert!(text.contains("verification evidence"));
    assert!(text.contains("may replace separate startup calls"));
    assert!(text.contains("durable_next_tool=complete_backlog_item"));
    assert!(text.contains("reconcile_project is optional"));

    let recovery = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "platypus://guidance/recovery".to_string(),
        })
        .await?;
    let text = resource_text(&recovery.contents[0]);
    assert!(text.contains("Concrete recovery paths"));
    assert!(text.contains("orphaned task evidence"));
    assert!(text.contains("inspect_integration_gates"));
    assert!(text.contains("update_finding_disposition"));
    assert!(text.contains("Successful direct completion"));

    let spec = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "platypus://guidance/spec-driven-development".to_string(),
        })
        .await?;
    let text = resource_text(&spec.contents[0]);
    assert!(text.contains("coding host"));
    assert!(text.contains("create_backlog_items"));
    assert!(text.contains("write_task_plan"));
    assert!(text.contains("Minimum viable direct-edit loop"));
    assert!(text.contains("prepare_work_optional"));
    assert!(text.contains("traceability tradeoff"));
    assert!(text.contains("optional audit/recovery"));

    let preload = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "platypus://guidance/tool-preload".to_string(),
        })
        .await?;
    let text = resource_text(&preload.contents[0]);
    assert!(text.contains("Startup Inspection Group"));
    assert!(text.contains("Backlog Planning Group"));
    assert!(text.contains("Direct Execution Group"));
    assert!(text.contains("Worker Handoff Group"));
    assert!(text.contains("Evidence And Findings Group"));
    assert!(text.contains("Recovery Group"));
    assert!(text.contains("Preloading is optional and host-specific"));
    assert!(text.contains("if a host cannot preload schemas"));
    assert!(text.contains("select:mcp__platypus__inspect_session"));
    assert!(text.contains("Direct Execution"));
    assert!(text.contains("quick_create_backlog_item"));
    assert!(!text.contains("draft_task_plan"));
    assert!(text.contains("Tool Naming Map"));
    assert!(text.contains("host-specific"));
    assert!(text.contains("create_backlog_items"));
    assert!(text.contains("dispatch_ready_work"));
    assert!(text.contains("complete_backlog_item"));
    assert!(text.contains("update_finding_disposition"));

    let core = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "platypus://tools/core-schemas".to_string(),
        })
        .await?;
    let text = resource_text(&core.contents[0]);
    assert!(text.contains("Platypus Core Schema Preload"));
    assert!(text.contains("mcp__platypus__inspect_session"));
    assert!(text.contains("create_backlog_items"));
    assert!(text.contains("schema source of truth"));

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
    assert!(prompt_names.contains(&"platypus-tool-preload"));
    assert!(prompt_names.contains(&"platypus-core-schemas"));
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
    assert!(text.contains("file_summary"));
    assert!(text.contains("manager_disposition"));
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

    let prompt = client
        .get_prompt(GetPromptRequestParams {
            meta: None,
            name: "platypus-recovery".to_string(),
            arguments: None,
        })
        .await?;
    let text = prompt_text(&prompt.messages[0]);
    assert!(text.contains("Concrete recovery paths"));
    assert!(text.contains("Platypus-Verification"));
    assert!(text.contains("record_verification_evidence"));

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
async fn stdio_server_writes_and_validates_task_plan() -> anyhow::Result<()> {
    let project = assignment_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let written = call_tool_json(
        &client,
        "write_task_plan",
        json!({
            "item_id": "PROJ-001",
            "plan": task_plan_json()
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

    let missing = call_tool_json(&client, "inspect_work_queue", json!({ "limit": 5 })).await?;
    assert_stage_status("inspect_work_queue missing plan", &missing, "completed");
    assert_eq!(missing["data"]["recommended_tool"], "write_task_plan");
    assert_eq!(
        missing["data"]["items"][0]["candidate"]["item_id"],
        "PROJ-001"
    );
    assert_eq!(missing["data"]["items"][0]["plan"]["status"], "missing");
    assert_eq!(missing["data"]["items"][0]["ready_to_dispatch"], false);

    let written = call_tool_json(
        &client,
        "write_task_plan",
        json!({
            "item_id": "PROJ-001",
            "plan": task_plan_json()
        }),
    )
    .await?;
    assert_stage_status("write_task_plan", &written, "completed");

    let ready = call_tool_json(&client, "inspect_work_queue", json!({ "limit": 5 })).await?;
    assert_stage_status("inspect_work_queue ready", &ready, "completed");
    assert_eq!(ready["data"]["recommended_tool"], "dispatch_ready_work");
    assert_eq!(ready["data"]["items"][0]["plan"]["status"], "valid");
    assert_eq!(
        ready["data"]["items"][0]["planning"]["required_mode"],
        "task_plan"
    );
    assert_eq!(ready["data"]["items"][0]["plan"]["task_count"], 1);
    assert_eq!(ready["data"]["items"][0]["ready_to_dispatch"], true);

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

    let item = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_item".into(),
            arguments: Some(json_args(json!({ "item_id": "PROJ-003" }))),
            task: None,
        })
        .await?;
    let item = item.structured_content.expect("item content");
    assert_eq!(item["action"], "inspect_item");
    assert_eq!(item["status"], "completed");
    assert_eq!(item["data"]["queue_state"], "dependency_blocked");
    assert_eq!(item["data"]["item"]["open_dependencies"][0], "PROJ-002");
    assert_eq!(item["data"]["recommended_tool"], "inspect_item");

    let compact = call_tool_json(
        &client,
        "get_backlog_item",
        json!({ "item_id": "PROJ-003", "max_markdown_bytes": 80 }),
    )
    .await?;
    assert_stage_status("get_backlog_item", &compact, "completed");
    assert_eq!(compact["data"]["item_id"], "PROJ-003");
    assert_eq!(compact["data"]["title"], "Third item");
    assert!(compact["data"]["markdown"]
        .as_str()
        .unwrap_or("")
        .contains("PROJ-003"));
    assert!(compact["data"]["markdown_bytes"].as_u64().unwrap_or(0) >= 80);
    assert_eq!(compact["data"]["markdown_truncated"], true);

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_inspects_compact_queue_status() -> anyhow::Result<()> {
    let project = TempDir::new().expect("temp dir");
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
    git(project.path(), &["add", "--all"]);
    git(project.path(), &["commit", "-m", "Add queue items"]);
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let response = call_tool_json(&client, "inspect_queue_status", json!({ "limit": 5 })).await?;

    assert_eq!(response["action"], "inspect_queue_status");
    assert_eq!(response["status"], "completed");
    assert_eq!(response["data"]["counts"]["total_count"], 2);
    assert_eq!(response["data"]["counts"]["runnable_count"], 1);
    assert_eq!(response["data"]["counts"]["dependency_blocked_count"], 1);
    assert_eq!(
        response["data"]["top_ready_items"][0]["item_id"],
        "PROJ-001"
    );
    assert_eq!(
        response["data"]["top_blocked_items"][0]["queue_state"],
        "dependency_blocked"
    );
    assert!(response["data"]["state_descriptions"]
        .as_array()
        .expect("states")
        .iter()
        .any(|state| state["queue_state"] == "direct_ready"));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_inspects_dependency_graph() -> anyhow::Result<()> {
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

    let response = call_tool_json(
        &client,
        "inspect_dependency_graph",
        json!({
            "focus_item_id": "PROJ-002",
            "include_closed": true,
            "limit": 10
        }),
    )
    .await?;

    assert_stage_status("inspect_dependency_graph", &response, "completed");
    assert_eq!(response["data"]["focus_item_id"], "PROJ-002");
    assert_eq!(response["data"]["total"], 3);
    assert_eq!(
        response["data"]["edges"].as_array().expect("edges").len(),
        2
    );
    assert_eq!(response["data"]["closed_nodes"][0], "PROJ-001");
    assert_eq!(response["data"]["runnable_nodes"][0], "PROJ-002");
    assert_eq!(
        response["data"]["topological_order"],
        json!(["PROJ-001", "PROJ-002", "PROJ-003"])
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
    assert!(project.path().join("AGENTS.md").is_file());
    assert!(project.path().join("CLAUDE.md").is_file());
    assert!(project.path().join("backlog/epics/general.md").is_file());
    let claude = fs::read_to_string(project.path().join("CLAUDE.md"))?;
    assert!(claude.contains("spec-driven development"));
    assert!(claude.contains("write_task_plan"));

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
async fn stdio_server_requests_planning_approval_before_dispatch() -> anyhow::Result<()> {
    let project = dispatch_project_fixture();
    mark_backlog_item_policy(
        project.path(),
        "PROJ-001",
        "worker_handoff",
        "approved_task_plan",
    )?;
    write_task_plan_yaml(project.path(), "PROJ-001")?;
    git(
        project.path(),
        &[
            "add",
            "backlog/items/PROJ-001.md",
            "backlog/plans/PROJ-001.yaml",
        ],
    );
    git(
        project.path(),
        &["commit", "-m", "Require planning approval"],
    );
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let blocked = call_tool_json(
        &client,
        "dispatch_ready_work",
        json!({
            "item_id": "PROJ-001",
            "max_tasks": 1,
            "worker": "coder",
            "prepare_handoffs": false
        }),
    )
    .await?;
    assert_stage_status("dispatch_ready_work planning blocked", &blocked, "failed");
    assert_eq!(blocked["data"]["stopped_reason"], "policy_blocked");

    let approval = call_tool_json(
        &client,
        "request_planning_approval",
        json!({
            "item_ids": ["PROJ-001"],
            "requested_by": "manager",
            "summary": "Approve the first implementation slice before dispatch."
        }),
    )
    .await?;
    assert_stage_status("request_planning_approval", &approval, "completed");
    let approval_id = string_at(&approval, &["data", "approval", "id"], "approval id");
    let approved = call_tool_json(
        &client,
        "approval_respond",
        json!({
            "approval_id": approval_id,
            "decision": "approve",
            "responder": "user"
        }),
    )
    .await?;
    assert_stage_status("approval_respond planning", &approved, "completed");

    let dispatched = call_tool_json(
        &client,
        "dispatch_ready_work",
        json!({
            "item_id": "PROJ-001",
            "max_tasks": 1,
            "worker": "coder",
            "prepare_handoffs": false
        }),
    )
    .await?;
    assert_stage_status(
        "dispatch_ready_work planning approved",
        &dispatched,
        "completed",
    );
    assert_eq!(dispatched["data"]["dispatched"], 1);

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_dispatches_manual_handoff_without_local_worker_config() -> anyhow::Result<()>
{
    let project = dispatch_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let result = call_tool_json(
        &client,
        "dispatch_ready_work",
        json!({
            "max_tasks": 1,
            "worker": "coder",
            "claimant": "stdio-manual",
            "execution_mode": "manual_handoff",
            "verification_command": ["make", "check"]
        }),
    )
    .await?;

    assert_stage_status("dispatch_ready_work", &result, "completed");
    assert_eq!(result["data"]["execution_mode"], "manual_handoff");
    let item = &result["data"]["items"][0];
    assert_eq!(item["status"], "prepared");
    assert_eq!(item["assignment"]["execution_mode"], "manual_handoff");
    assert_eq!(
        item["assignment"]["bundle"]["execution_mode"],
        "manual_handoff"
    );

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

    let guidance = call_tool_json(&client, "inspect_work_queue", json!({})).await?;
    assert_stage_status("inspect_work_queue", &guidance, "completed");
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
async fn stdio_server_prepare_worker_handoff_persists_worker_assignment() -> anyhow::Result<()> {
    let project = assignment_project_fixture();
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let dispatched = call_tool_json(&client, "dispatch_next_work", json!({})).await?;
    assert_stage_status("dispatch_next_work", &dispatched, "completed");
    let task_id = string_at(&dispatched, &["data", "task", "id"], "task id");

    let prepared = call_tool_json(
        &client,
        "prepare_worker_handoff",
        json!({
            "task_id": task_id,
            "worker": "coder",
            "claimant": "runner-stdio",
            "verification_command": ["make", "check"]
        }),
    )
    .await?;
    assert_stage_status("prepare_worker_handoff", &prepared, "completed");
    let assignment_id = string_at(&prepared, &["data", "assignment", "id"], "assignment id");

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
            name: "inspect_work_queue".into(),
            arguments: Some(JsonObject::new()),
            task: None,
        })
        .await?;
    let initial = initial.structured_content.expect("initial guidance");
    assert_eq!(initial["data"]["recommended_tool"], "write_task_plan");

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
            name: "inspect_work_queue".into(),
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
            name: "inspect_work_queue".into(),
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
            name: "inspect_work_queue".into(),
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

    let completed_without_verification = client
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
    let completed_without_verification = completed_without_verification
        .structured_content
        .expect("completed content");
    assert_eq!(completed_without_verification["status"], "completed");
    assert_eq!(
        completed_without_verification["action"],
        "complete_worker_task"
    );
    assert_eq!(
        completed_without_verification["data"]["assignment"]["verification_status"],
        "not_run"
    );

    assert_eq!(
        completed_without_verification["data"]["assignment"]["status"],
        "completed"
    );
    assert!(completed_without_verification["next_action"]
        .as_str()
        .expect("next action")
        .contains("run_task_verification"));

    let integration_guidance = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "inspect_work_queue".into(),
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
    assert_eq!(
        integration_guidance["data"]["params"]["allow_unverified"],
        true
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
            name: "inspect_work_queue".into(),
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
            "owned_surfaces": ["README.md"],
            "execution_path": "worker_handoff",
            "planning_gate": "none",
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

    let initial = call_tool_json(&client, "inspect_work_queue", json!({})).await?;
    assert_eq!(
        initial["data"]["recommended_tool"], "dispatch_ready_work",
        "stage inspect_work_queue before dispatch: {initial:#}"
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

    let integration_guidance = call_tool_json(&client, "inspect_work_queue", json!({})).await?;
    assert_eq!(
        integration_guidance["data"]["recommended_tool"], "integrate_worker_result",
        "stage inspect_work_queue before integration: {integration_guidance:#}"
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
async fn stdio_server_prepares_and_finishes_host_run_work() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let initialized = call_tool_json(
        &client,
        "init_project",
        json!({ "project_name": "Host Lifecycle Smoke" }),
    )
    .await?;
    assert_stage_status("init_project", &initialized, "completed");

    git(project.path(), &["init"]);
    git(project.path(), &["config", "user.name", "Platypus Test"]);
    git(
        project.path(),
        &["config", "user.email", "platypus@example.invalid"],
    );
    fs::create_dir_all(project.path().join("src"))?;
    fs::write(
        project.path().join("src/lib.rs"),
        "pub fn answer() -> u8 { 0 }\n",
    )?;

    let created = call_tool_json(
        &client,
        "create_backlog_item",
        json!({
            "id": "PROJ-001",
            "title": "Implement workflow feature",
            "priority": "P1",
            "type": "feature",
            "area": "workflow",
            "epic": "general",
            "owned_surfaces": ["src/lib.rs"],
            "execution_path": "worker_handoff",
            "planning_gate": "task_plan",
            "goal": "Implement one small workflow feature.",
            "implementation_contract": "Only edit src/lib.rs in the assignment worktree.",
            "acceptance": ["src/lib.rs exposes the new answer."]
        }),
    )
    .await?;
    assert_stage_status("create_backlog_item", &created, "completed");
    let plan = call_tool_json(
        &client,
        "write_task_plan",
        json!({
            "item_id": "PROJ-001",
            "plan": task_plan_json()
        }),
    )
    .await?;
    assert_stage_status("write_task_plan", &plan, "completed");

    git(project.path(), &["add", "--all"]);
    git(
        project.path(),
        &["commit", "-m", "Initialize host lifecycle project"],
    );

    let prepared = call_tool_json(
        &client,
        "prepare_work",
        json!({
            "item_id": "PROJ-001",
            "worker": "coder",
            "claimant": "stdio-host",
            "require_task_plan": true
        }),
    )
    .await?;
    assert_stage_status("prepare_work", &prepared, "completed");
    assert_eq!(
        prepared["data"]["host_actions"][0]["kind"],
        "run_in_worktree"
    );
    let host_action = &prepared["data"]["host_actions"][0];
    let assignment_id = host_action["assignment_id"]
        .as_str()
        .expect("assignment id")
        .to_string();
    let task_id = host_action["task_id"]
        .as_str()
        .expect("task id")
        .to_string();
    let worktree_path = PathBuf::from(
        host_action["worktree_path"]
            .as_str()
            .expect("worktree path"),
    );

    fs::write(
        worktree_path.join("src/lib.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )?;

    let finished = call_tool_json(
        &client,
        "finish_work",
        json!({
            "assignment_id": assignment_id,
            "status": "completed",
            "summary": "Implemented the workflow feature.",
            "verification_status": "passed",
            "verification_summary": "Reviewed src/lib.rs after worker edit.",
            "verification_refs": ["manual:stdio-smoke"],
            "findings_reviewed": true,
            "integrate_if_ready": false
        }),
    )
    .await?;
    assert_stage_status("finish_work", &finished, "completed");
    assert_eq!(finished["data"]["host_action"]["kind"], "integrate_result");
    assert_eq!(
        finished["data"]["assignment"]["changed_files"][0],
        "src/lib.rs"
    );
    assert_eq!(
        finished["data"]["evidence"]
            .as_array()
            .expect("evidence")
            .len(),
        1
    );

    let evidence = call_tool_json(
        &client,
        "list_evidence",
        json!({
            "source_item_id": "PROJ-001",
            "source_task_id": task_id,
            "kind": "verification"
        }),
    )
    .await?;
    assert_stage_status("list_evidence", &evidence, "completed");
    assert_eq!(evidence["data"]["returned"], 1);

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_server_completes_direct_backlog_item() -> anyhow::Result<()> {
    let project = TempDir::new()?;
    let client = start_client(Some(project.path().to_string_lossy().as_ref())).await?;

    let initialized = call_tool_json(
        &client,
        "init_project",
        json!({ "project_name": "Direct Lifecycle Smoke" }),
    )
    .await?;
    assert_stage_status("init_project", &initialized, "completed");

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
            "title": "Write direct feedback file",
            "priority": "P1",
            "type": "docs",
            "area": "feedback",
            "epic": "general",
            "owned_surfaces": ["FEEDBACK.md"],
            "goal": "Create one direct feedback file.",
            "implementation_contract": "Only edit FEEDBACK.md.",
            "acceptance": ["FEEDBACK.md exists."]
        }),
    )
    .await?;
    assert_stage_status("create_backlog_item", &created, "completed");
    git(project.path(), &["add", "--all"]);
    git(project.path(), &["commit", "-m", "Initialize direct work"]);

    let direct_queue = call_tool_json(&client, "inspect_work_queue", json!({})).await?;
    assert_stage_status("inspect_work_queue direct", &direct_queue, "completed");
    assert_eq!(
        direct_queue["data"]["items"][0]["queue_state"],
        "direct_ready"
    );
    assert_eq!(
        direct_queue["data"]["items"][0]["prepare_work_optional"],
        true
    );
    assert_eq!(
        direct_queue["data"]["recommended_tool"],
        "complete_backlog_item"
    );
    assert_eq!(
        direct_queue["data"]["minimal_direct_loop"]["primary_next_tool"],
        "complete_backlog_item"
    );
    assert_eq!(
        direct_queue["data"]["minimal_direct_loop"]["optional_guidance_tool"],
        "prepare_work"
    );
    assert!(direct_queue["data"]["schemas_likely_needed_next"]
        .as_array()
        .expect("schema hints")
        .iter()
        .any(|hint| hint["tool_name"] == "complete_backlog_item"
            && hint["usage"] == "required"
            && hint["host_neutral_query"] == "platypus tool complete_backlog_item"
            && hint["codex_tool_search_query"]
                == "mcp__platypus__complete_backlog_item platypus complete_backlog_item"
            && hint["claude_toolsearch_selector"]
                == "select:mcp__platypus__complete_backlog_item"));
    assert!(direct_queue["data"]["schemas_likely_needed_next"]
        .as_array()
        .expect("schema hints")
        .iter()
        .any(|hint| hint["tool_name"] == "record_verification_evidence"
            && hint["usage"] == "optional"));
    assert_eq!(
        direct_queue["data"]["claude_toolsearch_batch_selector"],
        "select:mcp__platypus__complete_backlog_item,mcp__platypus__record_verification_evidence,mcp__platypus__prepare_work"
    );
    assert_eq!(
        direct_queue["data"]["host_neutral_tool_search_query"],
        "platypus tools complete_backlog_item record_verification_evidence prepare_work"
    );

    let prepared = call_tool_json(
        &client,
        "prepare_work",
        json!({ "item_id": "PROJ-001", "require_task_plan": false }),
    )
    .await?;
    assert_stage_status("prepare_work", &prepared, "completed");
    assert_eq!(prepared["data"]["prepared_state"], "direct_guidance");
    assert_eq!(prepared["data"]["state_persisted"], false);
    assert_eq!(prepared["data"]["persistence"], "response_only");
    assert_eq!(
        prepared["data"]["durable_next_tool"],
        "complete_backlog_item"
    );
    assert!(prepared["data"]["persistence_summary"]
        .as_str()
        .expect("persistence summary")
        .contains("No task"));
    assert_eq!(prepared["data"]["host_actions"][0]["kind"], "direct_edit");
    assert!(prepared["data"]["host_actions"][0]["next_tools"]
        .as_array()
        .expect("next tools")
        .iter()
        .any(|tool| tool == "complete_backlog_item"));

    fs::write(project.path().join("FEEDBACK.md"), "# Feedback\n")?;
    let completed = call_tool_json(
        &client,
        "complete_backlog_item",
        json!({
            "item_id": "PROJ-001",
            "summary": "Created direct feedback file.",
            "changed_files": ["FEEDBACK.md"],
            "verification_status": "skipped",
            "verification_summary": "Manual smoke covered the file.",
            "verification_refs": ["manual:stdio-direct"]
        }),
    )
    .await?;
    assert_stage_status("complete_backlog_item", &completed, "completed");
    assert_eq!(completed["data"]["closed"], true);
    assert_eq!(completed["data"]["closure"]["source"], "runtime_event");
    assert_eq!(
        completed["data"]["closure"]["runtime_completion_recorded"],
        true
    );
    assert_eq!(completed["data"]["closure"]["git_trailer_portable"], false);
    assert_eq!(
        completed["data"]["commit_outcome"]["status"],
        "not_requested"
    );
    assert_eq!(completed["data"]["commit_outcome"]["requested"], false);
    assert_eq!(completed["data"]["detail"], "compact");
    assert_eq!(completed["data"]["queue_status"], serde_json::Value::Null);
    assert_eq!(
        completed["data"]["queue_status_error"],
        serde_json::Value::Null
    );
    assert_eq!(completed["data"]["compact"]["item_id"], "PROJ-001");
    assert!(completed["data"]["compact"]["status_line"]
        .as_str()
        .expect("status line")
        .contains("PROJ-001 closed"));
    assert_eq!(completed["data"]["compact"]["closed"], true);
    assert_eq!(
        completed["data"]["compact"]["closure_source"],
        "runtime_event"
    );
    assert_eq!(completed["data"]["compact"]["queue_state"], "closed");
    assert_eq!(
        completed["data"]["compact"]["recommended_tool"],
        "create_backlog_items"
    );
    assert_eq!(completed["data"]["auto_evidence_enabled"], true);
    assert_eq!(
        completed["data"]["generated_evidence"]
            .as_array()
            .expect("generated evidence")
            .len(),
        2
    );
    assert_eq!(completed["data"]["generated_evidence"][0]["kind"], "note");
    assert_eq!(
        completed["data"]["generated_evidence"][1]["kind"],
        "verification"
    );

    let queue = call_tool_json(
        &client,
        "inspect_work_queue",
        json!({ "require_task_plan": false }),
    )
    .await?;
    assert_stage_status("inspect_work_queue", &queue, "completed");
    assert_eq!(queue["data"]["items"].as_array().expect("items").len(), 0);
    assert_eq!(queue["data"]["inventory"]["total_count"], 1);
    assert_eq!(queue["data"]["inventory"]["closed_count"], 1);

    let explicit = call_tool_json(
        &client,
        "record_evidence",
        json!({
            "source_item_id": "PROJ-002",
            "kind": "note",
            "summary": "Explicit evidence covers the second direct edit.",
            "refs": ["file:SECOND.md"]
        }),
    )
    .await?;
    assert_stage_status("record_evidence explicit", &explicit, "completed");
    let explicit_id = explicit["data"]["evidence"]["id"]
        .as_str()
        .expect("evidence id")
        .to_string();

    let second = call_tool_json(
        &client,
        "create_backlog_item",
        json!({
            "id": "PROJ-002",
            "title": "Write second direct file",
            "priority": "P1",
            "type": "docs",
            "area": "feedback",
            "epic": "general",
            "owned_surfaces": ["SECOND.md"],
            "goal": "Create one second direct file.",
            "implementation_contract": "Only edit SECOND.md.",
            "acceptance": ["SECOND.md exists."]
        }),
    )
    .await?;
    assert_stage_status("create_backlog_item second", &second, "completed");

    fs::write(project.path().join("SECOND.md"), "# Second\n")?;
    let completed = call_tool_json(
        &client,
        "complete_backlog_item",
        json!({
            "item_id": "PROJ-002",
            "summary": "Created second direct feedback file.",
            "changed_files": ["SECOND.md"],
            "verification_status": "skipped",
            "verification_summary": "Explicit evidence covers verification.",
            "verification_refs": ["manual:stdio-direct-second"],
            "evidence_refs": [explicit_id],
            "record_auto_evidence": false
        }),
    )
    .await?;
    assert_stage_status(
        "complete_backlog_item no auto evidence",
        &completed,
        "completed",
    );
    assert_eq!(completed["data"]["auto_evidence_enabled"], false);
    assert_eq!(
        completed["data"]["generated_evidence"]
            .as_array()
            .expect("generated evidence")
            .len(),
        0
    );
    assert_eq!(completed["data"]["evidence"].as_array().unwrap().len(), 0);
    assert_eq!(
        completed["data"]["event"]["payload"]["generated_evidence_refs"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

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

fn task_plan_json() -> Value {
    json!({
        "item_id": "PROJ-001",
        "version": 1,
        "mode": "standard",
        "requirements": [
            { "id": "R1", "text": "Deliver the backlog item." }
        ],
        "design": {
            "summary": "Implement a focused assignment fixture slice.",
            "owned_surfaces": ["src/lib.rs"],
            "notes": null
        },
        "tasks": [
            {
                "id": "PROJ-001-T001",
                "title": "Implement assignment fixture",
                "goal": "Complete the assignment fixture behavior.",
                "requirement_refs": ["R1"],
                "depends_on": [],
                "owned_surfaces": ["src/lib.rs"],
                "verification": ["make check"],
                "acceptance": ["Backlog item acceptance is satisfied."],
                "notes": null
            }
        ]
    })
}

async fn call_tool_json(
    client: &rmcp::service::RunningService<rmcp::RoleClient, impl ClientHandler>,
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
    fs::write(temp.path().join("platy.yaml"), ready_config_yaml()).expect("config");
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
    fs::write(
        temp.path().join("platy.yaml"),
        "project: test\nworkflow:\n  execution:\n    default_path: worker_handoff\n    worker_planning_gate: none\n",
    )
    .expect("config");
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
    fs::write(temp.path().join("platy.yaml"), ready_config_yaml()).expect("config");
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
owned_surfaces:
- README.md
execution_path: worker_handoff
planning_gate: task_plan
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

fn ready_config_yaml() -> &'static str {
    "project: test\n"
}

fn mark_backlog_item_policy(
    root: &Path,
    item_id: &str,
    execution_path: &str,
    planning_gate: &str,
) -> anyhow::Result<()> {
    let path = root.join(format!("backlog/items/{item_id}.md"));
    let text = fs::read_to_string(&path)?;
    let text = text.replace(
        "\n---\n\n#",
        &format!("\nexecution_path: {execution_path}\nplanning_gate: {planning_gate}\n---\n\n#"),
    );
    fs::write(path, text)?;
    Ok(())
}

fn write_task_plan_yaml(root: &Path, item_id: &str) -> anyhow::Result<()> {
    fs::create_dir_all(root.join("backlog/plans"))?;
    fs::write(
        root.join(format!("backlog/plans/{item_id}.yaml")),
        format!(
            r#"item_id: {item_id}
version: 1
mode: standard
requirements:
  - id: R1
    text: Do the work.
design:
  summary: Focused implementation.
  owned_surfaces:
    - README.md
  notes: null
tasks:
  - id: {item_id}-T001
    title: Implement item
    goal: Complete the item.
    requirement_refs:
      - R1
    depends_on: []
    owned_surfaces:
      - README.md
    verification:
      - make check
    acceptance:
      - The item is implemented and verified.
    notes: null
"#
        ),
    )?;
    Ok(())
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
