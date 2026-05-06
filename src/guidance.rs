use crate::{
    backlog,
    models::{ActionResult, BacklogListData, NextSafeActionData, NextSafeActionParams},
    storage,
};
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

pub fn next_safe_action(
    default_root: &Path,
    params: NextSafeActionParams,
) -> ActionResult<NextSafeActionData> {
    let action = "next_safe_action";
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not inspect next safe action.",
                error.to_string(),
            )
        }
    };
    let root = storage.storage.root.display().to_string();

    if let Ok(Some(assignment)) = active_assignment(&storage.connection, "running") {
        return completed(
            action,
            &root,
            "record_worker_progress",
            format!(
                "Worker assignment `{}` is running for task `{}`.",
                assignment.id, assignment.task_id
            ),
            "Record progress while the worker is active, or complete the worker task when the result is ready.",
            [
                ("root", root.as_str()),
                ("assignment_id", assignment.id.as_str()),
                ("event_type", "worker_progress"),
                ("summary", "Describe the worker progress."),
            ],
        );
    }
    if let Ok(Some(assignment)) = active_assignment(&storage.connection, "prepared") {
        return completed(
            action,
            &root,
            "start_worker_task",
            format!(
                "Worker assignment `{}` is prepared for task `{}`.",
                assignment.id, assignment.task_id
            ),
            "Start the prepared assignment before recording progress or completion.",
            [
                ("root", root.as_str()),
                ("assignment_id", assignment.id.as_str()),
                ("worker_session", "external-worker-session"),
                ("summary", ""),
            ],
        );
    }
    if let Ok(Some(task)) = queued_task(&storage.connection) {
        return completed(
            action,
            &root,
            "prepare_worker_handoff",
            format!("Task `{}` is queued for worker execution.", task.id),
            "Prepare a single handoff object so the host can assign the worktree and bundle to a worker.",
            [
                ("root", root.as_str()),
                ("task_id", task.id.as_str()),
                ("worker", task.worker.as_deref().unwrap_or("")),
                ("claimant", "external-worker"),
            ],
        );
    }
    if let Ok(Some(task)) = completed_task_without_verification(&storage.connection) {
        return completed(
            action,
            &root,
            "record_verification_evidence",
            format!("Completed task `{}` has no verification evidence.", task.id),
            "Record verification evidence before treating the work as reconciled.",
            [
                ("root", root.as_str()),
                ("source_item_id", task.source_item_id.as_str()),
                ("source_task_id", task.id.as_str()),
                ("summary", "Describe the verification result."),
            ],
        );
    }

    let backlog = backlog::list_backlog(default_root, Some(root.as_str()), Some(1));
    if let ActionResult {
        data: Some(BacklogListData { candidates, .. }),
        ..
    } = backlog
    {
        if let Some(candidate) = candidates.first() {
            return completed(
                action,
                &root,
                "dispatch_next_work",
                format!("Backlog item `{}` is runnable.", candidate.item_id),
                "Dispatch the next runnable backlog item to create a durable task.",
                [("root", root.as_str()), ("summary", "")],
            );
        }
    }

    completed(
        action,
        &root,
        "create_backlog_item",
        "No queued task or runnable backlog item was found.".to_string(),
        "Create or draft a backlog item before dispatching work.",
        [
            ("root", root.as_str()),
            ("summary", "Describe the work item."),
        ],
    )
}

#[derive(Debug)]
struct AssignmentHint {
    id: String,
    task_id: String,
}

#[derive(Debug)]
struct TaskHint {
    id: String,
    source_item_id: String,
    worker: Option<String>,
}

fn active_assignment(
    connection: &rusqlite::Connection,
    status: &str,
) -> rusqlite::Result<Option<AssignmentHint>> {
    connection
        .query_row(
            r#"
            SELECT id, task_id
            FROM worker_assignments
            WHERE status = ?1
            ORDER BY updated_at ASC, id ASC
            LIMIT 1
            "#,
            [status],
            |row| {
                Ok(AssignmentHint {
                    id: row.get("id")?,
                    task_id: row.get("task_id")?,
                })
            },
        )
        .optional()
}

fn queued_task(connection: &rusqlite::Connection) -> rusqlite::Result<Option<TaskHint>> {
    connection
        .query_row(
            r#"
            SELECT id, source_item_id, worker
            FROM tasks
            WHERE status = 'queued'
            ORDER BY created_at ASC, id ASC
            LIMIT 1
            "#,
            [],
            row_to_task_hint,
        )
        .optional()
}

fn completed_task_without_verification(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<Option<TaskHint>> {
    connection
        .query_row(
            r#"
            SELECT tasks.id, tasks.source_item_id, tasks.worker
            FROM tasks
            WHERE tasks.status = 'completed'
              AND NOT EXISTS (
                SELECT 1 FROM evidence
                WHERE evidence.source_task_id = tasks.id
                  AND evidence.kind = 'verification'
              )
            ORDER BY tasks.finished_at ASC, tasks.id ASC
            LIMIT 1
            "#,
            [],
            row_to_task_hint,
        )
        .optional()
}

fn row_to_task_hint(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskHint> {
    Ok(TaskHint {
        id: row.get("id")?,
        source_item_id: row.get("source_item_id")?,
        worker: row.get("worker")?,
    })
}

fn completed<const N: usize>(
    action: &str,
    root: &str,
    recommended_tool: &str,
    summary: String,
    reason: &str,
    params: [(&str, &str); N],
) -> ActionResult<NextSafeActionData> {
    ActionResult::completed(
        action,
        summary.clone(),
        NextSafeActionData {
            root: root.to_string(),
            recommended_tool: recommended_tool.to_string(),
            summary,
            reason: reason.to_string(),
            params: params
                .into_iter()
                .filter(|(_, value)| !value.is_empty())
                .map(|(key, value)| (key.to_string(), Value::String(value.to_string())))
                .collect::<BTreeMap<_, _>>(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{create_task_record, NewTask};
    use tempfile::TempDir;

    #[test]
    fn recommends_preparing_queued_task() {
        let project = TempDir::new().expect("temp dir");
        create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Queued task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");

        let result = next_safe_action(project.path(), NextSafeActionParams { root: None });
        let data = result.data.expect("next action");

        assert_eq!(data.recommended_tool, "prepare_worker_handoff");
        assert_eq!(data.params["task_id"], "PROJ-001-T001");
    }
}
