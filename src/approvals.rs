use crate::{
    events::{self, NewEvent},
    models::{
        ActionResult, ActionStatus, ApprovalListData, ApprovalListParams, ApprovalRecord,
        ApprovalRespondParams, ApprovalResponseData,
    },
    storage::{self, ApprovalInsert},
};
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

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
    let existing = match approvals.get(approval_id).optional() {
        Ok(existing) => existing,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect approval.", error.to_string())
        }
    };
    let Some(existing) = existing else {
        return ActionResult::skipped(
            action,
            format!("Approval `{approval_id}` was not found."),
            "List pending approvals and retry with a valid approval id.",
        );
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
        models::{ApprovalRespondParams, EventsReplayParams},
    };
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
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "approval_requested");
        assert_eq!(events[1].event_type, "approval_responded");
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
}
