use crate::{
    backlog,
    events::{self, NewEvent},
    models::{
        ActionResult, ActionStatus, ApprovalListData, ApprovalListParams, ApprovalRecord,
        ApprovalRespondParams, ApprovalResponseData, PlanningApprovalData, PlanningApprovalState,
        RequestPlanningApprovalParams,
    },
    storage::{self, ApprovalInsert},
    storage::{ApprovalStore, RepositoryError},
};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const PLANNING_APPROVAL_SCOPE: &str = "planning";

#[derive(Debug, Clone)]
pub struct NewApproval {
    pub scope: String,
    pub title: String,
    pub summary: String,
    pub requested_by: Option<String>,
    pub metadata: BTreeMap<String, Value>,
}

pub fn create_approval(
    default_root: &Path,
    root: Option<&str>,
    approval: NewApproval,
) -> Result<ApprovalRecord, String> {
    let storage = storage::connect(default_root, root).map_err(|error| error.to_string())?;
    let scope = clean_scope(&approval.scope)?;
    let title = clean_required("title", &approval.title)?;
    let summary = clean_required("summary", &approval.summary)?;
    let requested_by = clean_optional(approval.requested_by);
    let record = storage
        .repository()
        .approvals()
        .create(ApprovalInsert {
            scope,
            title,
            summary,
            requested_by,
            metadata: approval.metadata,
        })
        .map_err(|error| error.to_string())?;
    let _ = events::record_event(
        default_root,
        root,
        NewEvent {
            event_type: "approval_requested".to_string(),
            scope: "approval".to_string(),
            task_id: None,
            summary: format!("Approval `{}` requested.", record.id),
            payload: Some(serde_json::json!({
                "approval_id": record.id,
                "approval_scope": record.scope,
                "title": record.title
            })),
        },
    )?;
    Ok(record)
}

pub fn approval_list(
    default_root: &Path,
    params: ApprovalListParams,
) -> ActionResult<ApprovalListData> {
    let action = "approval_list";
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open approval storage.",
                error.to_string(),
            )
        }
    };
    let status = params
        .status
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(status) = status {
        if !valid_status(status) {
            return ActionResult::failed(
                action,
                "Could not list approvals.",
                "invalid approval status",
            );
        }
    }
    let limit = bounded_limit(params.limit);
    let approvals = match storage.repository().approvals().list(status, limit) {
        Ok(approvals) => approvals,
        Err(error) => {
            return ActionResult::failed(action, "Could not list approvals.", error.to_string())
        }
    };
    let returned = approvals.len();
    let data = ApprovalListData {
        root: storage.storage.root.display().to_string(),
        approvals,
        returned,
    };
    if returned == 0 {
        ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No approvals matched the filters.".to_string(),
            next_action: Some("Request an approval before listing pending approvals.".to_string()),
            recovery_action: None,
            data: Some(data),
            error: None,
        }
    } else {
        ActionResult::completed(action, format!("Returned {returned} approval(s)."), data)
    }
}

pub fn approval_respond(
    default_root: &Path,
    params: ApprovalRespondParams,
) -> ActionResult<ApprovalResponseData> {
    let action = "approval_respond";
    let approval_id = params.approval_id.trim();
    if approval_id.is_empty() {
        return ActionResult::failed(
            action,
            "Could not respond to approval.",
            "approval_id is required",
        );
    }
    let response = match clean_decision(&params.decision) {
        Ok(response) => response,
        Err(error) => return ActionResult::failed(action, "Could not respond to approval.", error),
    };
    let responder = clean_optional(params.responder).unwrap_or_else(|| "user".to_string());
    let reason = clean_optional(params.reason);
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open approval storage.",
                error.to_string(),
            )
        }
    };
    let approvals = storage.repository().approvals();
    let existing = match approvals.get(approval_id) {
        Ok(existing) => existing,
        Err(RepositoryError::NotFound) => {
            return ActionResult::skipped(
                action,
                format!("Approval `{approval_id}` was not found."),
                "List pending approvals and retry with a valid approval id.",
            )
        }
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect approval.", error.to_string())
        }
    };
    if existing.status != "pending" {
        return ActionResult::skipped(
            action,
            format!("Approval `{approval_id}` is already {}.", existing.status),
            "List pending approvals before responding.",
        );
    }
    let status = if response == "approved" {
        "approved"
    } else {
        "denied"
    };
    if let Err(error) = approvals.respond(
        approval_id,
        status,
        &response,
        &responder,
        reason.as_deref(),
    ) {
        return ActionResult::failed(action, "Could not update approval.", error.to_string());
    }
    let record = match approvals.get(approval_id) {
        Ok(record) => record,
        Err(error) => {
            return ActionResult::failed(action, "Could not reload approval.", error.to_string())
        }
    };
    if let Err(error) = events::record_event(
        default_root,
        params.root.as_deref(),
        NewEvent {
            event_type: "approval_responded".to_string(),
            scope: "approval".to_string(),
            task_id: None,
            summary: format!("Approval `{}` {}.", record.id, record.status),
            payload: Some(serde_json::json!({
                "approval_id": record.id,
                "status": record.status,
                "responder": record.responder
            })),
        },
    ) {
        return ActionResult::failed(action, "Could not record approval event.", error);
    }
    ActionResult::completed(
        action,
        format!("Approval `{}` {}.", record.id, record.status),
        ApprovalResponseData { approval: record },
    )
}

