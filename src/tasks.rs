use crate::{
    models::{
        ActionResult, ActionStatus, ClaimNextTaskParams, InspectTaskEventsParams,
        InspectTaskParams, TaskEventListData, TaskEventRecord, TaskRecord, TaskRecordData,
        WorkerGuidanceData,
    },
    storage::{self, TaskEventInsert, TaskInsert},
};
use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;
use std::path::Path;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct NewTaskEvent {
    pub task_id: String,
    pub sequence: Option<i64>,
    pub event_type: String,
    pub summary: String,
    pub payload: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct NewTask {
    pub source_item_id: String,
    pub title: String,
    pub worker: Option<String>,
}

pub fn create_task_record(
    default_root: &Path,
    root: Option<&str>,
    task: NewTask,
) -> Result<TaskRecord, String> {
    let storage = storage::connect(default_root, root).map_err(|error| error.to_string())?;
    let source_item_id = clean_required("source_item_id", &task.source_item_id)?;
    let title = clean_required("title", &task.title)?;
    match storage.repository().tasks().create(TaskInsert {
        source_item_id: source_item_id.clone(),
        title,
        worker: clean_optional(task.worker),
    }) {
        Ok(task) => Ok(task),
        Err(error) => {
            if is_unique_constraint_error(&error) {
                return Err(format!(
                    "active task already exists for backlog item `{source_item_id}`"
                ));
            }
            Err(error.to_string())
        }
    }
}

pub fn record_task_event(
    default_root: &Path,
    root: Option<&str>,
    event: NewTaskEvent,
) -> Result<TaskEventRecord, String> {
    let storage = storage::connect(default_root, root).map_err(|error| error.to_string())?;
    let task_id = clean_required("task_id", &event.task_id)?;
    let event_type = clean_required("event_type", &event.event_type)?;
    let summary = clean_required("summary", &event.summary)?;
    if matches!(event.sequence, Some(sequence) if sequence <= 0) {
        return Err("sequence must be greater than zero".to_string());
    }
    storage
        .repository()
        .tasks()
        .record_event(TaskEventInsert {
            task_id,
            sequence: event.sequence,
            event_type,
            summary,
            payload: event.payload,
        })
        .map_err(|error| error.to_string())
}

pub fn get_task_by_id(
    default_root: &Path,
    root: Option<&str>,
    task_id: &str,
) -> Result<TaskRecord, String> {
    let storage = storage::connect(default_root, root).map_err(|error| error.to_string())?;
    storage
        .repository()
        .tasks()
        .get(task_id)
        .map_err(|error| error.to_string())
}

pub fn mark_task_running(
    default_root: &Path,
    root: Option<&str>,
    task_id: &str,
) -> Result<TaskRecord, String> {
    let storage = storage::connect(default_root, root).map_err(|error| error.to_string())?;
    let task_id = clean_required("task_id", task_id)?;
    let tasks = storage.repository().tasks();
    let updated = tasks
        .mark_running(&task_id)
        .map_err(|error| error.to_string())?;
    if updated == 0 {
        return Err(format!(
            "task `{task_id}` must be claimed before it can run"
        ));
    }
    tasks.get(&task_id).map_err(|error| error.to_string())
}

pub fn finish_task(
    default_root: &Path,
    root: Option<&str>,
    task_id: &str,
    status: &str,
) -> Result<TaskRecord, String> {
    let storage = storage::connect(default_root, root).map_err(|error| error.to_string())?;
    let task_id = clean_required("task_id", task_id)?;
    let status = clean_terminal_status(status)?;
    let tasks = storage.repository().tasks();
    let updated = tasks
        .finish(&task_id, &status)
        .map_err(|error| error.to_string())?;
    if updated == 0 {
        return Err(format!(
            "task `{task_id}` must be claimed or running before it can finish"
        ));
    }
    tasks.get(&task_id).map_err(|error| error.to_string())
}

pub fn send_worker_guidance(
    default_root: &Path,
    params: crate::models::SendWorkerGuidanceParams,
) -> ActionResult<WorkerGuidanceData> {
    let action = "send_worker_guidance";
    let task_id = match clean_required("task_id", &params.task_id) {
        Ok(task_id) => task_id,
        Err(error) => {
            return ActionResult::failed(action, "Could not send worker guidance.", error)
        }
    };
    let message = match clean_required("message", &params.message) {
        Ok(message) => message,
        Err(error) => {
            return ActionResult::failed(action, "Could not send worker guidance.", error)
        }
    };
    let author = clean_optional(params.author).unwrap_or_else(|| "user".to_string());
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(action, "Could not open task storage.", error.to_string())
        }
    };
    let tasks = storage.repository().tasks();
    let task = match tasks.get(&task_id) {
        Ok(task) => task,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return ActionResult::skipped(
                action,
                format!("Task `{task_id}` was not found."),
                "Dispatch work before sending worker guidance.",
            )
        }
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect task.", error.to_string())
        }
    };
    if !active_status(&task.status) {
        return ActionResult::skipped(
            action,
            format!("Task `{task_id}` is {}.", task.status),
            "Send worker guidance only to queued, claimed, or running tasks.",
        );
    }
    let event = match tasks.record_event(TaskEventInsert {
        task_id: task.id.clone(),
        sequence: None,
        event_type: "worker_guidance".to_string(),
        summary: format!("Guidance sent by `{author}`."),
        payload: Some(serde_json::json!({
            "author": author,
            "message": message,
            "task_status": task.status
        })),
    }) {
        Ok(event) => event,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not persist worker guidance.",
                error.to_string(),
            )
        }
    };
    ActionResult::completed(
        action,
        format!("Guidance recorded for task `{task_id}`."),
        WorkerGuidanceData {
            root: storage.storage.root.display().to_string(),
            task_id,
            author,
            message,
            event,
        },
    )
}

