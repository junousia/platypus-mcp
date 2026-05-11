use crate::{
    approvals, assignments, backlog, bundle, config, dispatch, events, evidence, findings, goal,
    guidance, host_guidance, host_lifecycle, integrations, leases,
    models::{
        AcquireLeaseParams, ActionResult, AgentProfileData, AgentProfilesData, AgentProfilesParams,
        ApprovalListData, ApprovalListParams, ApprovalRespondParams, ApprovalResponseData,
        ClaimNextTaskParams, ClassifyPlanningNeedsParams, ClassifyWorkflowFitParams,
        CompleteWorkerExecutionParams, ConfigureAgentProfileParams, CreateBacklogItemParams,
        CreateBacklogItemsParams, CreateEpicParams, CreatedBacklogItemData,
        CreatedBacklogItemsData, CreatedEpicData, DispatchNextWorkData, DispatchReadyWorkData,
        DispatchReadyWorkParams, DoctorSnapshotData, DraftBacklogData, DraftBacklogItemsParams,
        DraftExternalBacklogItemsParams, DraftExternalReportParams, DraftTaskPlanParams,
        EventsReplayData, EventsReplayParams, EvidenceListData, EvidenceRecordData,
        ExternalBacklogDraftData, ExternalReportApprovalData, ExternalReportDispatchData,
        ExternalReportDraftData, FindingDispositionData, FindingListData, FindingRecordData,
        FindingValidationData, FinishWorkData, FinishWorkParams, GenerateTaskBundleParams,
        GitHubIssueImportData, ImportGitHubIssuesParams, InitProjectParams,
        InspectDependencyGraphParams, InspectTaskEventsParams, InspectTaskParams,
        InspectWorkQueueParams, InspectWorkerAssignmentParams, IntegrateWorkerResultParams,
        LeaseListData, LeaseRecordData, LimitParams, ListEpicsData, ListEvidenceParams,
        ListFindingsParams, ListLeasesParams, NextSafeActionData, NextSafeActionParams, PingData,
        PingParams, PlanGoalWorkData, PlanGoalWorkParams, PlanningClassificationData,
        PrepareWorkData, PrepareWorkParams, PrepareWorkerAssignmentParams, ProjectScaffoldData,
        ProjectStatusData, ReconcileParams, ReconciliationData, RecordEvidenceParams,
        RecordExternalReportDispatchParams, RecordFindingParams, RecordVerificationEvidenceParams,
        RecordWorkerEventParams, ReleaseLeaseParams, RenewLeaseParams,
        RequestExternalReportApprovalParams, RequestPlanningApprovalParams, RootParams,
        RunTaskVerificationParams, RunnerPrepareParams, RunnerReportData, SendWorkerGuidanceParams,
        StartGoalWorkData, StartGoalWorkParams, StartWorkerExecutionParams,
        StorageCapabilityProbeData, StorageCapabilityProbeParams, TaskBundleData,
        TaskEventListData, TaskPlanData, TaskPlanItemParams, TaskPlanListData, TaskPlanQueryParams,
        TaskPlanValidationData, TaskPlanWriteData, TaskRecordData, TaskVerificationRunData,
        UpdateFindingDispositionParams, ValidateBacklogParams, ValidateFindingsParams,
        WorkQueueData, WorkerAssignmentData, WorkerAssignmentEventData, WorkerGuidanceData,
        WorkerResultIntegrationData, WorkflowConfigData, WorkflowConfigParams, WorkflowFitData,
        WorktreeCleanupData, WorktreeCleanupParams, WorktreeCreateParams, WorktreeData,
        WorktreeDiffData, WorktreeDiffParams, WorktreeStatusParams, WriteTaskPlanParams,
    },
    project, reconcile, runner, storage, tasks, workspace,
};
use anyhow::Result as AnyhowResult;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        ContextInclusion, CreateMessageRequestParams, GetPromptRequestParams, GetPromptResult,
        ListPromptsResult, ListResourcesResult, PaginatedRequestParams, ReadResourceRequestParams,
        ReadResourceResult, ResourceContents, SamplingMessage, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData as McpError, Json, Peer, RoleServer, ServerHandler,
    ServiceExt,
};
use serde_json::{Map, Value};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

#[derive(Clone, Debug)]
pub struct PlatypusMcp {
    tool_router: ToolRouter<Self>,
    default_root: PathBuf,
}

impl PlatypusMcp {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router_with_compatible_schemas(),
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

    fn tool_router_with_compatible_schemas() -> ToolRouter<Self> {
        let mut router = Self::tool_router();
        for route in router.map.values_mut() {
            route.attr.input_schema =
                Arc::new(normalize_schema_object((*route.attr.input_schema).clone()));
            if let Some(output_schema) = route.attr.output_schema.as_ref() {
                route.attr.output_schema =
                    Some(Arc::new(normalize_schema_object((**output_schema).clone())));
            }
        }
        router
    }
}

fn normalize_schema_object(schema: rmcp::model::JsonObject) -> rmcp::model::JsonObject {
    let mut value = Value::Object(schema);
    normalize_schema_value(&mut value);
    match value {
        Value::Object(schema) => schema,
        _ => Map::new(),
    }
}