pub fn request_planning_approval(
    default_root: &Path,
    params: RequestPlanningApprovalParams,
) -> ActionResult<PlanningApprovalData> {
    let action = "request_planning_approval";
    let item_ids = match clean_item_ids(params.item_ids) {
        Ok(item_ids) => item_ids,
        Err(error) => {
            return ActionResult::failed(action, "Could not request planning approval.", error)
        }
    };
    for item_id in &item_ids {
        if let Err(error) =
            backlog::backlog_item_snapshot(default_root, params.root.as_deref(), item_id)
        {
            return ActionResult::failed(
                action,
                "Could not request planning approval.",
                format!("unknown backlog item `{item_id}`: {error}"),
            );
        }
    }
    let root = match backlog::resolve_backlog_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(action, "Could not request planning approval.", error)
        }
    };
    let summary = clean_optional(params.summary).unwrap_or_else(|| {
        format!(
            "Approve planning before dispatch for backlog item(s): {}.",
            item_ids.join(", ")
        )
    });
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "item_ids".to_string(),
        Value::Array(item_ids.iter().cloned().map(Value::String).collect()),
    );
    metadata.insert(
        "approval_kind".to_string(),
        Value::String(if item_ids.len() == 1 {
            "task_plan".to_string()
        } else {
            "backlog_tranche".to_string()
        }),
    );
    let approval = match create_approval(
        default_root,
        params.root.as_deref(),
        NewApproval {
            scope: PLANNING_APPROVAL_SCOPE.to_string(),
            title: planning_approval_title(&item_ids),
            summary,
            requested_by: clean_optional(params.requested_by),
            metadata,
        },
    ) {
        Ok(approval) => approval,
        Err(error) => {
            return ActionResult::failed(action, "Could not request planning approval.", error)
        }
    };
    let states = item_ids
        .iter()
        .map(|item_id| planning_state_from_record(item_id, true, Some(&approval)))
        .collect::<Vec<_>>();
    ActionResult::completed(
        action,
        format!("Planning approval `{}` requested.", approval.id),
        PlanningApprovalData {
            root: root.display().to_string(),
            approval,
            states,
        },
    )
}

pub fn planning_approval_state(
    default_root: &Path,
    root: Option<&str>,
    item_id: &str,
    required: bool,
) -> Result<PlanningApprovalState, String> {
    let approval = latest_planning_approval_for_item(default_root, root, item_id)?;
    Ok(planning_state_from_record(
        item_id,
        required,
        approval.as_ref(),
    ))
}

pub fn planning_approval_is_approved(
    default_root: &Path,
    root: Option<&str>,
    item_id: &str,
) -> Result<bool, String> {
    Ok(planning_approval_state(default_root, root, item_id, true)?.approved)
}

pub(crate) fn approval_covers_item(approval: &ApprovalRecord, item_id: &str) -> bool {
    approval
        .metadata
        .get("item_ids")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|value| value.as_str() == Some(item_id)))
}

fn clean_scope(value: &str) -> Result<String, String> {
    let scope = clean_required("scope", value)?;
    if scope
        .chars()
        .all(|character| character.is_ascii_lowercase() || character == '_' || character == '-')
    {
        Ok(scope)
    } else {
        Err("scope must contain lowercase ASCII letters, '-' or '_'".to_string())
    }
}

fn latest_planning_approval_for_item(
    default_root: &Path,
    root: Option<&str>,
    item_id: &str,
) -> Result<Option<ApprovalRecord>, String> {
    let Some(storage) = storage::connect_existing_read_only(default_root, root)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let approvals = storage
        .repository()
        .approvals()
        .list_newest_unbounded(None)
        .map_err(|error| error.to_string())?;
    Ok(approvals.into_iter().find(|approval| {
        approval.scope == PLANNING_APPROVAL_SCOPE && approval_covers_item(approval, item_id)
    }))
}

