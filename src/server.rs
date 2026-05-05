use crate::{
    backlog,
    models::{
        ActionResult, CreateBacklogItemParams, CreatedBacklogItemData, DraftBacklogData,
        DraftBacklogItemsParams, InspectTaskEventsParams, LimitParams, ListFindingsParams,
        PingData, PingParams, ProjectStatusData, RootParams, SendWorkerGuidanceParams,
        UnsupportedData, UpdateFindingDispositionParams, ValidateBacklogParams,
        ValidateFindingsParams,
    },
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
    #[tool(description = "Health check for the Platypus MCP server.")]
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

    #[tool(description = "Inspect current project and backlog status.")]
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

    #[tool(description = "Compatibility alias for inspect_status.")]
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

    #[tool(description = "List runnable backlog candidates.")]
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

    #[tool(description = "Validate structured Platypus backlog files.")]
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

    #[tool(description = "Draft typed backlog candidates from a product goal.")]
    pub async fn draft_backlog_items(
        &self,
        Parameters(params): Parameters<DraftBacklogItemsParams>,
    ) -> Json<ActionResult<DraftBacklogData>> {
        Json(backlog::draft_backlog_items(params))
    }

    #[tool(description = "Create one structured backlog item.")]
    pub async fn create_backlog_item(
        &self,
        Parameters(params): Parameters<CreateBacklogItemParams>,
    ) -> Json<ActionResult<CreatedBacklogItemData>> {
        Json(backlog::create_backlog_item(&self.default_root, params))
    }

    #[tool(description = "Dispatch the next runnable backlog item.")]
    pub async fn dispatch_next_work(
        &self,
        Parameters(params): Parameters<RootParams>,
    ) -> Json<ActionResult<UnsupportedData>> {
        Json(not_implemented(
            "dispatch_next_work",
            params.root.as_deref(),
            "Worker runtime dispatch is not wired into the Rust MCP server yet.",
        ))
    }

    #[tool(description = "Inspect recent task supervision events.")]
    pub async fn inspect_task_events(
        &self,
        Parameters(params): Parameters<InspectTaskEventsParams>,
    ) -> Json<ActionResult<UnsupportedData>> {
        Json(not_implemented(
            "inspect_task_events",
            params.root.as_deref(),
            &format!(
                "Task event storage is not wired yet for task `{}`.",
                params.task_id
            ),
        ))
    }

    #[tool(description = "Send guidance to a running worker task.")]
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

    #[tool(description = "List implementation findings awaiting disposition.")]
    pub async fn list_findings(
        &self,
        Parameters(params): Parameters<ListFindingsParams>,
    ) -> Json<ActionResult<UnsupportedData>> {
        Json(not_implemented(
            "list_findings",
            params.root.as_deref(),
            "Finding storage is not wired into the Rust MCP server yet.",
        ))
    }

    #[tool(description = "Validate finding dispositions.")]
    pub async fn validate_findings(
        &self,
        Parameters(params): Parameters<ValidateFindingsParams>,
    ) -> Json<ActionResult<UnsupportedData>> {
        Json(not_implemented(
            "validate_findings",
            params.root.as_deref(),
            "Finding validation is not wired into the Rust MCP server yet.",
        ))
    }

    #[tool(description = "Persist a manager disposition for one finding.")]
    pub async fn update_finding_disposition(
        &self,
        Parameters(params): Parameters<UpdateFindingDispositionParams>,
    ) -> Json<ActionResult<UnsupportedData>> {
        Json(not_implemented(
            "update_finding_disposition",
            params.root.as_deref(),
            &format!(
                "Finding disposition storage is not wired yet for `{}`.",
                params.finding_id
            ),
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
            "draft_backlog_items",
            "create_backlog_item",
            "dispatch_next_work",
            "inspect_task_events",
            "send_worker_guidance",
            "list_findings",
            "validate_findings",
            "update_finding_disposition",
        ] {
            assert!(names.contains(expected), "missing tool {expected}");
        }
    }
}
