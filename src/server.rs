use crate::{
    backlog, bundle, dispatch, findings,
    models::{
        ActionResult, ClaimNextTaskParams, CreateBacklogItemParams, CreatedBacklogItemData,
        DispatchNextWorkData, DoctorSnapshotData, DraftBacklogData, DraftBacklogItemsParams,
        FindingDispositionData, FindingListData, FindingRecordData, FindingValidationData,
        GenerateTaskBundleParams, InitProjectParams, InspectTaskEventsParams, InspectTaskParams,
        LimitParams, ListFindingsParams, PingData, PingParams, ProjectScaffoldData,
        ProjectStatusData, RecordFindingParams, RootParams, RunnerPrepareParams, RunnerReportData,
        SendWorkerGuidanceParams, TaskBundleData, TaskEventListData, TaskRecordData,
        UnsupportedData, UpdateFindingDispositionParams, ValidateBacklogParams,
        ValidateFindingsParams, WorktreeCreateParams, WorktreeData, WorktreeStatusParams,
    },
    project, runner, tasks, workspace,
};
use anyhow::Result;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router, Json, ServerHandler, ServiceExt,
};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Debug)]
pub struct PlatypusMcp {
    tool_router: ToolRouter<Self>,
    default_root: PathBuf,
}

impl PlatypusMcp {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            default_root: std::env::var("PLATYPUS_MCP_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        }
    }

    pub fn tool_names(&self) -> BTreeSet<String> {
        self.tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect()
    }
}

impl Default for PlatypusMcp {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler]
impl ServerHandler for PlatypusMcp {}