fn planning_state_from_record(
    item_id: &str,
    required: bool,
    approval: Option<&ApprovalRecord>,
) -> PlanningApprovalState {
    let approved = approval.is_some_and(|approval| approval.status == "approved");
    let reason = match approval {
        Some(approval) if approval.status == "approved" => {
            format!("Planning approval `{}` is approved.", approval.id)
        }
        Some(approval) if approval.status == "pending" => {
            format!("Planning approval `{}` is still pending.", approval.id)
        }
        Some(approval) if approval.status == "denied" => {
            format!("Planning approval `{}` was denied.", approval.id)
        }
        Some(approval) => format!(
            "Planning approval `{}` has unsupported status `{}`.",
            approval.id, approval.status
        ),
        None if required => {
            "Planning approval is required before dispatch and has not been requested.".to_string()
        }
        None => "Planning approval is not required.".to_string(),
    };
    PlanningApprovalState {
        item_id: item_id.to_string(),
        required,
        approved,
        approval_id: approval.map(|approval| approval.id.clone()),
        status: approval.map(|approval| approval.status.clone()),
        reason,
    }
}

fn clean_item_ids(item_ids: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut cleaned = Vec::new();
    for item_id in item_ids {
        let item_id = item_id.trim();
        if item_id.is_empty() {
            continue;
        }
        if !seen.insert(item_id.to_string()) {
            continue;
        }
        cleaned.push(item_id.to_string());
    }
    if cleaned.is_empty() {
        Err("item_ids must include at least one backlog item id".to_string())
    } else {
        Ok(cleaned)
    }
}

fn planning_approval_title(item_ids: &[String]) -> String {
    if item_ids.len() == 1 {
        format!("Approve planning for {}", item_ids[0])
    } else {
        format!("Approve planning for {} backlog items", item_ids.len())
    }
}

fn clean_decision(value: &str) -> Result<String, String> {
    match value.trim() {
        "approve" | "approved" => Ok("approved".to_string()),
        "deny" | "denied" => Ok("denied".to_string()),
        _ => Err("decision must be approve or deny".to_string()),
    }
}

fn valid_status(value: &str) -> bool {
    matches!(value, "pending" | "approved" | "denied")
}

