use crate::{
    models::{
        ActionResult, ActionStatus, BacklogCandidate, DispatchNextWorkData, RootParams, TaskRecord,
    },
    state::{
        sqlite::SqliteProjectState, BacklogCandidateSnapshot, DispatchWorkCommand, ProjectState,
        ProjectStateError, TaskSnapshot,
    },
};
use std::path::Path;

pub fn dispatch_next_work(
    default_root: &Path,
    params: RootParams,
) -> ActionResult<DispatchNextWorkData> {
    let action = "dispatch_next_work";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    match state.dispatch_work(DispatchWorkCommand {
        summary: None,
        preferred_worker: None,
    }) {
        Ok(outcome) => {
            let task = task_record(outcome.task);
            ActionResult {
                action: action.to_string(),
                status: ActionStatus::Completed,
                summary: format!("Dispatched {} as task {}.", task.source_item_id, task.id),
                next_action: Some(
                    "Use inspect_task_events and an external harness to execute the queued task."
                        .to_string(),
                ),
                data: Some(DispatchNextWorkData {
                    root: state.root().display().to_string(),
                    candidate: backlog_candidate(outcome.candidate),
                    task,
                }),
                error: None,
            }
        }
        Err(ProjectStateError::NotFound { .. }) => ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No runnable backlog items to dispatch.".to_string(),
            next_action: Some("Create or unblock backlog items.".to_string()),
            data: None,
            error: None,
        },
        Err(ProjectStateError::Conflict { message }) if message.contains("leased by") => {
            ActionResult::skipped(
                action,
                message,
                "Wait for the lease to expire or release it before dispatching work.",
            )
        }
        Err(error) => state_error(action, "Could not dispatch runnable backlog work.", error),
    }
}

fn state_error<T: schemars::JsonSchema + serde::Serialize>(
    action: &str,
    summary: &str,
    error: ProjectStateError,
) -> ActionResult<T> {
    ActionResult::failed(action, summary, error.to_string())
}

fn backlog_candidate(candidate: BacklogCandidateSnapshot) -> BacklogCandidate {
    BacklogCandidate {
        source: candidate.source,
        item_id: candidate.item_id,
        title: candidate.title,
        priority: candidate.priority,
        item_type: candidate.item_type,
        area: candidate.area,
        suggested_worker: candidate.suggested_worker,
        owned_surfaces: candidate.owned_surfaces,
        external_refs: candidate.external_refs,
    }
}

fn task_record(task: TaskSnapshot) -> TaskRecord {
    let (workspace_path, workspace_branch, workspace_base_ref) = match task.worker_workspace {
        Some(workspace) => (
            Some(workspace.path),
            Some(workspace.branch),
            Some(workspace.base_ref),
        ),
        None => (None, None, None),
    };
    TaskRecord {
        id: task.id,
        source_item_id: task.source_item_id,
        title: task.title,
        status: task.status,
        worker: task.worker,
        claimed_by: task.claimed_by,
        claimed_at: task.claimed_at,
        started_at: task.started_at,
        finished_at: task.finished_at,
        workspace_path,
        workspace_branch,
        workspace_base_ref,
        created_at: task.created_at,
        updated_at: task.updated_at,
    }
}
