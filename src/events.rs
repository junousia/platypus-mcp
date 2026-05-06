use crate::{
    models::{ActionResult, ActionStatus, EventRecord, EventsReplayData, EventsReplayParams},
    storage,
};
use rusqlite::{params, Row};
use serde_json::Value;
use std::path::Path;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct NewEvent {
    pub event_type: String,
    pub scope: String,
    pub task_id: Option<String>,
    pub summary: String,
    pub payload: Option<Value>,
}

pub fn record_event(
    default_root: &Path,
    root: Option<&str>,
    event: NewEvent,
) -> Result<EventRecord, String> {
    let storage = storage::connect(default_root, root).map_err(|error| error.to_string())?;
    let event_type = clean_required("event_type", &event.event_type)?;
    let scope = clean_scope(&event.scope)?;
    let summary = clean_required("summary", &event.summary)?;
    let task_id = clean_optional(event.task_id);
    let payload_json = event
        .payload
        .map(|payload| serde_json::to_string(&payload).unwrap_or_else(|_| "null".to_string()));

    storage
        .connection
        .execute(
            r#"
            INSERT INTO events(event_type, scope, task_id, summary, payload_json)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![event_type, scope, task_id, summary, payload_json],
        )
        .map_err(|error| error.to_string())?;
    let id = storage.connection.last_insert_rowid();
    get_event(&storage.connection, id).map_err(|error| error.to_string())
}

pub fn events_replay(
    default_root: &Path,
    params: EventsReplayParams,
) -> ActionResult<EventsReplayData> {
    let action = "events_replay";
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(action, "Could not open event storage.", error.to_string())
        }
    };
    let scope = params
        .scope
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let task_id = params
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let limit = bounded_limit(params.limit);

    let mut events = match query_events(&storage.connection, scope, task_id, limit) {
        Ok(events) => events,
        Err(error) => {
            return ActionResult::failed(action, "Could not replay events.", error.to_string())
        }
    };
    if scope.is_none() || scope == Some("task") {
        match query_task_events(&storage.connection, task_id, limit) {
            Ok(task_events) => events.extend(task_events),
            Err(error) => {
                return ActionResult::failed(
                    action,
                    "Could not replay task events.",
                    error.to_string(),
                )
            }
        }
    }
    events.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.cursor.cmp(&right.cursor))
    });
    if events.len() > limit {
        events.truncate(limit);
    }
    let returned = events.len();
    let data = EventsReplayData {
        root: storage.storage.root.display().to_string(),
        events,
        returned,
    };
    if returned == 0 {
        ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No events matched the replay filters.".to_string(),
            next_action: Some("Run work or create approvals before replaying events.".to_string()),
            data: Some(data),
            error: None,
        }
    } else {
        ActionResult::completed(action, format!("Replayed {returned} event(s)."), data)
    }
}

fn get_event(connection: &rusqlite::Connection, id: i64) -> rusqlite::Result<EventRecord> {
    connection.query_row(
        r#"
        SELECT id, event_type, scope, task_id, summary, payload_json, created_at
        FROM events
        WHERE id = ?1
        "#,
        [id],
        row_to_event,
    )
}

fn query_events(
    connection: &rusqlite::Connection,
    scope: Option<&str>,
    task_id: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<EventRecord>> {
    let mut statement = connection.prepare(
        r#"
        SELECT id, event_type, scope, task_id, summary, payload_json, created_at
        FROM events
        WHERE (?1 IS NULL OR scope = ?1)
          AND (?2 IS NULL OR task_id = ?2)
        ORDER BY id ASC
        LIMIT ?3
        "#,
    )?;
    let rows = statement.query_map(params![scope, task_id, limit], row_to_event)?;
    rows.collect()
}

fn query_task_events(
    connection: &rusqlite::Connection,
    task_id: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<EventRecord>> {
    let mut statement = connection.prepare(
        r#"
        SELECT task_id, sequence, event_type, summary, payload_json, created_at
        FROM task_events
        WHERE (?1 IS NULL OR task_id = ?1)
        ORDER BY id ASC
        LIMIT ?2
        "#,
    )?;
    let rows = statement.query_map(params![task_id, limit], |row| {
        let task_id: String = row.get("task_id")?;
        let sequence: i64 = row.get("sequence")?;
        let payload_json: Option<String> = row.get("payload_json")?;
        Ok(EventRecord {
            cursor: format!("task:{task_id}:{sequence}"),
            event_type: row.get("event_type")?,
            scope: "task".to_string(),
            task_id: Some(task_id),
            summary: row.get("summary")?,
            payload: payload_json.and_then(|raw| serde_json::from_str(&raw).ok()),
            created_at: row.get("created_at")?,
        })
    })?;
    rows.collect()
}

fn row_to_event(row: &Row<'_>) -> rusqlite::Result<EventRecord> {
    let id: i64 = row.get("id")?;
    let payload_json: Option<String> = row.get("payload_json")?;
    Ok(EventRecord {
        cursor: format!("event:{id}"),
        event_type: row.get("event_type")?,
        scope: row.get("scope")?,
        task_id: row.get("task_id")?,
        summary: row.get("summary")?,
        payload: payload_json.and_then(|raw| serde_json::from_str(&raw).ok()),
        created_at: row.get("created_at")?,
    })
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
    use crate::tasks::{record_task_event, NewTaskEvent};
    use tempfile::TempDir;

    #[test]
    fn records_and_replays_project_and_task_events() {
        let project = TempDir::new().expect("temp dir");
        record_event(
            project.path(),
            None,
            NewEvent {
                event_type: "approval_requested".to_string(),
                scope: "approval".to_string(),
                task_id: None,
                summary: "Approval requested.".to_string(),
                payload: Some(serde_json::json!({ "approval": "A-001" })),
            },
        )
        .expect("event");
        record_task_event(
            project.path(),
            None,
            NewTaskEvent {
                task_id: "task-1".to_string(),
                sequence: None,
                event_type: "worker_started".to_string(),
                summary: "Worker started.".to_string(),
                payload: None,
            },
        )
        .expect("task event");

        let replayed = events_replay(
            project.path(),
            EventsReplayParams {
                root: None,
                task_id: None,
                scope: None,
                limit: None,
            },
        );
        let data = replayed.data.expect("events");

        assert!(matches!(replayed.status, ActionStatus::Completed));
        assert_eq!(data.returned, 2);
        assert!(data.events.iter().any(|event| event.scope == "approval"));
        assert!(data.events.iter().any(|event| event.scope == "task"));
    }

    #[test]
    fn replay_filters_by_task_id() {
        let project = TempDir::new().expect("temp dir");
        record_task_event(
            project.path(),
            None,
            NewTaskEvent {
                task_id: "task-1".to_string(),
                sequence: None,
                event_type: "worker_started".to_string(),
                summary: "Worker started.".to_string(),
                payload: None,
            },
        )
        .expect("task event");

        let replayed = events_replay(
            project.path(),
            EventsReplayParams {
                root: None,
                task_id: Some("task-1".to_string()),
                scope: Some("task".to_string()),
                limit: Some(10),
            },
        );
        let data = replayed.data.expect("events");

        assert_eq!(data.returned, 1);
        assert_eq!(data.events[0].task_id.as_deref(), Some("task-1"));
    }
}
