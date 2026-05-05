use crate::{
    backlog,
    models::{ActionResult, ActionStatus, BacklogListData, DispatchNextWorkData, RootParams},
    tasks::{self, NewTask, NewTaskEvent},
};
use serde_json::json;
use std::path::Path;

pub fn dispatch_next_work(
    default_root: &Path,
    params: RootParams,
) -> ActionResult<DispatchNextWorkData> {
    let action = "dispatch_next_work";
    let backlog = backlog::list_backlog(default_root, params.root.as_deref(), Some(1));
    let BacklogListData { root, candidates } = match backlog {
        ActionResult {
            status: ActionStatus::Completed,
            data: Some(data),
            ..
        }
        | ActionResult {
            status: ActionStatus::Skipped,
            data: Some(data),
            ..
        } => data,
        ActionResult { error, summary, .. } => {
            return ActionResult::failed(
                action,
                "Could not select runnable backlog work.",
                error.unwrap_or(summary),
            );
        }
    };

    let Some(candidate) = candidates.into_iter().next() else {
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No runnable backlog items to dispatch.".to_string(),
            next_action: Some("Create or unblock backlog items.".to_string()),
            data: None,
            error: None,
        };
    };

    let task = match tasks::create_task_record(
        default_root,
        Some(&root),
        NewTask {
            source_item_id: candidate.item_id.clone(),
            title: candidate.title.clone(),
            worker: candidate.suggested_worker.clone(),
        },
    ) {
        Ok(task) => task,
        Err(error) => {
            return ActionResult::failed(action, "Could not create task record.", error);
        }
    };

    if let Err(error) = tasks::record_task_event(
        default_root,
        Some(&root),
        NewTaskEvent {
            task_id: task.id.clone(),
            sequence: Some(1),
            event_type: "task_queued".to_string(),
            summary: format!(
                "Queued {} for external worker execution.",
                candidate.item_id
            ),
            payload: Some(json!({
                "source": candidate.source,
                "item_id": candidate.item_id,
                "worker": candidate.suggested_worker,
                "status": "queued"
            })),
        },
    ) {
        return ActionResult::failed(action, "Could not persist task event.", error);
    }

    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Completed,
        summary: format!("Dispatched {} as task {}.", task.source_item_id, task.id),
        next_action: Some(
            "Use inspect_task_events and an external harness to execute the queued task."
                .to_string(),
        ),
        data: Some(DispatchNextWorkData {
            root,
            candidate,
            task,
        }),
        error: None,
    }
}