fn clean_required(field: &str, value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(trimmed.to_string())
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn bounded_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        events::events_replay,
        models::{ApprovalRespondParams, EventsReplayParams, RequestPlanningApprovalParams},
    };
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn creates_lists_and_approves_pending_approval() {
        let project = TempDir::new().expect("temp dir");
        let created = create_approval(
            project.path(),
            None,
            NewApproval {
                scope: "tool".to_string(),
                title: "Create backlog item".to_string(),
                summary: "Allow create_backlog_item.".to_string(),
                requested_by: Some("manager".to_string()),
                metadata: BTreeMap::new(),
            },
        )
        .expect("approval");

        let listed = approval_list(
            project.path(),
            ApprovalListParams {
                root: None,
                status: Some("pending".to_string()),
                limit: None,
            },
        );
        let listed_data = listed.data.expect("listed");

        assert!(matches!(listed.status, ActionStatus::Completed));
        assert_eq!(listed_data.returned, 1);
        assert_eq!(listed_data.approvals[0].id, created.id);

        let approved = approval_respond(
            project.path(),
            ApprovalRespondParams {
                root: None,
                approval_id: created.id.clone(),
                decision: "approve".to_string(),
                responder: Some("user".to_string()),
                reason: Some("trusted session".to_string()),
            },
        );
        let approval = approved.data.expect("approved").approval;

        assert!(matches!(approved.status, ActionStatus::Completed));
        assert_eq!(approval.status, "approved");
        assert_eq!(approval.response.as_deref(), Some("approved"));
        assert_eq!(approval.responder.as_deref(), Some("user"));
        assert!(approval.responded_at.is_some());

        let replayed = events_replay(
            project.path(),
            EventsReplayParams {
                root: None,
                task_id: None,
                scope: Some("approval".to_string()),
                limit: None,
            },
        );
        let events = replayed.data.expect("events").events;
        assert_eq!(events.len(), 4);
        assert!(events
            .iter()
            .any(|event| event.event_type == "approval_requested"));
        assert!(events
            .iter()
            .any(|event| event.event_type == "approval_responded"));
        assert!(events
            .iter()
            .any(|event| event.event_type == "approval_approved"));
    }

    #[test]
    fn denies_pending_approval() {
        let project = TempDir::new().expect("temp dir");
        let created = create_approval(
            project.path(),
            None,
            NewApproval {
                scope: "tool".to_string(),
                title: "Dangerous tool".to_string(),
                summary: "Allow tool.".to_string(),
                requested_by: None,
                metadata: BTreeMap::new(),
            },
        )
        .expect("approval");

        let denied = approval_respond(
            project.path(),
            ApprovalRespondParams {
                root: None,
                approval_id: created.id,
                decision: "deny".to_string(),
                responder: None,
                reason: None,
            },
        );
        let approval = denied.data.expect("denied").approval;

        assert_eq!(approval.status, "denied");
        assert_eq!(approval.response.as_deref(), Some("denied"));
    }

    #[test]
    fn missing_approval_response_is_skipped() {
        let project = TempDir::new().expect("temp dir");

        let result = approval_respond(
            project.path(),
            ApprovalRespondParams {
                root: None,
                approval_id: "APR-999".to_string(),
                decision: "approve".to_string(),
                responder: None,
                reason: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Skipped));
    }

    #[test]
    fn planning_approval_is_stored_outside_backlog_markdown() {
        let project = backlog_project();
        let item_path = project.path().join("backlog/items/PROJ-001.md");
        let before = fs::read_to_string(&item_path).expect("item before");

        let requested = request_planning_approval(
            project.path(),
            RequestPlanningApprovalParams {
                root: None,
                item_ids: vec!["PROJ-001".to_string()],
                requested_by: Some("manager".to_string()),
                summary: Some("Review the task plan before dispatch.".to_string()),
            },
        );
        let approval = requested.data.expect("planning approval").approval;

        assert!(matches!(requested.status, ActionStatus::Completed));
        assert_eq!(approval.scope, "planning");
        assert!(approval_covers_item(&approval, "PROJ-001"));
        assert_eq!(
            fs::read_to_string(&item_path).expect("item after"),
            before,
            "planning approval must not write runtime state into backlog markdown"
        );

        let state = planning_approval_state(project.path(), None, "PROJ-001", true)
            .expect("planning state");
        assert!(!state.approved);
        assert_eq!(state.status.as_deref(), Some("pending"));

        let approved = approval_respond(
            project.path(),
            ApprovalRespondParams {
                root: None,
                approval_id: approval.id,
                decision: "approve".to_string(),
                responder: Some("user".to_string()),
                reason: Some("Reviewed.".to_string()),
            },
        );
        assert!(matches!(approved.status, ActionStatus::Completed));
        assert!(
            planning_approval_is_approved(project.path(), None, "PROJ-001")
                .expect("approved state")
        );
    }

    #[test]
    fn planning_approval_lookup_includes_newest_records_beyond_first_page() {
        let project = backlog_project();
        for index in 0..1001 {
            create_approval(
                project.path(),
                None,
                NewApproval {
                    scope: "general".to_string(),
                    title: format!("Approval {index}"),
                    summary: "Filler approval.".to_string(),
                    requested_by: None,
                    metadata: BTreeMap::new(),
                },
            )
            .expect("filler approval");
        }

        let requested = request_planning_approval(
            project.path(),
            RequestPlanningApprovalParams {
                root: None,
                item_ids: vec!["PROJ-001".to_string()],
                requested_by: Some("manager".to_string()),
                summary: Some("Review the plan.".to_string()),
            },
        );
        let approval = requested.data.expect("planning approval").approval;

        let state = planning_approval_state(project.path(), None, "PROJ-001", true)
            .expect("planning approval state");

        assert_eq!(state.approval_id.as_deref(), Some(approval.id.as_str()));
        assert_eq!(state.status.as_deref(), Some("pending"));
    }

    fn backlog_project() -> TempDir {
        let project = TempDir::new().expect("temp dir");
        fs::write(project.path().join("platy.yaml"), "project: test\n").expect("config");
        fs::create_dir_all(project.path().join("backlog/items")).expect("items");
        fs::create_dir_all(project.path().join("backlog/epics")).expect("epics");
        fs::write(
            project.path().join("backlog/epics/general.md"),
            "---\nid: general\ntitle: General\nstatus: active\npriority: P1\narea: general\n---\n\n# General\n",
        )
        .expect("epic");
        fs::write(
            project.path().join("backlog/items/PROJ-001.md"),
            "---\nid: PROJ-001\ntitle: Planned work\npriority: P1\ntype: feature\narea: app\nepic: general\ndepends_on: []\nowned_surfaces:\n- src/app.rs\n---\n\n# PROJ-001 Planned work\n\n## Goal\n\nGoal.\n\n## Implementation Contract\n\nContract.\n\n## Acceptance\n\n- Done.\n",
        )
        .expect("item");
        project
    }
}