pub fn inspect_task(
    default_root: &Path,
    params: InspectTaskParams,
) -> ActionResult<TaskRecordData> {
    let action = "inspect_task";
    let task_id = params.task_id.trim();
    if task_id.is_empty() {
        return ActionResult::failed(action, "Could not inspect task.", "task_id is required");
    }
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(action, "Could not open task storage.", error.to_string())
        }
    };
    match storage.repository().tasks().get(task_id) {
        Ok(task) => ActionResult::completed(
            action,
            format!("Task `{}` inspected.", task.id),
            TaskRecordData {
                root: storage.storage.root.display().to_string(),
                task,
            },
        ),
        Err(rusqlite::Error::QueryReturnedNoRows) => ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: format!("Task `{task_id}` was not found."),
            next_action: Some("Dispatch work before inspecting a task id.".to_string()),
            data: None,
            error: None,
        },
        Err(error) => ActionResult::failed(action, "Could not inspect task.", error.to_string()),
    }
}

pub fn claim_next_task(
    default_root: &Path,
    params: ClaimNextTaskParams,
) -> ActionResult<TaskRecordData> {
    let action = "claim_next_task";
    let worker = clean_optional(params.worker);
    let claimant = clean_optional(params.claimant).unwrap_or_else(|| "runner".to_string());
    let mut storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(action, "Could not open task storage.", error.to_string())
        }
    };
    let root = storage.storage.root.display().to_string();

    let transaction = match storage.connection.transaction() {
        Ok(transaction) => transaction,
        Err(error) => {
            return ActionResult::failed(action, "Could not begin task claim.", error.to_string())
        }
    };

    let Some(task) = (match select_next_queued_task(&transaction, worker.as_deref()) {
        Ok(task) => task,
        Err(error) => {
            return ActionResult::failed(action, "Could not select queued task.", error.to_string())
        }
    }) else {
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No queued task is available to claim.".to_string(),
            next_action: Some("Dispatch runnable backlog work first.".to_string()),
            data: None,
            error: None,
        };
    };

    let updated = match transaction.execute(
        r#"
        UPDATE tasks
        SET status = 'claimed',
            claimed_by = ?2,
            claimed_at = CURRENT_TIMESTAMP,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1 AND status = 'queued'
        "#,
        params![task.id, claimant],
    ) {
        Ok(updated) => updated,
        Err(error) => {
            return ActionResult::failed(action, "Could not claim task.", error.to_string())
        }
    };
    if updated == 0 {
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: format!("Task `{}` was already claimed.", task.id),
            next_action: Some("Retry claim_next_task to claim another queued task.".to_string()),
            data: None,
            error: None,
        };
    }

    let claimed = match get_task(&transaction, &task.id) {
        Ok(task) => task,
        Err(error) => {
            return ActionResult::failed(action, "Could not load claimed task.", error.to_string())
        }
    };
    let sequence = match next_sequence(&transaction, &claimed.id) {
        Ok(sequence) => sequence,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not allocate task event.",
                error.to_string(),
            )
        }
    };
    let payload = serde_json::json!({
        "worker": claimed.worker.as_deref(),
        "claimed_by": claimed.claimed_by.as_deref(),
        "status": claimed.status.as_str()
    });
    let payload_json = match serde_json::to_string(&payload) {
        Ok(payload_json) => payload_json,
        Err(error) => {
            return ActionResult::failed(action, "Could not encode claim event.", error.to_string())
        }
    };
    if let Err(error) = transaction.execute(
        r#"
        INSERT INTO task_events(task_id, sequence, event_type, summary, payload_json)
        VALUES (?1, ?2, 'task_claimed', ?3, ?4)
        "#,
        params![
            claimed.id,
            sequence,
            format!(
                "Task claimed by `{}`.",
                claimed.claimed_by.as_deref().unwrap_or("runner")
            ),
            payload_json
        ],
    ) {
        return ActionResult::failed(action, "Could not record claim event.", error.to_string());
    }
    if let Err(error) = transaction.commit() {
        return ActionResult::failed(action, "Could not commit task claim.", error.to_string());
    }

    ActionResult::completed(
        action,
        format!("Claimed task `{}`.", claimed.id),
        TaskRecordData {
            root,
            task: claimed,
        },
    )
}