fn normalize_schema_value(value: &mut Value) {
    match value {
        Value::Object(schema) => {
            if matches!(schema.get("nullable"), Some(Value::Bool(true)))
                && !schema.contains_key("type")
                && matches!(schema.get("const"), Some(Value::Null))
            {
                schema.remove("nullable");
                schema.insert("type".to_string(), Value::String("null".to_string()));
            }

            if schema.get("type").and_then(Value::as_str) == Some("integer")
                && schema
                    .get("format")
                    .and_then(Value::as_str)
                    .is_some_and(is_rust_unsigned_integer_format)
            {
                schema.remove("format");
            }

            for child in schema.values_mut() {
                normalize_schema_value(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_schema_value(item);
            }
        }
        _ => {}
    }
}

fn is_rust_unsigned_integer_format(format: &str) -> bool {
    matches!(
        format,
        "uint" | "uint8" | "uint16" | "uint32" | "uint64" | "usize"
    )
}

impl Default for PlatypusMcp {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler]
impl ServerHandler for PlatypusMcp {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .enable_prompts()
            .build();
        info.instructions = Some(
            "Use Platypus MCP for spec-driven development: inspect first, convert goals to backlog, plan non-trivial work, dispatch through worktrees, then record evidence and integrate. Read `platypus://guidance/spec-driven-development` or get the `platypus-spec-driven-development` prompt before shaping free-form goals."
                .to_string(),
        );
        info
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(
            host_guidance::resource_list(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResult, McpError> {
        let Some(entry) = host_guidance::by_uri(&request.uri) else {
            return Err(McpError::resource_not_found(
                format!("unknown Platypus guidance resource `{}`", request.uri),
                None,
            ));
        };

        Ok(ReadResourceResult {
            contents: vec![ResourceContents::TextResourceContents {
                uri: entry.uri.to_string(),
                mime_type: Some("text/markdown".to_string()),
                text: entry.text.to_string(),
                meta: None,
            }],
        })
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(
            host_guidance::prompt_list(),
        ))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<GetPromptResult, McpError> {
        let Some(entry) = host_guidance::by_prompt_name(&request.name) else {
            return Err(McpError::invalid_params(
                format!("unknown Platypus guidance prompt `{}`", request.name),
                None,
            ));
        };

        Ok(GetPromptResult {
            description: Some(entry.description.to_string()),
            messages: host_guidance::prompt_messages(entry),
        })
    }
}

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
        title = "Next Safe Action",
        description = "Recommend the next safe Platypus tool call for the current project state.",
        annotations(
            title = "Next Safe Action",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn next_safe_action(
        &self,
        Parameters(params): Parameters<NextSafeActionParams>,
    ) -> Json<ActionResult<NextSafeActionData>> {
        Json(guidance::next_safe_action(&self.default_root, params))
    }

    #[tool(
        title = "Inspect Work Queue",
        description = "Inspect runnable backlog work with task-plan status and recommended next tool.",
        annotations(
            title = "Inspect Work Queue",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn inspect_work_queue(
        &self,
        Parameters(params): Parameters<InspectWorkQueueParams>,
    ) -> Json<ActionResult<WorkQueueData>> {
        Json(guidance::inspect_work_queue(&self.default_root, params))
    }

    #[tool(
        title = "Classify Planning Needs",
        description = "Classify runnable backlog items as direct, standard, or full planning mode.",
        annotations(
            title = "Classify Planning Needs",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn classify_planning_needs(
        &self,
        Parameters(params): Parameters<ClassifyPlanningNeedsParams>,
    ) -> Json<ActionResult<PlanningClassificationData>> {
        Json(guidance::classify_planning_needs(
            &self.default_root,
            params,
        ))
    }

    #[tool(
        title = "Classify Workflow Fit",
        description = "Classify whether a user goal should use direct scaffolding, full Platypus workflow, or a hybrid flow.",
        annotations(
            title = "Classify Workflow Fit",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn classify_workflow_fit(
        &self,
        Parameters(params): Parameters<ClassifyWorkflowFitParams>,
    ) -> Json<ActionResult<WorkflowFitData>> {
        Json(guidance::classify_workflow_fit(&self.default_root, params))
    }

    #[tool(
        title = "Plan Goal Work",
        description = "Plan a user goal without mutating project state and return the concrete next Platypus tool call.",
        annotations(
            title = "Plan Goal Work",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn plan_goal_work(
        &self,
        Parameters(params): Parameters<PlanGoalWorkParams>,
    ) -> Json<ActionResult<PlanGoalWorkData>> {
        Json(goal::plan_goal_work(&self.default_root, params))
    }

    #[tool(
        title = "Start Goal Work",
        description = "Start goal-oriented workflow in one call: classify mode, create or reuse tracking, and optionally dispatch prepared work.",
        annotations(
            title = "Start Goal Work",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn start_goal_work(
        &self,
        Parameters(params): Parameters<StartGoalWorkParams>,
    ) -> Json<ActionResult<StartGoalWorkData>> {
        Json(goal::start_goal_work(&self.default_root, params))
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
        title = "Inspect Backlog Inventory",
        description = "Inspect all backlog items with runnable, blocked, and Git-trailer closure reasons.",
        annotations(
            title = "Inspect Backlog Inventory",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn inspect_backlog_inventory(
        &self,
        Parameters(params): Parameters<LimitParams>,
    ) -> Json<ActionResult<crate::models::BacklogInventoryData>> {
        Json(backlog::inspect_backlog_inventory(
            &self.default_root,
            params.root.as_deref(),
            params.limit,
        ))
    }

    #[tool(
        title = "Inspect Dependency Graph",
        description = "Inspect backlog dependency graph nodes, edges, runnable state, closure state, missing references, and cycles.",
        annotations(
            title = "Inspect Dependency Graph",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn inspect_dependency_graph(
        &self,
        Parameters(params): Parameters<InspectDependencyGraphParams>,
    ) -> Json<ActionResult<crate::models::BacklogDependencyGraphData>> {
        Json(backlog::inspect_dependency_graph(
            &self.default_root,
            params,
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
        description = "Optionally draft typed backlog candidates from a product goal using MCP client sampling; skips when sampling is unavailable.",
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
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<DraftBacklogItemsParams>,
    ) -> Json<ActionResult<DraftBacklogData>> {
        if !client_supports_sampling(&peer) {
            return Json(backlog::draft_backlog_items(params));
        }
        let prompt = match backlog::draft_backlog_items_sampling_prompt(&params) {
            Ok(prompt) => prompt,
            Err(error) => {
                return Json(ActionResult::failed(
                    "draft_backlog_items",
                    "Could not draft backlog items.",
                    error,
                ))
            }
        };
        let text = match sample_text(&peer, "Backlog drafting assistant.", prompt, 4_000).await {
            Ok(text) => text,
            Err(error) => {
                return Json(ActionResult::failed(
                    "draft_backlog_items",
                    "Could not draft backlog items.",
                    error,
                ))
            }
        };
        Json(backlog::draft_backlog_items_from_sample(&params, &text))
    }

    #[tool(
        title = "Draft External Backlog Items",
        description = "Draft provider-neutral backlog candidates from host-provided external work records.",
        annotations(
            title = "Draft External Backlog Items",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn draft_external_backlog_items(
        &self,
        Parameters(params): Parameters<DraftExternalBacklogItemsParams>,
    ) -> Json<ActionResult<ExternalBacklogDraftData>> {
        Json(integrations::draft_external_backlog_items(
            &self.default_root,
            params,
        ))
    }

    #[tool(
        title = "Import GitHub Issues",
        description = "Import host-provided GitHub issue records as local backlog snapshots.",
        annotations(
            title = "Import GitHub Issues",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn import_github_issues(
        &self,
        Parameters(params): Parameters<ImportGitHubIssuesParams>,
    ) -> Json<ActionResult<GitHubIssueImportData>> {
        Json(integrations::import_github_issues(
            &self.default_root,
            params,
        ))
    }

    #[tool(
        title = "Draft External Report",
        description = "Draft a provider-neutral external report payload from local backlog, task, and evidence state.",
        annotations(
            title = "Draft External Report",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn draft_external_report(
        &self,
        Parameters(params): Parameters<DraftExternalReportParams>,
    ) -> Json<ActionResult<ExternalReportDraftData>> {
        Json(integrations::draft_external_report(
            &self.default_root,
            params,
        ))
    }

    #[tool(
        title = "Request External Report Approval",
        description = "Request durable approval before sending a drafted external report through a host or plugin provider.",
        annotations(
            title = "Request External Report Approval",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn request_external_report_approval(
        &self,
        Parameters(params): Parameters<RequestExternalReportApprovalParams>,
    ) -> Json<ActionResult<ExternalReportApprovalData>> {
        Json(integrations::request_external_report_approval(
            &self.default_root,
            params,
        ))
    }

    #[tool(
        title = "Request Planning Approval",
        description = "Request durable approval for a task plan or backlog tranche before dispatching non-direct work.",
        annotations(
            title = "Request Planning Approval",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn request_planning_approval(
        &self,
        Parameters(params): Parameters<RequestPlanningApprovalParams>,
    ) -> Json<ActionResult<crate::models::PlanningApprovalData>> {
        Json(approvals::request_planning_approval(
            &self.default_root,
            params,
        ))
    }

    #[tool(
        title = "Record External Report Dispatch",
        description = "Record an approved host/plugin external report dispatch result.",
        annotations(
            title = "Record External Report Dispatch",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn record_external_report_dispatch(
        &self,
        Parameters(params): Parameters<RecordExternalReportDispatchParams>,
    ) -> Json<ActionResult<ExternalReportDispatchData>> {
        Json(integrations::record_external_report_dispatch(
            &self.default_root,
            params,
        ))
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
        title = "Create Backlog Items",
        description = "Atomically create multiple structured backlog item files.",
        annotations(
            title = "Create Backlog Items",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn create_backlog_items(
        &self,
        Parameters(params): Parameters<CreateBacklogItemsParams>,
    ) -> Json<ActionResult<CreatedBacklogItemsData>> {
        Json(backlog::create_backlog_items(&self.default_root, params))
    }

    #[tool(
        title = "Create Epic",
        description = "Create one structured backlog epic file.",
        annotations(
            title = "Create Epic",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn create_epic(
        &self,
        Parameters(params): Parameters<CreateEpicParams>,
    ) -> Json<ActionResult<CreatedEpicData>> {
        Json(backlog::create_epic(&self.default_root, params))
    }

    #[tool(
        title = "List Epics",
        description = "List structured backlog epics.",
        annotations(
            title = "List Epics",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn list_epics(
        &self,
        Parameters(params): Parameters<RootParams>,
    ) -> Json<ActionResult<ListEpicsData>> {
        Json(backlog::list_epics(&self.default_root, params))
    }

    #[tool(
        title = "Draft Task Plan",
        description = "Optionally draft a strict task plan using MCP client sampling; skips when sampling is unavailable.",
        annotations(
            title = "Draft Task Plan",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn draft_task_plan(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<DraftTaskPlanParams>,
    ) -> Json<ActionResult<TaskPlanData>> {
        if !client_supports_sampling(&peer) {
            return Json(backlog::draft_task_plan(&self.default_root, params));
        }
        let prompt = match backlog::draft_task_plan_sampling_prompt(&self.default_root, &params) {
            Ok(prompt) => prompt,
            Err(error) => {
                return Json(ActionResult::failed(
                    "draft_task_plan",
                    "Could not draft task plan.",
                    error,
                ))
            }
        };
        let text = match sample_text(&peer, "Task planning assistant.", prompt, 6_000).await {
            Ok(text) => text,
            Err(error) => {
                return Json(ActionResult::failed(
                    "draft_task_plan",
                    "Could not draft task plan.",
                    error,
                ))
            }
        };
        Json(backlog::draft_task_plan_from_sample(
            &self.default_root,
            &params,
            &text,
        ))
    }

    #[tool(
        title = "Inspect Task Plan",
        description = "Read one committed task plan from backlog/plans.",
        annotations(
            title = "Inspect Task Plan",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn inspect_task_plan(
        &self,
        Parameters(params): Parameters<TaskPlanItemParams>,
    ) -> Json<ActionResult<TaskPlanData>> {
        Json(backlog::inspect_task_plan(&self.default_root, params))
    }

    #[tool(
        title = "List Task Plans",
        description = "List committed task plans under backlog/plans.",
        annotations(
            title = "List Task Plans",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn list_task_plans(
        &self,
        Parameters(params): Parameters<TaskPlanQueryParams>,
    ) -> Json<ActionResult<TaskPlanListData>> {
        Json(backlog::list_task_plans(&self.default_root, params))
    }

    #[tool(
        title = "Validate Task Plan",
        description = "Validate strict task plan YAML under backlog/plans.",
        annotations(
            title = "Validate Task Plan",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn validate_task_plan(
        &self,
        Parameters(params): Parameters<TaskPlanQueryParams>,
    ) -> Json<ActionResult<TaskPlanValidationData>> {
        Json(backlog::validate_task_plan(&self.default_root, params))
    }

    #[tool(
        title = "Write Task Plan",
        description = "Write one strict task plan to backlog/plans.",
        annotations(
            title = "Write Task Plan",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn write_task_plan(
        &self,
        Parameters(params): Parameters<WriteTaskPlanParams>,
    ) -> Json<ActionResult<TaskPlanWriteData>> {
        Json(backlog::write_task_plan(&self.default_root, params))
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
        title = "Dispatch Ready Work",
        description = "Dispatch multiple runnable backlog items and prepare worker handoffs in one safe batch.",
        annotations(
            title = "Dispatch Ready Work",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn dispatch_ready_work(
        &self,
        Parameters(params): Parameters<DispatchReadyWorkParams>,
    ) -> Json<ActionResult<DispatchReadyWorkData>> {
        Json(dispatch::dispatch_ready_work(&self.default_root, params))
    }

    #[tool(
        title = "Prepare Work",
        description = "Inspect the executable queue and prepare the next host-run action. Direct work returns direct-edit guidance; non-direct work prepares a manual handoff worktree without launching a worker.",
        annotations(
            title = "Prepare Work",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn prepare_work(
        &self,
        Parameters(params): Parameters<PrepareWorkParams>,
    ) -> Json<ActionResult<PrepareWorkData>> {
        Json(host_lifecycle::prepare_work(&self.default_root, params))
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
        title = "Worktree Diff",
        description = "Inspect bounded changes in a persisted task worktree.",
        annotations(
            title = "Worktree Diff",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn worktree_diff(
        &self,
        Parameters(params): Parameters<WorktreeDiffParams>,
    ) -> Json<ActionResult<WorktreeDiffData>> {
        Json(workspace::worktree_diff(&self.default_root, params))
    }

    #[tool(
        title = "Inspect Worktree Changes",
        description = "Friendly alias for worktree_diff. Inspect bounded changes in a persisted task worktree.",
        annotations(
            title = "Inspect Worktree Changes",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn inspect_worktree_changes(
        &self,
        Parameters(params): Parameters<WorktreeDiffParams>,
    ) -> Json<ActionResult<WorktreeDiffData>> {
        let mut result = workspace::worktree_diff(&self.default_root, params);
        result.action = "inspect_worktree_changes".to_string();
        Json(result)
    }

    #[tool(
        title = "Worktree Cleanup",
        description = "Remove a persisted task worktree when it is clean or explicitly forced.",
        annotations(
            title = "Worktree Cleanup",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn worktree_cleanup(
        &self,
        Parameters(params): Parameters<WorktreeCleanupParams>,
    ) -> Json<ActionResult<WorktreeCleanupData>> {
        Json(workspace::worktree_cleanup(&self.default_root, params))
    }

    #[tool(
        title = "Integrate Worker Result",
        description = "Integrate a completed verified worker task worktree into the manager workspace.",
        annotations(
            title = "Integrate Worker Result",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn integrate_worker_result(
        &self,
        Parameters(params): Parameters<IntegrateWorkerResultParams>,
    ) -> Json<ActionResult<WorkerResultIntegrationData>> {
        Json(workspace::integrate_worker_result(
            &self.default_root,
            params,
        ))
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
        title = "Prepare Worker Assignment",
        description = "Claim a task, create or reuse its worktree, generate a worker bundle, and persist a single assignment handoff object.",
        annotations(
            title = "Prepare Worker Assignment",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn prepare_worker_assignment(
        &self,
        Parameters(params): Parameters<PrepareWorkerAssignmentParams>,
    ) -> Json<ActionResult<WorkerAssignmentData>> {
        Json(assignments::prepare_worker_assignment(
            &self.default_root,
            params,
        ))
    }

    #[tool(
        title = "Prepare Worker Handoff",
        description = "Friendly alias for prepare_worker_assignment. Prepare a single handoff object for an external worker.",
        annotations(
            title = "Prepare Worker Handoff",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn prepare_worker_handoff(
        &self,
        Parameters(params): Parameters<PrepareWorkerAssignmentParams>,
    ) -> Json<ActionResult<WorkerAssignmentData>> {
        let mut result = assignments::prepare_worker_assignment(&self.default_root, params);
        result.action = "prepare_worker_handoff".to_string();
        Json(result)
    }

    #[tool(
        title = "Inspect Worker Assignment",
        description = "Inspect one persisted worker assignment handoff object.",
        annotations(
            title = "Inspect Worker Assignment",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn inspect_worker_assignment(
        &self,
        Parameters(params): Parameters<InspectWorkerAssignmentParams>,
    ) -> Json<ActionResult<WorkerAssignmentData>> {
        Json(assignments::inspect_worker_assignment(
            &self.default_root,
            params,
        ))
    }

    #[tool(
        title = "Start Worker Execution",
        description = "Record that an external worker harness has begun a prepared assignment. This only updates Platypus lifecycle state; it does not launch a worker process.",
        annotations(
            title = "Start Worker Execution",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn start_worker_execution(
        &self,
        Parameters(params): Parameters<StartWorkerExecutionParams>,
    ) -> Json<ActionResult<WorkerAssignmentData>> {
        Json(assignments::start_worker_execution(
            &self.default_root,
            params,
        ))
    }

    #[tool(
        title = "Start Worker Task",
        description = "Friendly alias for start_worker_execution. Record that an external worker harness has begun a prepared handoff; this does not launch a worker process.",
        annotations(
            title = "Start Worker Task",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn start_worker_task(
        &self,
        Parameters(params): Parameters<StartWorkerExecutionParams>,
    ) -> Json<ActionResult<WorkerAssignmentData>> {
        let mut result = assignments::start_worker_execution(&self.default_root, params);
        result.action = "start_worker_task".to_string();
        Json(result)
    }

    #[tool(
        title = "Record Worker Event",
        description = "Persist a progress event for a running worker assignment.",
        annotations(
            title = "Record Worker Event",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn record_worker_event(
        &self,
        Parameters(params): Parameters<RecordWorkerEventParams>,
    ) -> Json<ActionResult<WorkerAssignmentEventData>> {
        Json(assignments::record_worker_event(&self.default_root, params))
    }

    #[tool(
        title = "Record Worker Progress",
        description = "Friendly alias for record_worker_event. Persist a progress update for a running worker task.",
        annotations(
            title = "Record Worker Progress",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn record_worker_progress(
        &self,
        Parameters(params): Parameters<RecordWorkerEventParams>,
    ) -> Json<ActionResult<WorkerAssignmentEventData>> {
        let mut result = assignments::record_worker_event(&self.default_root, params);
        result.action = "record_worker_progress".to_string();
        Json(result)
    }

    #[tool(
        title = "Complete Worker Execution",
        description = "Persist a worker result and finish the assigned task with guarded result data.",
        annotations(
            title = "Complete Worker Execution",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn complete_worker_execution(
        &self,
        Parameters(params): Parameters<CompleteWorkerExecutionParams>,
    ) -> Json<ActionResult<WorkerAssignmentData>> {
        Json(assignments::complete_worker_execution(
            &self.default_root,
            params,
        ))
    }

    #[tool(
        title = "Complete Worker Task",
        description = "Friendly alias for complete_worker_execution. Persist a worker result and finish the assigned task.",
        annotations(
            title = "Complete Worker Task",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn complete_worker_task(
        &self,
        Parameters(params): Parameters<CompleteWorkerExecutionParams>,
    ) -> Json<ActionResult<WorkerAssignmentData>> {
        let mut result = assignments::complete_worker_execution(&self.default_root, params);
        result.action = "complete_worker_task".to_string();
        Json(result)
    }

    #[tool(
        title = "Finish Work",
        description = "Complete a host-run worker assignment and return the next lifecycle action: verify, record findings, integrate, recover, or finish.",
        annotations(
            title = "Finish Work",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn finish_work(
        &self,
        Parameters(params): Parameters<FinishWorkParams>,
    ) -> Json<ActionResult<FinishWorkData>> {
        Json(host_lifecycle::finish_work(&self.default_root, params))
    }

    #[tool(
        title = "Run Task Verification",
        description = "Run the assignment verification command in the task worktree and persist a verification run event. Requires assignment_id or task_id.",
        annotations(
            title = "Run Task Verification",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn run_task_verification(
        &self,
        Parameters(params): Parameters<RunTaskVerificationParams>,
    ) -> Json<ActionResult<TaskVerificationRunData>> {
        Json(assignments::run_task_verification(
            &self.default_root,
            params,
        ))
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
        title = "Approval List",
        description = "List pending or completed approval requests.",
        annotations(
            title = "Approval List",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn approval_list(
        &self,
        Parameters(params): Parameters<ApprovalListParams>,
    ) -> Json<ActionResult<ApprovalListData>> {
        Json(approvals::approval_list(&self.default_root, params))
    }

    #[tool(
        title = "Approval Respond",
        description = "Approve or deny one pending approval request.",
        annotations(
            title = "Approval Respond",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn approval_respond(
        &self,
        Parameters(params): Parameters<ApprovalRespondParams>,
    ) -> Json<ActionResult<ApprovalResponseData>> {
        Json(approvals::approval_respond(&self.default_root, params))
    }

    #[tool(
        title = "Acquire Lease",
        description = "Acquire a durable project or task lease.",
        annotations(
            title = "Acquire Lease",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn acquire_lease(
        &self,
        Parameters(params): Parameters<AcquireLeaseParams>,
    ) -> Json<ActionResult<LeaseRecordData>> {
        Json(leases::acquire_lease(&self.default_root, params))
    }

    #[tool(
        title = "List Leases",
        description = "List active or historical project and task leases.",
        annotations(
            title = "List Leases",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn list_leases(
        &self,
        Parameters(params): Parameters<ListLeasesParams>,
    ) -> Json<ActionResult<LeaseListData>> {
        Json(leases::list_leases(&self.default_root, params))
    }

    #[tool(
        title = "Renew Lease",
        description = "Renew an active durable project or task lease.",
        annotations(
            title = "Renew Lease",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn renew_lease(
        &self,
        Parameters(params): Parameters<RenewLeaseParams>,
    ) -> Json<ActionResult<LeaseRecordData>> {
        Json(leases::renew_lease(&self.default_root, params))
    }

    #[tool(
        title = "Release Lease",
        description = "Release an active durable project or task lease.",
        annotations(
            title = "Release Lease",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn release_lease(
        &self,
        Parameters(params): Parameters<ReleaseLeaseParams>,
    ) -> Json<ActionResult<LeaseRecordData>> {
        Json(leases::release_lease(&self.default_root, params))
    }

    #[tool(
        title = "Events Replay",
        description = "Replay bounded project, task, worker, and approval events.",
        annotations(
            title = "Events Replay",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn events_replay(
        &self,
        Parameters(params): Parameters<EventsReplayParams>,
    ) -> Json<ActionResult<EventsReplayData>> {
        Json(events::events_replay(&self.default_root, params))
    }

    #[tool(
        title = "Storage Capability Probe",
        description = "Probe the reference storage backend contract without mutating project runtime state.",
        annotations(
            title = "Storage Capability Probe",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn storage_capability_probe(
        &self,
        Parameters(params): Parameters<StorageCapabilityProbeParams>,
    ) -> Json<ActionResult<StorageCapabilityProbeData>> {
        Json(storage::capability_probe(&self.default_root, params))
    }

    #[tool(
        title = "Record Evidence",
        description = "Record one durable evidence item for a task, finding, or backlog item.",
        annotations(
            title = "Record Evidence",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn record_evidence(
        &self,
        Parameters(params): Parameters<RecordEvidenceParams>,
    ) -> Json<ActionResult<EvidenceRecordData>> {
        Json(evidence::record_evidence(&self.default_root, params))
    }

    #[tool(
        title = "Record Verification Evidence",
        description = "Record verification evidence for a completed task without requiring the caller to remember the evidence kind.",
        annotations(
            title = "Record Verification Evidence",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn record_verification_evidence(
        &self,
        Parameters(params): Parameters<RecordVerificationEvidenceParams>,
    ) -> Json<ActionResult<EvidenceRecordData>> {
        Json(evidence::record_verification_evidence(
            &self.default_root,
            params,
        ))
    }

    #[tool(
        title = "List Evidence",
        description = "List durable evidence records with optional filters.",
        annotations(
            title = "List Evidence",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn list_evidence(
        &self,
        Parameters(params): Parameters<ListEvidenceParams>,
    ) -> Json<ActionResult<EvidenceListData>> {
        Json(evidence::list_evidence(&self.default_root, params))
    }

    #[tool(
        title = "Reconcile Project",
        description = "Compare backlog closure, task state, findings, and evidence for gaps.",
        annotations(
            title = "Reconcile Project",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn reconcile_project(
        &self,
        Parameters(params): Parameters<ReconcileParams>,
    ) -> Json<ActionResult<ReconciliationData>> {
        Json(reconcile::reconcile_project(&self.default_root, params))
    }

    #[tool(
        title = "List Agent Profiles",
        description = "List configured manager and worker agent profiles.",
        annotations(
            title = "List Agent Profiles",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn list_agent_profiles(
        &self,
        Parameters(params): Parameters<AgentProfilesParams>,
    ) -> Json<ActionResult<AgentProfilesData>> {
        Json(config::list_agent_profiles(&self.default_root, params))
    }

    #[tool(
        title = "Configure Agent Profile",
        description = "Create or update one manager or worker agent profile.",
        annotations(
            title = "Configure Agent Profile",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn configure_agent_profile(
        &self,
        Parameters(params): Parameters<ConfigureAgentProfileParams>,
    ) -> Json<ActionResult<AgentProfileData>> {
        Json(config::configure_agent_profile(&self.default_root, params))
    }

    #[tool(
        title = "Inspect Workflow Config",
        description = "Inspect effective workflow integration configuration for the project.",
        annotations(
            title = "Inspect Workflow Config",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        execution(task_support = "forbidden")
    )]
    pub async fn inspect_workflow_config(
        &self,
        Parameters(params): Parameters<WorkflowConfigParams>,
    ) -> Json<ActionResult<WorkflowConfigData>> {
        Json(config::inspect_workflow_config(&self.default_root, params))
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
    ) -> Json<ActionResult<WorkerGuidanceData>> {
        Json(tasks::send_worker_guidance(&self.default_root, params))
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

pub async fn serve_stdio() -> AnyhowResult<()> {
    let server = PlatypusMcp::new().serve(rmcp::transport::stdio()).await?;
    server.waiting().await?;
    Ok(())
}

fn client_supports_sampling(peer: &Peer<RoleServer>) -> bool {
    peer.peer_info()
        .map(|info| info.capabilities.sampling.is_some())
        .unwrap_or(false)
}

async fn sample_text(
    peer: &Peer<RoleServer>,
    system_prompt: &str,
    prompt: String,
    max_tokens: u32,
) -> Result<String, String> {
    let result = peer
        .create_message(CreateMessageRequestParams {
            meta: None,
            task: None,
            messages: vec![SamplingMessage::user_text(prompt)],
            model_preferences: None,
            system_prompt: Some(system_prompt.to_string()),
            include_context: Some(ContextInclusion::ThisServer),
            temperature: Some(0.2),
            max_tokens,
            stop_sequences: None,
            metadata: None,
            tools: None,
            tool_choice: None,
        })
        .await
        .map_err(|error| format!("client sampling failed: {error}"))?;
    result
        .validate()
        .map_err(|error| format!("client sampling response was invalid: {error}"))?;
    crate::sampling::message_text(&result.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::TaskSupport;
    use serde_json::Value;

    #[test]
    fn tool_router_exposes_basic_tool_set() {
        let server = PlatypusMcp::new();
        let names = server.tool_names();
        for expected in [
            "ping",
            "inspect_status",
            "project_status",
            "next_safe_action",
            "inspect_work_queue",
            "classify_planning_needs",
            "classify_workflow_fit",
            "plan_goal_work",
            "start_goal_work",
            "list_backlog",
            "inspect_backlog_inventory",
            "inspect_dependency_graph",
            "validate_backlog",
            "doctor_snapshot",
            "init_project",
            "draft_backlog_items",
            "draft_external_backlog_items",
            "import_github_issues",
            "draft_external_report",
            "request_external_report_approval",
            "request_planning_approval",
            "record_external_report_dispatch",
            "create_backlog_item",
            "create_backlog_items",
            "create_epic",
            "list_epics",
            "draft_task_plan",
            "inspect_task_plan",
            "list_task_plans",
            "validate_task_plan",
            "write_task_plan",
            "dispatch_next_work",
            "dispatch_ready_work",
            "prepare_work",
            "inspect_task",
            "claim_next_task",
            "worktree_create",
            "worktree_status",
            "worktree_diff",
            "inspect_worktree_changes",
            "worktree_cleanup",
            "integrate_worker_result",
            "generate_task_bundle",
            "prepare_worker_assignment",
            "prepare_worker_handoff",
            "inspect_worker_assignment",
            "start_worker_execution",
            "start_worker_task",
            "record_worker_event",
            "record_worker_progress",
            "complete_worker_execution",
            "complete_worker_task",
            "finish_work",
            "run_task_verification",
            "runner_prepare_next",
            "inspect_task_events",
            "approval_list",
            "approval_respond",
            "acquire_lease",
            "list_leases",
            "renew_lease",
            "release_lease",
            "events_replay",
            "storage_capability_probe",
            "record_evidence",
            "record_verification_evidence",
            "list_evidence",
            "reconcile_project",
            "list_agent_profiles",
            "configure_agent_profile",
            "inspect_workflow_config",
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
            "create_backlog_items",
            "create_epic",
            "write_task_plan",
            "import_github_issues",
            "request_external_report_approval",
            "request_planning_approval",
            "record_external_report_dispatch",
            "start_goal_work",
            "dispatch_next_work",
            "dispatch_ready_work",
            "prepare_work",
            "claim_next_task",
            "worktree_create",
            "worktree_cleanup",
            "integrate_worker_result",
            "prepare_worker_assignment",
            "prepare_worker_handoff",
            "start_worker_execution",
            "start_worker_task",
            "record_worker_event",
            "record_worker_progress",
            "complete_worker_execution",
            "complete_worker_task",
            "finish_work",
            "run_task_verification",
            "runner_prepare_next",
            "approval_respond",
            "acquire_lease",
            "renew_lease",
            "release_lease",
            "record_evidence",
            "record_verification_evidence",
            "configure_agent_profile",
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
                Some(matches!(name, "init_project" | "worktree_cleanup")),
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

    #[test]
    fn nested_tool_inputs_are_advertised_as_structured_objects() {
        let server = PlatypusMcp::new();
        let tools = server.tool_router.list_all();

        assert_array_items_are_objects(&tools, "draft_external_backlog_items", "records");
        assert_array_items_are_objects(&tools, "import_github_issues", "issues");
        assert_array_items_are_objects(&tools, "create_backlog_item", "external_refs");
        assert_array_items_are_objects(&tools, "create_backlog_items", "items");
        assert_array_items_are_objects(&tools, "finish_work", "findings");
        assert_property_is_object(&tools, "request_external_report_approval", "draft");
        assert_property_is_object(&tools, "write_task_plan", "plan");
    }

    #[test]
    fn tool_schemas_do_not_emit_nullable_without_type() {
        let server = PlatypusMcp::new();
        let tools = server.tool_router.list_all();

        for tool in tools {
            let input = serde_json::to_value(tool.input_schema.as_ref()).expect("input schema");
            assert_no_nullable_without_type(&input, &format!("{}.inputSchema", tool.name));

            if let Some(output_schema) = tool.output_schema.as_ref() {
                let output = serde_json::to_value(output_schema.as_ref()).expect("output schema");
                assert_no_nullable_without_type(&output, &format!("{}.outputSchema", tool.name));
            }
        }
    }

    #[test]
    fn tool_schemas_do_not_emit_rust_unsigned_integer_formats() {
        let server = PlatypusMcp::new();
        let tools = server.tool_router.list_all();

        for tool in tools {
            let input = serde_json::to_value(tool.input_schema.as_ref()).expect("input schema");
            assert_no_rust_unsigned_integer_formats(&input, &format!("{}.inputSchema", tool.name));

            if let Some(output_schema) = tool.output_schema.as_ref() {
                let output = serde_json::to_value(output_schema.as_ref()).expect("output schema");
                assert_no_rust_unsigned_integer_formats(
                    &output,
                    &format!("{}.outputSchema", tool.name),
                );
            }
        }
    }

    #[test]
    fn tool_schemas_have_human_field_descriptions() {
        let server = PlatypusMcp::new();
        let tools = server.tool_router.list_all();

        for tool in tools {
            let input = serde_json::to_value(tool.input_schema.as_ref()).expect("input schema");
            assert_schema_properties_have_descriptions(
                &input,
                &format!("{}.inputSchema", tool.name),
            );

            if let Some(output_schema) = tool.output_schema.as_ref() {
                let output = serde_json::to_value(output_schema.as_ref()).expect("output schema");
                assert_schema_properties_have_descriptions(
                    &output,
                    &format!("{}.outputSchema", tool.name),
                );
            }
        }
    }

    #[test]
    fn tool_input_schemas_expose_enum_and_bound_constraints() {
        let server = PlatypusMcp::new();
        let tools = server.tool_router.list_all();

        assert_property_enum_values(
            &input_schema(&tools, "create_backlog_item"),
            "priority",
            &["P0", "P1", "P2"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "create_backlog_item"),
            "type",
            &["foundation", "feature", "safety", "ux", "test", "docs"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "create_epic"),
            "status",
            &["active", "archived"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "create_epic"),
            "priority",
            &["P0", "P1", "P2"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "start_goal_work"),
            "mode",
            &["auto", "direct_scaffold", "hybrid", "platypus_workflow"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "plan_goal_work"),
            "mode",
            &["auto", "direct_scaffold", "hybrid", "platypus_workflow"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "plan_goal_work"),
            "intent",
            &["auto", "planning_only", "ready_to_execute"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "approval_respond"),
            "decision",
            &["approve", "deny"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "complete_worker_task"),
            "status",
            &["completed", "failed", "cancelled"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "complete_worker_task"),
            "verification_status",
            &["passed", "failed", "skipped", "not_run"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "integrate_worker_result"),
            "strategy",
            &[
                "merge_commit",
                "fast_forward",
                "squash",
                "apply_changed_files",
            ],
        );
        assert_property_enum_values(
            &input_schema(&tools, "configure_agent_profile"),
            "role",
            &["manager", "worker"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "configure_agent_profile"),
            "harness",
            &["codex", "claude", "fake", "custom"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "record_evidence"),
            "kind",
            &[
                "commit",
                "verification",
                "file_summary",
                "worker_finding",
                "manager_disposition",
                "external_report",
                "note",
            ],
        );
        assert_property_enum_values(
            &input_schema(&tools, "update_finding_disposition"),
            "status",
            &[
                "open",
                "accepted",
                "resolved",
                "rejected",
                "deferred",
                "duplicate",
            ],
        );
        assert_property_numeric_bounds(
            &input_schema(&tools, "dispatch_ready_work"),
            "max_tasks",
            1,
            10,
        );
        assert_property_enum_values(
            &input_schema(&tools, "prepare_work"),
            "execution_mode",
            &["auto", "profiled_worker", "manual_handoff"],
        );
        assert_property_numeric_bounds(&input_schema(&tools, "prepare_work"), "max_tasks", 1, 10);
        assert_property_enum_values(
            &input_schema(&tools, "finish_work"),
            "status",
            &["completed", "failed", "cancelled"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "finish_work"),
            "verification_status",
            &["passed", "failed", "skipped", "not_run"],
        );
        assert_property_enum_values(
            &input_schema(&tools, "finish_work"),
            "integration_strategy",
            &[
                "merge_commit",
                "fast_forward",
                "squash",
                "apply_changed_files",
            ],
        );
        assert_property_numeric_bounds(&input_schema(&tools, "approval_list"), "limit", 1, 200);
        assert_property_numeric_bounds(
            &input_schema(&tools, "acquire_lease"),
            "ttl_seconds",
            1,
            86400,
        );
        assert_property_numeric_bounds(
            &input_schema(&tools, "run_task_verification"),
            "timeout_seconds",
            1,
            600,
        );
    }

    #[test]
    fn tool_input_schemas_expose_examples_and_patterns() {
        let server = PlatypusMcp::new();
        let tools = server.tool_router.list_all();

        let create_backlog_item = input_schema(&tools, "create_backlog_item");
        assert_property_has_example(&create_backlog_item, "title");
        assert_property_has_example(&create_backlog_item, "goal");
        assert_property_pattern(&create_backlog_item, "id", r"^[A-Z]+-[0-9]{3}$");
        assert_property_pattern(&create_backlog_item, "id_prefix", r"^[A-Z]+$");
        assert_array_item_pattern(&create_backlog_item, "depends_on", r"^[A-Z]+-[0-9]{3}$");

        let create_backlog_items = input_schema(&tools, "create_backlog_items");
        assert_property_pattern(&create_backlog_items, "id_prefix", r"^[A-Z]+$");

        let create_epic = input_schema(&tools, "create_epic");
        assert_property_has_example(&create_epic, "id");
        assert_property_pattern(&create_epic, "id", r"^[A-Za-z0-9_-]+$");

        let start_goal_work = input_schema(&tools, "start_goal_work");
        assert_property_has_example(&start_goal_work, "goal");
        assert_property_has_example(&start_goal_work, "owned_surfaces");
        assert_property_has_example(&start_goal_work, "verification_command");
        assert!(
            start_goal_work
                .get("properties")
                .and_then(Value::as_object)
                .map(|properties| !properties.contains_key("scaffold_in_place"))
                .unwrap_or(true),
            "start_goal_work schema must not advertise removed scaffold_in_place flag"
        );

        let plan_goal_work = input_schema(&tools, "plan_goal_work");
        assert_property_has_example(&plan_goal_work, "goal");
        assert_property_has_example(&plan_goal_work, "owned_surfaces");

        let record_worker_event = input_schema(&tools, "record_worker_event");
        assert_property_has_example(&record_worker_event, "assignment_id");
        assert_property_has_example(&record_worker_event, "summary");
        assert_property_pattern(
            &record_worker_event,
            "event_type",
            r"^[A-Za-z0-9_.:-]{1,80}$",
        );

        let inspect_task_events = input_schema(&tools, "inspect_task_events");
        assert_property_has_example(&inspect_task_events, "task_id");
        assert_property_numeric_bounds(&inspect_task_events, "limit", 1, 200);

        let prepare_work = input_schema(&tools, "prepare_work");
        assert_property_has_example(&prepare_work, "item_id");
        assert_property_has_example(&prepare_work, "worker");
        assert_property_has_example(&prepare_work, "verification_command");

        let finish_work = input_schema(&tools, "finish_work");
        assert_property_has_example(&finish_work, "assignment_id");
        assert_property_has_example(&finish_work, "task_id");
        assert_property_has_example(&finish_work, "summary");
    }

    fn assert_array_items_are_objects(
        tools: &[rmcp::model::Tool],
        tool_name: &str,
        property_name: &str,
    ) {
        let schema = input_schema(tools, tool_name);
        let property = property_schema(&schema, property_name);
        assert_eq!(
            property["type"], "array",
            "{tool_name}.{property_name} should be an array: {property:#}"
        );
        let items = &property["items"];
        assert!(
            has_object_type(items),
            "{tool_name}.{property_name} items should be inline objects, got: {items:#}"
        );
    }

    fn assert_property_is_object(
        tools: &[rmcp::model::Tool],
        tool_name: &str,
        property_name: &str,
    ) {
        let schema = input_schema(tools, tool_name);
        let property = property_schema(&schema, property_name);
        assert!(
            has_object_type(property),
            "{tool_name}.{property_name} should be an inline object, got: {property:#}"
        );
    }

    fn input_schema(tools: &[rmcp::model::Tool], tool_name: &str) -> Value {
        let tool = tools
            .iter()
            .find(|tool| tool.name == tool_name)
            .unwrap_or_else(|| panic!("{tool_name} missing"));
        serde_json::to_value(tool.input_schema.as_ref()).expect("schema json")
    }

    fn property_schema<'a>(schema: &'a Value, property_name: &str) -> &'a Value {
        schema
            .get("properties")
            .and_then(|properties| properties.get(property_name))
            .unwrap_or_else(|| panic!("{property_name} missing in schema: {schema:#}"))
    }

    fn has_object_type(schema: &Value) -> bool {
        match schema.get("type") {
            Some(Value::String(kind)) => kind == "object",
            Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind == "object"),
            _ => false,
        }
    }

    fn assert_no_nullable_without_type(schema: &Value, path: &str) {
        match schema {
            Value::Object(object) => {
                assert!(
                    !(matches!(object.get("nullable"), Some(Value::Bool(true)))
                        && !object.contains_key("type")),
                    "{path} contains nullable without type: {object:#?}"
                );
                for (key, value) in object {
                    assert_no_nullable_without_type(value, &format!("{path}.{key}"));
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter().enumerate() {
                    assert_no_nullable_without_type(value, &format!("{path}[{index}]"));
                }
            }
            _ => {}
        }
    }

    fn assert_no_rust_unsigned_integer_formats(schema: &Value, path: &str) {
        match schema {
            Value::Object(object) => {
                assert!(
                    !(object.get("type").and_then(Value::as_str) == Some("integer")
                        && object
                            .get("format")
                            .and_then(Value::as_str)
                            .is_some_and(is_rust_unsigned_integer_format)),
                    "{path} contains Rust unsigned integer format: {object:#?}"
                );
                for (key, value) in object {
                    assert_no_rust_unsigned_integer_formats(value, &format!("{path}.{key}"));
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter().enumerate() {
                    assert_no_rust_unsigned_integer_formats(value, &format!("{path}[{index}]"));
                }
            }
            _ => {}
        }
    }

    fn assert_schema_properties_have_descriptions(schema: &Value, path: &str) {
        match schema {
            Value::Object(object) => {
                if let Some(Value::Object(properties)) = object.get("properties") {
                    for (property_name, property_schema) in properties {
                        let property_path = format!("{path}.properties.{property_name}");
                        let description = property_schema
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_else(|| {
                                panic!("{property_path} missing description: {property_schema:#}")
                            });
                        assert_human_description(description, &property_path);
                    }
                }
                for (key, value) in object {
                    assert_schema_properties_have_descriptions(value, &format!("{path}.{key}"));
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter().enumerate() {
                    assert_schema_properties_have_descriptions(value, &format!("{path}[{index}]"));
                }
            }
            _ => {}
        }
    }

    fn assert_human_description(description: &str, path: &str) {
        let disallowed = [
            "Value for ",
            "Project root for this request or result.",
            "Status value for this record or operation.",
            "Human-readable summary.",
            "Reason or rationale for this result.",
            "Kind or category value.",
            "Record identifier.",
        ];
        for prefix in disallowed {
            assert!(
                !description.starts_with(prefix),
                "{path} has placeholder description: {description}"
            );
        }
    }

    fn assert_property_enum_values(schema: &Value, property_name: &str, expected: &[&str]) {
        let property = property_schema(schema, property_name);
        let mut actual = Vec::new();
        collect_enum_values(schema, property, &mut actual);
        actual.sort();
        actual.dedup();

        let mut expected = expected
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(
            actual, expected,
            "{property_name} enum values did not match in schema: {property:#}"
        );
    }

    fn assert_property_numeric_bounds(
        schema: &Value,
        property_name: &str,
        expected_minimum: i64,
        expected_maximum: i64,
    ) {
        let property = property_schema(schema, property_name);
        let mut bounds = Vec::new();
        collect_numeric_bounds(schema, property, &mut bounds);
        assert!(
            bounds
                .iter()
                .any(|(minimum, maximum)| *minimum == Some(expected_minimum)
                    && *maximum == Some(expected_maximum)),
            "{property_name} missing numeric bounds {expected_minimum}..{expected_maximum}: {property:#}"
        );
    }

    fn assert_property_has_example(schema: &Value, property_name: &str) {
        let property = property_schema(schema, property_name);
        let mut examples = Vec::new();
        collect_examples(schema, property, &mut examples);
        assert!(
            !examples.is_empty(),
            "{property_name} missing example in schema: {property:#}"
        );
    }

    fn assert_property_pattern(schema: &Value, property_name: &str, expected_pattern: &str) {
        let property = property_schema(schema, property_name);
        let mut patterns = Vec::new();
        collect_patterns(schema, property, &mut patterns);
        assert!(
            patterns.iter().any(|pattern| pattern == expected_pattern),
            "{property_name} missing pattern {expected_pattern}: {property:#}"
        );
    }

    fn assert_array_item_pattern(schema: &Value, property_name: &str, expected_pattern: &str) {
        let property = property_schema(schema, property_name);
        let Some(items) = property.get("items") else {
            panic!("{property_name} missing items schema: {property:#}");
        };
        let mut patterns = Vec::new();
        collect_patterns(schema, items, &mut patterns);
        assert!(
            patterns.iter().any(|pattern| pattern == expected_pattern),
            "{property_name} items missing pattern {expected_pattern}: {property:#}"
        );
    }

    fn collect_enum_values(root: &Value, schema: &Value, values: &mut Vec<String>) {
        match schema {
            Value::Object(object) => {
                if let Some(Value::String(reference)) = object.get("$ref") {
                    if let Some(resolved) = resolve_schema_ref(root, reference) {
                        collect_enum_values(root, resolved, values);
                    }
                }
                if let Some(Value::Array(enums)) = object.get("enum") {
                    values.extend(enums.iter().filter_map(Value::as_str).map(str::to_string));
                }
                for key in ["anyOf", "oneOf", "allOf"] {
                    if let Some(Value::Array(items)) = object.get(key) {
                        for item in items {
                            collect_enum_values(root, item, values);
                        }
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect_enum_values(root, item, values);
                }
            }
            _ => {}
        }
    }

    fn collect_examples(root: &Value, schema: &Value, examples: &mut Vec<Value>) {
        match schema {
            Value::Object(object) => {
                if let Some(Value::String(reference)) = object.get("$ref") {
                    if let Some(resolved) = resolve_schema_ref(root, reference) {
                        collect_examples(root, resolved, examples);
                    }
                }
                if let Some(Value::Array(values)) = object.get("examples") {
                    examples.extend(values.iter().cloned());
                }
                if let Some(value) = object.get("example") {
                    examples.push(value.clone());
                }
                for key in ["anyOf", "oneOf", "allOf"] {
                    if let Some(Value::Array(items)) = object.get(key) {
                        for item in items {
                            collect_examples(root, item, examples);
                        }
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect_examples(root, item, examples);
                }
            }
            _ => {}
        }
    }

    fn collect_patterns(root: &Value, schema: &Value, patterns: &mut Vec<String>) {
        match schema {
            Value::Object(object) => {
                if let Some(Value::String(reference)) = object.get("$ref") {
                    if let Some(resolved) = resolve_schema_ref(root, reference) {
                        collect_patterns(root, resolved, patterns);
                    }
                }
                if let Some(Value::String(pattern)) = object.get("pattern") {
                    patterns.push(pattern.to_string());
                }
                for key in ["anyOf", "oneOf", "allOf"] {
                    if let Some(Value::Array(items)) = object.get(key) {
                        for item in items {
                            collect_patterns(root, item, patterns);
                        }
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect_patterns(root, item, patterns);
                }
            }
            _ => {}
        }
    }

    fn collect_numeric_bounds(
        root: &Value,
        schema: &Value,
        bounds: &mut Vec<(Option<i64>, Option<i64>)>,
    ) {
        match schema {
            Value::Object(object) => {
                if let Some(Value::String(reference)) = object.get("$ref") {
                    if let Some(resolved) = resolve_schema_ref(root, reference) {
                        collect_numeric_bounds(root, resolved, bounds);
                    }
                }
                let minimum = object.get("minimum").and_then(Value::as_i64);
                let maximum = object.get("maximum").and_then(Value::as_i64);
                if minimum.is_some() || maximum.is_some() {
                    bounds.push((minimum, maximum));
                }
                for key in ["anyOf", "oneOf", "allOf"] {
                    if let Some(Value::Array(items)) = object.get(key) {
                        for item in items {
                            collect_numeric_bounds(root, item, bounds);
                        }
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect_numeric_bounds(root, item, bounds);
                }
            }
            _ => {}
        }
    }

    fn resolve_schema_ref<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
        reference.strip_prefix("#/").and_then(|path| {
            path.split('/')
                .try_fold(root, |value, part| value.get(part))
        })
    }
}
