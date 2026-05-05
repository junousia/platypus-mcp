use crate::{
    models::{
        ActionResult, ActionStatus, InspectTaskEventsParams, TaskEventListData, TaskEventRecord,
    },
    storage,
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

pub fn record_task_event(
    default_root: &Path,
    root: Option<&str>,
    event: NewTaskEvent,
) -> Result<TaskEventRecord, String> {
    let storage = storage::connect(default_root, root).map_err(|error| error.to_string())?;
    let task_id = clean_required("task_id", &event.task_id)?;
    let event_type = clean_required("event_type", &event.event_type)?;
    let summary = clean_required("summary", &event.summary)?;
    let sequence = match event.sequence {
        Some(sequence) if sequence > 0 => sequence,
        Some(_) => return Err("sequence must be greater than zero".to_string()),
        None => next_sequence(&storage.connection, &task_id).map_err(|error| error.to_string())?,
    };
    let payload_json = event
        .payload
        .map(|payload| serde_json::to_string(&payload).unwrap_or_else(|_| "null".to_string()));

    storage
        .connection
        .execute(
            r#"
            INSERT INTO task_events(task_id, sequence, event_type, summary, payload_json)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![task_id, sequence, event_type, summary, payload_json],
        )
        .map_err(|error| error.to_string())?;

    get_task_event(&storage.connection, &task_id, sequence).map_err(|error| error.to_string())
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

    let events = match query_task_events(&storage.connection, task_id, limit) {
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

fn query_task_events(
    connection: &rusqlite::Connection,
    task_id: &str,
    limit: usize,
) -> rusqlite::Result<Vec<TaskEventRecord>> {
    let mut statement = connection.prepare(
        r#"
        SELECT task_id, sequence, event_type, summary, payload_json, created_at
        FROM task_events
        WHERE task_id = ?1
        ORDER BY sequence ASC, id ASC
        LIMIT ?2
        "#,
    )?;
    let rows = statement.query_map(params![task_id, limit], row_to_task_event)?;
    rows.collect()
}

fn get_task_event(
    connection: &rusqlite::Connection,
    task_id: &str,
    sequence: i64,
) -> rusqlite::Result<TaskEventRecord> {
    connection.query_row(
        r#"
        SELECT task_id, sequence, event_type, summary, payload_json, created_at
        FROM task_events
        WHERE task_id = ?1 AND sequence = ?2
        "#,
        params![task_id, sequence],
        row_to_task_event,
    )
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

fn row_to_task_event(row: &Row<'_>) -> rusqlite::Result<TaskEventRecord> {
    let payload_json: Option<String> = row.get("payload_json")?;
    Ok(TaskEventRecord {
        task_id: row.get("task_id")?,
        sequence: row.get("sequence")?,
        event_type: row.get("event_type")?,
        summary: row.get("summary")?,
        payload: payload_json.and_then(|raw| serde_json::from_str(&raw).ok()),
        created_at: row.get("created_at")?,
    })
}

fn clean_required(field: &str, value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(trimmed.to_string())
    }
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
}