pub fn inspect_task_events(
    default_root: &Path,
    params: InspectTaskEventsParams,
) -> ActionResult<TaskEventListData> {
    let action = "inspect_task_events";
    let task_id = params.task_id.trim();
    if task_id.is_empty() {
        return ActionResult::failed(
            action,
            "Could not inspect task events.",
            "task_id is required",
        );
    }

    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open task event storage.",
                error.to_string(),
            )
        }
    };
    let limit = bounded_limit(params.limit);

    let events = match storage.repository().tasks().list_events(task_id, limit) {
        Ok(events) => events,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not inspect task events.",
                error.to_string(),
            );
        }
    };
    let returned = events.len();
    let data = TaskEventListData {
        root: storage.storage.root.display().to_string(),
        task_id: task_id.to_string(),
        events,
        returned,
    };

    if returned == 0 {
        ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: format!("No events found for task `{task_id}`."),
            next_action: Some(
                "Check the task id or dispatch work before inspecting events.".to_string(),
            ),
            data: Some(data),
            error: None,
        }
    } else {
        ActionResult::completed(action, format!("Returned {returned} task event(s)."), data)
    }
}

fn get_task(connection: &rusqlite::Connection, id: &str) -> rusqlite::Result<TaskRecord> {
    connection.query_row(
        r#"
        SELECT id, source_item_id, title, status, worker, claimed_by, claimed_at, started_at,
               finished_at, workspace_path, workspace_branch, workspace_base_ref, created_at,
               updated_at
        FROM tasks
        WHERE id = ?1
        "#,
        [id],
        row_to_task,
    )
}