#[tool_router(router = tool_router)]
impl PlatypusMcp {
    #[tool(
        title = "Ping",
        description = "Health check for the Platypus MCP server.",
        annotations(
            title = "Ping",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn ping(
        &self,
        Parameters(params): Parameters<PingParams>,
    ) -> Json<ActionResult<PingData>> {
        Json(ActionResult::completed(
            "ping",
            "Platypus MCP server is available.",
            PingData {
                echo: params.message.unwrap_or_else(|| "pong".to_string()),
            },
        ))
    }

    #[tool(
        title = "Inspect Status",
        description = "Inspect current project and backlog status.",
        annotations(
            title = "Inspect Status",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn inspect_status(
        &self,
        Parameters(params): Parameters<LimitParams>,
    ) -> Json<ActionResult<ProjectStatusData>> {
        Json(backlog::inspect_status(
            &self.default_root,
            params.root.as_deref(),
            params.limit,
        ))
    }

    #[tool(
        title = "Project Status",
        description = "Compatibility alias for inspect_status.",
        annotations(
            title = "Project Status",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn project_status(
        &self,
        Parameters(params): Parameters<RootParams>,
    ) -> Json<ActionResult<ProjectStatusData>> {
        Json(backlog::inspect_status(
            &self.default_root,
            params.root.as_deref(),
            Some(20),
        ))
    }

    #[tool(
        title = "List Backlog",
        description = "List runnable backlog candidates.",
        annotations(
            title = "List Backlog",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn list_backlog(
        &self,
        Parameters(params): Parameters<LimitParams>,
    ) -> Json<ActionResult<crate::models::BacklogListData>> {
        Json(backlog::list_backlog(
            &self.default_root,
            params.root.as_deref(),
            params.limit,
        ))
    }

    #[tool(
        title = "Validate Backlog",
        description = "Validate structured Platypus backlog files.",
        annotations(
            title = "Validate Backlog",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn validate_backlog(
        &self,
        Parameters(params): Parameters<ValidateBacklogParams>,
    ) -> Json<ActionResult<crate::models::BacklogValidationData>> {
        Json(backlog::validate_backlog(
            &self.default_root,
            params.root.as_deref(),
            params.include_errors.unwrap_or(true),
        ))
    }

    #[tool(
        title = "Doctor Snapshot",
        description = "Inspect project setup and recovery guidance.",
        annotations(
            title = "Doctor Snapshot",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn doctor_snapshot(
        &self,
        Parameters(params): Parameters<RootParams>,
    ) -> Json<ActionResult<DoctorSnapshotData>> {
        Json(project::doctor_snapshot(
            &self.default_root,
            params.root.as_deref(),
        ))
    }

    #[tool(
        title = "Init Project",
        description = "Initialize missing Platypus project scaffold files.",
        annotations(
            title = "Init Project",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn init_project(
        &self,
        Parameters(params): Parameters<InitProjectParams>,
    ) -> Json<ActionResult<ProjectScaffoldData>> {
        Json(project::init_project(&self.default_root, params))
    }

    #[tool(
        title = "Draft Backlog Items",
        description = "Draft typed backlog candidates from a product goal.",
        annotations(
            title = "Draft Backlog Items",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn draft_backlog_items(
        &self,
        Parameters(params): Parameters<DraftBacklogItemsParams>,
    ) -> Json<ActionResult<DraftBacklogData>> {
        Json(backlog::draft_backlog_items(params))
    }

    #[tool(
        title = "Create Backlog Item",
        description = "Create one structured backlog item.",
        annotations(
            title = "Create Backlog Item",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn create_backlog_item(
        &self,
        Parameters(params): Parameters<CreateBacklogItemParams>,
    ) -> Json<ActionResult<CreatedBacklogItemData>> {
        Json(backlog::create_backlog_item(&self.default_root, params))
    }

    #[tool(
        title = "Dispatch Next Work",
        description = "Dispatch the next runnable backlog item.",
        annotations(
            title = "Dispatch Next Work",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn dispatch_next_work(
        &self,
        Parameters(params): Parameters<RootParams>,
    ) -> Json<ActionResult<DispatchNextWorkData>> {
        Json(dispatch::dispatch_next_work(&self.default_root, params))
    }

    #[tool(
        title = "Inspect Task",
        description = "Inspect one persisted task record.",
        annotations(
            title = "Inspect Task",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn inspect_task(
        &self,
        Parameters(params): Parameters<InspectTaskParams>,
    ) -> Json<ActionResult<TaskRecordData>> {
        Json(tasks::inspect_task(&self.default_root, params))
    }

    #[tool(
        title = "Claim Next Task",
        description = "Atomically claim the next queued task for an external runner.",
        annotations(
            title = "Claim Next Task",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn claim_next_task(
        &self,
        Parameters(params): Parameters<ClaimNextTaskParams>,
    ) -> Json<ActionResult<TaskRecordData>> {
        Json(tasks::claim_next_task(&self.default_root, params))
    }

    #[tool(
        title = "Worktree Create",
        description = "Create an isolated Git worktree for a task.",
        annotations(
            title = "Worktree Create",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn worktree_create(
        &self,
        Parameters(params): Parameters<WorktreeCreateParams>,
    ) -> Json<ActionResult<WorktreeData>> {
        Json(workspace::worktree_create(&self.default_root, params))
    }

    #[tool(
        title = "Worktree Status",
        description = "Inspect the persisted Git worktree for a task.",
        annotations(
            title = "Worktree Status",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn worktree_status(
        &self,
        Parameters(params): Parameters<WorktreeStatusParams>,
    ) -> Json<ActionResult<WorktreeData>> {
        Json(workspace::worktree_status(&self.default_root, params))
    }

    #[tool(
        title = "Generate Task Bundle",
        description = "Generate a deterministic worker brief for a task.",
        annotations(
            title = "Generate Task Bundle",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn generate_task_bundle(
        &self,
        Parameters(params): Parameters<GenerateTaskBundleParams>,
    ) -> Json<ActionResult<TaskBundleData>> {
        Json(bundle::generate_task_bundle(&self.default_root, params))
    }

    #[tool(
        title = "Runner Prepare Next",
        description = "Claim queued tasks and prepare worktrees and bundles without executing workers.",
        annotations(
            title = "Runner Prepare Next",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn runner_prepare_next(
        &self,
        Parameters(params): Parameters<RunnerPrepareParams>,
    ) -> Json<ActionResult<RunnerReportData>> {
        Json(runner::prepare_next(&self.default_root, params))
    }

    #[tool(
        title = "Inspect Task Events",
        description = "Inspect recent task supervision events.",
        annotations(
            title = "Inspect Task Events",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn inspect_task_events(
        &self,
        Parameters(params): Parameters<InspectTaskEventsParams>,
    ) -> Json<ActionResult<TaskEventListData>> {
        Json(tasks::inspect_task_events(&self.default_root, params))
    }

    #[tool(
        title = "Send Worker Guidance",
        description = "Send guidance to a running worker task.",
        annotations(
            title = "Send Worker Guidance",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn send_worker_guidance(
        &self,
        Parameters(params): Parameters<SendWorkerGuidanceParams>,
    ) -> Json<ActionResult<UnsupportedData>> {
        Json(not_implemented(
            "send_worker_guidance",
            params.root.as_deref(),
            &format!(
                "Worker mailbox delivery is not wired yet for task `{}`.",
                params.task_id
            ),
        ))
    }

    #[tool(
        title = "Record Finding",
        description = "Persist one implementation finding for later disposition.",
        annotations(
            title = "Record Finding",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn record_finding(
        &self,
        Parameters(params): Parameters<RecordFindingParams>,
    ) -> Json<ActionResult<FindingRecordData>> {
        Json(findings::record_finding(&self.default_root, params))
    }

    #[tool(
        title = "List Findings",
        description = "List implementation findings awaiting disposition.",
        annotations(
            title = "List Findings",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn list_findings(
        &self,
        Parameters(params): Parameters<ListFindingsParams>,
    ) -> Json<ActionResult<FindingListData>> {
        Json(findings::list_findings(&self.default_root, params))
    }

    #[tool(
        title = "Validate Findings",
        description = "Validate finding dispositions.",
        annotations(
            title = "Validate Findings",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn validate_findings(
        &self,
        Parameters(params): Parameters<ValidateFindingsParams>,
    ) -> Json<ActionResult<FindingValidationData>> {
        Json(findings::validate_findings(&self.default_root, params))
    }

    #[tool(
        title = "Update Finding Disposition",
        description = "Persist a manager disposition for one finding.",
        annotations(
            title = "Update Finding Disposition",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn update_finding_disposition(
        &self,
        Parameters(params): Parameters<UpdateFindingDispositionParams>,
    ) -> Json<ActionResult<FindingDispositionData>> {
        Json(findings::update_finding_disposition(
            &self.default_root,
            params,
        ))
    }
}

pub async fn serve_stdio() -> Result<()> {
    let server = PlatypusMcp::new().serve(rmcp::transport::stdio()).await?;
    server.waiting().await?;
    Ok(())
}

fn not_implemented(
    action: &str,
    _root: Option<&str>,
    reason: &str,
) -> ActionResult<UnsupportedData> {
    ActionResult {
        action: action.to_string(),
        status: crate::models::ActionStatus::Skipped,
        summary: format!("{} is not implemented in the Rust MCP server yet.", action),
        next_action: Some("Use backlog inspection tools or wait for runtime wiring.".to_string()),
        data: Some(UnsupportedData {
            supported: false,
            reason: reason.to_string(),
        }),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::TaskSupport;

    #[test]
    fn tool_router_exposes_basic_tool_set() {
        let server = PlatypusMcp::new();
        let names = server.tool_names();
        for expected in [
            "ping",
            "inspect_status",
            "project_status",
            "list_backlog",
            "validate_backlog",
            "doctor_snapshot",
            "init_project",
            "draft_backlog_items",
            "create_backlog_item",
            "dispatch_next_work",
            "inspect_task",
            "claim_next_task",
            "worktree_create",
            "worktree_status",
            "generate_task_bundle",
            "runner_prepare_next",
            "inspect_task_events",
            "send_worker_guidance",
            "record_finding",
            "list_findings",
            "validate_findings",
            "update_finding_disposition",
        ] {
            assert!(names.contains(expected), "missing tool {expected}");
        }
    }

    #[test]
    fn tools_expose_schema_annotations_and_execution_hints() {
        let server = PlatypusMcp::new();
        let tools = server.tool_router.list_all();
        let mutating_tools = BTreeSet::from([
            "init_project",
            "create_backlog_item",
            "dispatch_next_work",
            "claim_next_task",
            "worktree_create",
            "runner_prepare_next",
            "send_worker_guidance",
            "record_finding",
            "update_finding_disposition",
        ]);

        for tool in tools {
            let name = tool.name.as_ref();
            assert!(tool.title.is_some(), "{name} missing title");
            assert!(tool.description.is_some(), "{name} missing description");
            assert!(
                tool.output_schema.is_some(),
                "{name} missing structured output schema"
            );

            let annotations = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{name} missing annotations"));
            let is_mutating = mutating_tools.contains(name);
            assert_eq!(
                annotations.read_only_hint,
                Some(!is_mutating),
                "{name} has wrong read-only hint"
            );
            assert_eq!(
                annotations.open_world_hint,
                Some(false),
                "{name} should be closed-world"
            );
            assert_eq!(
                annotations.destructive_hint,
                Some(name == "init_project"),
                "{name} has wrong destructive hint"
            );

            let expected_idempotent = !is_mutating || name == "update_finding_disposition";
            assert_eq!(
                annotations.idempotent_hint,
                Some(expected_idempotent),
                "{name} has wrong idempotent hint"
            );

            let execution = tool
                .execution
                .as_ref()
                .unwrap_or_else(|| panic!("{name} missing execution metadata"));
            assert_eq!(
                execution.task_support,
                Some(TaskSupport::Forbidden),
                "{name} should forbid task invocation"
            );
        }
    }
}
