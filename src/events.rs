use crate::{
    models::{ActionResult, ActionStatus, EventRecord, EventsReplayData, EventsReplayParams},
    storage::EventStore,
    storage::{self, EventInsert},
};
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
    storage
        .repository()
        .events()
        .record(EventInsert {
            event_type,
            scope,
            task_id,
            summary,
            payload: event.payload,
        })
        .map_err(|error| error.to_string())
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

    let event_repository = storage.repository().events();
    let mut events = match event_repository.list(scope, task_id, limit) {
        Ok(events) => events,
        Err(error) => {
            return ActionResult::failed(action, "Could not replay events.", error.to_string())
        }
    };
    if scope.is_none() || scope == Some("task") {
        match event_repository.list_task_events(task_id, limit) {
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
