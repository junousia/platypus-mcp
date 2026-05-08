use crate::models::{TaskBundle, TaskEventRecord, TaskRecord, WorkerAssignment, WorktreeData};
use rusqlite::{params, OptionalExtension, Row};
use serde_json::{json, Value};

pub(super) enum ClaimOutcome {
    Claimed(TaskRecord),
    Existing(WorkerAssignment),
    NoTask,
}

pub(super) fn claim_task_for_assignment(
    connection: &mut rusqlite::Connection,
    task_id: Option<&str>,
    worker: Option<&str>,
    claimant: &str,
) -> Result<ClaimOutcome, String> {
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let Some(task) = (match task_id {
        Some(task_id) => select_task_optional(&transaction, task_id),
        None => select_next_queued_task(&transaction, worker),
    })
    .map_err(|error| error.to_string())?
    else {
        return Ok(ClaimOutcome::NoTask);
    };

    if let Some(assignment) =
        active_assignment_for_task(&transaction, &task.id).map_err(|error| error.to_string())?
    {
        return Ok(ClaimOutcome::Existing(assignment));
    }
    if !matches!(task.status.as_str(), "queued" | "claimed") {
        return Err(format!(
            "task `{}` is `{}`; only queued or claimed tasks can be assigned",
            task.id, task.status
        ));
    }
    if task.status == "queued" {
        let updated = transaction
            .execute(
                r#"
                UPDATE tasks
                SET status = 'claimed',
                    claimed_by = ?2,
                    claimed_at = COALESCE(claimed_at, CURRENT_TIMESTAMP),
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1 AND status = 'queued'
                "#,
                params![task.id, claimant],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err(format!("task `{}` could not be claimed", task.id));
        }
        insert_task_event(
            &transaction,
            &task.id,
            "task_claimed",
            &format!("Task claimed by `{claimant}` for worker assignment."),
            Some(json!({
                "worker": task.worker.as_deref(),
                "claimed_by": claimant,
                "status": "claimed"
            })),
        )
        .map_err(|error| error.to_string())?;
    }
    let claimed = select_task(&transaction, &task.id).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(ClaimOutcome::Claimed(claimed))
}