fn select_next_queued_task(
    connection: &rusqlite::Connection,
    worker: Option<&str>,
) -> rusqlite::Result<Option<TaskRecord>> {
    connection
        .query_row(
            r#"
            SELECT id, source_item_id, title, status, worker, claimed_by, claimed_at, started_at,
                   finished_at, workspace_path, workspace_branch, workspace_base_ref, created_at,
                   updated_at
            FROM tasks
            WHERE status = 'queued'
              AND (?1 IS NULL OR worker IS NULL OR worker = ?1)
            ORDER BY created_at ASC, id ASC
            LIMIT 1
            "#,
            [worker],
            row_to_task,
        )
        .optional()
}

fn next_sequence(connection: &rusqlite::Connection, task_id: &str) -> rusqlite::Result<i64> {
    let current: Option<i64> = connection
        .query_row(
            "SELECT MAX(sequence) FROM task_events WHERE task_id = ?1",
            [task_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(current.unwrap_or(0) + 1)
}

fn row_to_task(row: &Row<'_>) -> rusqlite::Result<TaskRecord> {
    Ok(TaskRecord {
        id: row.get("id")?,
        source_item_id: row.get("source_item_id")?,
        title: row.get("title")?,
        status: row.get("status")?,
        worker: row.get("worker")?,
        claimed_by: row.get("claimed_by")?,
        claimed_at: row.get("claimed_at")?,
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
        workspace_path: row.get("workspace_path")?,
        workspace_branch: row.get("workspace_branch")?,
        workspace_base_ref: row.get("workspace_base_ref")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn is_unique_constraint_error(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(error, _)
            if error.code == rusqlite::ErrorCode::ConstraintViolation
    )
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

fn clean_terminal_status(value: &str) -> Result<String, String> {
    match value.trim() {
        "completed" | "failed" | "cancelled" => Ok(value.trim().to_string()),
        _ => Err("terminal task status must be completed, failed, or cancelled".to_string()),
    }
}

fn active_status(value: &str) -> bool {
    matches!(value, "queued" | "claimed" | "running")
}

fn bounded_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn records_and_replays_task_events_in_sequence_order() {
        let project = TempDir::new().expect("temp dir");
        record_task_event(
            project.path(),
            None,
            NewTaskEvent {
                task_id: "task-1".to_string(),
                sequence: Some(2),
                event_type: "worker_result".to_string(),
                summary: "Worker finished.".to_string(),
                payload: Some(serde_json::json!({ "status": "completed" })),
            },
        )
        .expect("second event");
        record_task_event(
            project.path(),
            None,
            NewTaskEvent {
                task_id: "task-1".to_string(),
                sequence: Some(1),
                event_type: "worker_started".to_string(),
                summary: "Worker started.".to_string(),
                payload: None,
            },
        )
        .expect("first event");

        let inspected = inspect_task_events(
            project.path(),
            InspectTaskEventsParams {
                root: None,
                task_id: "task-1".to_string(),
                limit: None,
            },
        );
        let data = inspected.data.expect("event data");

        assert!(matches!(inspected.status, ActionStatus::Completed));
        assert_eq!(data.returned, 2);
        assert_eq!(data.events[0].sequence, 1);
        assert_eq!(data.events[1].sequence, 2);
    }

    #[test]
    fn unknown_task_returns_skipped_structured_result() {
        let project = TempDir::new().expect("temp dir");

        let inspected = inspect_task_events(
            project.path(),
            InspectTaskEventsParams {
                root: None,
                task_id: "missing".to_string(),
                limit: None,
            },
        );

        assert!(matches!(inspected.status, ActionStatus::Skipped));
        assert_eq!(inspected.data.expect("event data").returned, 0);
    }

    #[test]
    fn auto_allocates_next_sequence() {
        let project = TempDir::new().expect("temp dir");
        let first = record_task_event(
            project.path(),
            None,
            NewTaskEvent {
                task_id: "task-1".to_string(),
                sequence: None,
                event_type: "started".to_string(),
                summary: "Started.".to_string(),
                payload: None,
            },
        )
        .expect("first");
        let second = record_task_event(
            project.path(),
            None,
            NewTaskEvent {
                task_id: "task-1".to_string(),
                sequence: None,
                event_type: "finished".to_string(),
                summary: "Finished.".to_string(),
                payload: None,
            },
        )
        .expect("second");

        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
    }

    #[test]
    fn prevents_duplicate_active_task_for_backlog_item() {
        let project = TempDir::new().expect("temp dir");
        create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "First task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("first task");

        let duplicate = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Duplicate task".to_string(),
                worker: Some("coder".to_string()),
            },
        );

        assert!(duplicate
            .expect_err("duplicate active task")
            .contains("active task already exists"));
    }

    #[test]
    fn claims_next_queued_task_and_records_event() {
        let project = TempDir::new().expect("temp dir");
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Claimable task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");

        let claimed = claim_next_task(
            project.path(),
            ClaimNextTaskParams {
                root: None,
                worker: Some("coder".to_string()),
                claimant: Some("runner-1".to_string()),
            },
        );
        let claimed_task = claimed.data.expect("claimed data").task;

        assert!(matches!(claimed.status, ActionStatus::Completed));
        assert_eq!(claimed_task.id, task.id);
        assert_eq!(claimed_task.status, "claimed");
        assert_eq!(claimed_task.claimed_by.as_deref(), Some("runner-1"));
        assert!(claimed_task.claimed_at.is_some());

        let events = inspect_task_events(
            project.path(),
            InspectTaskEventsParams {
                root: None,
                task_id: task.id,
                limit: None,
            },
        );
        let events = events.data.expect("event data");
        assert_eq!(events.returned, 1);
        assert_eq!(events.events[0].event_type, "task_claimed");
    }

    #[test]
    fn marks_claimed_task_running_and_finished() {
        let project = TempDir::new().expect("temp dir");
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Runnable task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        claim_next_task(
            project.path(),
            ClaimNextTaskParams {
                root: None,
                worker: Some("coder".to_string()),
                claimant: None,
            },
        );

        let running = mark_task_running(project.path(), None, &task.id).expect("running");
        assert_eq!(running.status, "running");
        assert!(running.started_at.is_some());

        let finished = finish_task(project.path(), None, &task.id, "completed").expect("finished");
        assert_eq!(finished.status, "completed");
        assert!(finished.finished_at.is_some());
    }

    #[test]
    fn sends_worker_guidance_to_active_task() {
        let project = TempDir::new().expect("temp dir");
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Guided task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");

        let guidance = send_worker_guidance(
            project.path(),
            crate::models::SendWorkerGuidanceParams {
                root: None,
                task_id: task.id.clone(),
                message: "Focus on tests first.".to_string(),
                author: Some("manager".to_string()),
            },
        );
        let data = guidance.data.expect("guidance data");

        assert!(matches!(guidance.status, ActionStatus::Completed));
        assert_eq!(data.author, "manager");
        assert_eq!(data.event.event_type, "worker_guidance");

        let events = inspect_task_events(
            project.path(),
            InspectTaskEventsParams {
                root: None,
                task_id: task.id,
                limit: None,
            },
        )
        .data
        .expect("events");
        assert_eq!(events.returned, 1);
        assert_eq!(events.events[0].event_type, "worker_guidance");
    }

    #[test]
    fn rejects_guidance_for_unknown_or_terminal_task() {
        let project = TempDir::new().expect("temp dir");
        let missing = send_worker_guidance(
            project.path(),
            crate::models::SendWorkerGuidanceParams {
                root: None,
                task_id: "missing".to_string(),
                message: "Continue.".to_string(),
                author: None,
            },
        );
        assert!(matches!(missing.status, ActionStatus::Skipped));

        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Finished task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        claim_next_task(
            project.path(),
            ClaimNextTaskParams {
                root: None,
                worker: Some("coder".to_string()),
                claimant: None,
            },
        );
        finish_task(project.path(), None, &task.id, "completed").expect("finish");

        let terminal = send_worker_guidance(
            project.path(),
            crate::models::SendWorkerGuidanceParams {
                root: None,
                task_id: task.id,
                message: "Continue.".to_string(),
                author: None,
            },
        );
        assert!(matches!(terminal.status, ActionStatus::Skipped));
    }
}