pub(super) fn insert_task_event(
    connection: &rusqlite::Connection,
    task_id: &str,
    event_type: &str,
    summary: &str,
    payload: Option<Value>,
) -> rusqlite::Result<TaskEventRecord> {
    let sequence = next_sequence(connection, task_id)?;
    let payload_json = payload
        .map(|payload| serde_json::to_string(&payload).unwrap_or_else(|_| "null".to_string()));
    connection.execute(
        r#"
        INSERT INTO task_events(task_id, sequence, event_type, summary, payload_json)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
        params![task_id, sequence, event_type, summary, payload_json],
    )?;
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

pub(super) fn insert_assignment(
    connection: &rusqlite::Connection,
    task: &TaskRecord,
    worktree: &WorktreeData,
    bundle: &TaskBundle,
    assigned_by: Option<&str>,
) -> Result<WorkerAssignment, String> {
    let id = next_assignment_id(connection, &task.id).map_err(|error| error.to_string())?;
    let bundle_json = serde_json::to_string(bundle).map_err(|error| error.to_string())?;
    connection
        .execute(
            r#"
            INSERT INTO worker_assignments(
                id, task_id, worker, status, assigned_by, worktree_path, bundle_json
            )
            VALUES (?1, ?2, ?3, 'prepared', ?4, ?5, ?6)
            "#,
            params![
                id,
                task.id,
                task.worker,
                assigned_by,
                worktree.path,
                bundle_json
            ],
        )
        .map_err(|error| error.to_string())?;
    load_assignment(connection, &id).map_err(|error| error.to_string())
}

pub(super) fn load_assignment(
    connection: &rusqlite::Connection,
    assignment_id: &str,
) -> rusqlite::Result<WorkerAssignment> {
    connection.query_row(
        r#"
        SELECT id, task_id, worker, status, assigned_by, worktree_path, bundle_json,
               worker_session, started_at, completed_at, result_status, summary,
               changed_files_json, verification_status, created_at, updated_at
        FROM worker_assignments
        WHERE id = ?1
        "#,
        [assignment_id],
        row_to_assignment,
    )
}

fn active_assignment_for_task(
    connection: &rusqlite::Connection,
    task_id: &str,
) -> rusqlite::Result<Option<WorkerAssignment>> {
    connection
        .query_row(
            r#"
            SELECT id, task_id, worker, status, assigned_by, worktree_path, bundle_json,
                   worker_session, started_at, completed_at, result_status, summary,
                   changed_files_json, verification_status, created_at, updated_at
            FROM worker_assignments
            WHERE task_id = ?1 AND status IN ('prepared', 'running')
            ORDER BY created_at ASC, id ASC
            LIMIT 1
            "#,
            [task_id],
            row_to_assignment,
        )
        .optional()
}

fn select_task(connection: &rusqlite::Connection, task_id: &str) -> rusqlite::Result<TaskRecord> {
    connection.query_row(
        r#"
        SELECT id, source_item_id, title, status, worker, claimed_by, claimed_at, started_at,
               finished_at, workspace_path, workspace_branch, workspace_base_ref, created_at,
               updated_at
        FROM tasks
        WHERE id = ?1
        "#,
        [task_id],
        row_to_task,
    )
}

fn select_task_optional(
    connection: &rusqlite::Connection,
    task_id: &str,
) -> rusqlite::Result<Option<TaskRecord>> {
    connection
        .query_row(
            r#"
            SELECT id, source_item_id, title, status, worker, claimed_by, claimed_at, started_at,
                   finished_at, workspace_path, workspace_branch, workspace_base_ref, created_at,
                   updated_at
            FROM tasks
            WHERE id = ?1
            "#,
            [task_id],
            row_to_task,
        )
        .optional()
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

fn next_assignment_id(
    connection: &rusqlite::Connection,
    task_id: &str,
) -> rusqlite::Result<String> {
    let existing: i64 = connection.query_row(
        "SELECT COUNT(*) FROM worker_assignments WHERE task_id = ?1",
        [task_id],
        |row| row.get(0),
    )?;
    Ok(format!("{task_id}-A{:03}", existing + 1))
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

fn row_to_task_event(row: &Row<'_>) -> rusqlite::Result<TaskEventRecord> {
    let payload_json: Option<String> = row.get("payload_json")?;
    let payload = payload_json
        .as_deref()
        .and_then(|payload| serde_json::from_str(payload).ok());
    Ok(TaskEventRecord {
        task_id: row.get("task_id")?,
        sequence: row.get("sequence")?,
        event_type: row.get("event_type")?,
        summary: row.get("summary")?,
        payload,
        created_at: row.get("created_at")?,
        replay_order: 0,
    })
}

fn row_to_assignment(row: &Row<'_>) -> rusqlite::Result<WorkerAssignment> {
    let bundle_json: String = row.get("bundle_json")?;
    let bundle: TaskBundle = serde_json::from_str(&bundle_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            bundle_json.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;
    let changed_files_json: Option<String> = row.get("changed_files_json")?;
    let changed_files = changed_files_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                changed_files_json.as_deref().unwrap_or_default().len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?
        .unwrap_or_default();
    Ok(WorkerAssignment {
        id: row.get("id")?,
        task_id: row.get("task_id")?,
        worker: row.get("worker")?,
        status: row.get("status")?,
        assigned_by: row.get("assigned_by")?,
        worktree_path: row.get("worktree_path")?,
        bundle,
        worker_session: row.get("worker_session")?,
        started_at: row.get("started_at")?,
        completed_at: row.get("completed_at")?,
        result_status: row.get("result_status")?,
        summary: row.get("summary")?,
        changed_files,
        verification_status: row.get("verification_status")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}
