use crate::{
    bundle,
    models::{
        ActionResult, ActionStatus, CompleteWorkerExecutionParams, GenerateTaskBundleParams,
        InspectWorkerAssignmentParams, PrepareWorkerAssignmentParams, RecordWorkerEventParams,
        StartWorkerExecutionParams, TaskBundleData, WorkerAssignmentData,
        WorkerAssignmentEventData, WorktreeCreateParams,
    },
    storage,
    tasks::{self, NewTaskEvent},
    workspace,
};
use rusqlite::params;
use serde_json::{json, Value};
use std::path::Path;

mod store;
mod validation;

use store::{
    claim_task_for_assignment, insert_assignment, insert_task_event, load_assignment, ClaimOutcome,
};
use validation::{
    clean_changed_files, clean_event_type, clean_optional, clean_required, clean_terminal_status,
    validate_changed_files,
};

const DEFAULT_CLAIMANT: &str = "external-worker";

pub fn prepare_worker_assignment(
    default_root: &Path,
    params: PrepareWorkerAssignmentParams,
) -> ActionResult<WorkerAssignmentData> {
    let action = "prepare_worker_assignment";
    let claimant = clean_optional(params.claimant.clone())
        .or_else(|| clean_optional(params.worker.clone()))
        .unwrap_or_else(|| DEFAULT_CLAIMANT.to_string());
    let worker_filter = clean_optional(params.worker.clone());
    let task_id = clean_optional(params.task_id.clone());

    let mut storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open assignment storage.",
                error.to_string(),
            )
        }
    };
    let root = storage.storage.root.clone();
    let root_string = root.display().to_string();

    let task = match claim_task_for_assignment(
        &mut storage.connection,
        task_id.as_deref(),
        worker_filter.as_deref(),
        &claimant,
    ) {
        Ok(ClaimOutcome::Claimed(task)) => task,
        Ok(ClaimOutcome::Existing(assignment)) => return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: format!(
                "Task `{}` already has active assignment `{}`.",
                assignment.task_id, assignment.id
            ),
            next_action: Some(
                "Use inspect_worker_assignment, then start or complete the existing assignment."
                    .to_string(),
            ),
            data: Some(WorkerAssignmentData {
                root: root_string,
                assignment,
            }),
            error: None,
        },
        Ok(ClaimOutcome::NoTask) => {
            return ActionResult::skipped(
                action,
                "No queued task is available for assignment.",
                "Dispatch runnable backlog work first.",
            )
        }
        Err(error) => {
            return ActionResult::failed(action, "Could not claim task for assignment.", error)
        }
    };
    drop(storage);

    let worktree = match workspace::worktree_create(
        default_root,
        WorktreeCreateParams {
            root: Some(root_string.clone()),
            task_id: task.id.clone(),
            base_ref: params.base_ref,
        },
    ) {
        ActionResult {
            status: ActionStatus::Completed | ActionStatus::Skipped,
            data: Some(data),
            ..
        } => data,
        ActionResult { error, summary, .. } => {
            return ActionResult::failed(
                action,
                "Could not prepare task worktree.",
                error.unwrap_or(summary),
            )
        }
    };

    let bundle = match bundle::generate_task_bundle(
        default_root,
        GenerateTaskBundleParams {
            root: Some(root_string.clone()),
            task_id: task.id.clone(),
            verification_command: params.verification_command,
        },
    ) {
        ActionResult {
            status: ActionStatus::Completed,
            data: Some(TaskBundleData { bundle, .. }),
            ..
        } => bundle,
        ActionResult { error, summary, .. } => {
            return ActionResult::failed(
                action,
                "Could not generate assignment bundle.",
                error.unwrap_or(summary),
            )
        }
    };

    let storage = match storage::connect(default_root, Some(root_string.as_str())) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not reopen assignment storage.",
                error.to_string(),
            )
        }
    };
    let assignment = match insert_assignment(
        &storage.connection,
        &task,
        &worktree,
        &bundle,
        Some(claimant.as_str()),
    ) {
        Ok(assignment) => assignment,
        Err(error) => {
            return ActionResult::failed(action, "Could not persist worker assignment.", error)
        }
    };
    if let Err(error) = tasks::record_task_event(
        default_root,
        Some(root_string.as_str()),
        NewTaskEvent {
            task_id: task.id.clone(),
            sequence: None,
            event_type: "worker_assignment_prepared".to_string(),
            summary: format!("Prepared worker assignment `{}`.", assignment.id),
            payload: Some(json!({
                "assignment_id": assignment.id,
                "worker": assignment.worker,
                "worktree_path": assignment.worktree_path
            })),
        },
    ) {
        return ActionResult::failed(action, "Could not record assignment event.", error);
    }

    ActionResult::completed(
        action,
        format!("Prepared worker assignment `{}`.", assignment.id),
        WorkerAssignmentData {
            root: root_string,
            assignment,
        },
    )
}

pub fn inspect_worker_assignment(
    default_root: &Path,
    params: InspectWorkerAssignmentParams,
) -> ActionResult<WorkerAssignmentData> {
    let action = "inspect_worker_assignment";
    let assignment_id = match clean_required("assignment_id", &params.assignment_id) {
        Ok(id) => id,
        Err(error) => return ActionResult::failed(action, "Could not inspect assignment.", error),
    };
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open assignment storage.",
                error.to_string(),
            )
        }
    };
    match load_assignment(&storage.connection, &assignment_id) {
        Ok(assignment) => ActionResult::completed(
            action,
            format!("Inspected worker assignment `{assignment_id}`."),
            WorkerAssignmentData {
                root: storage.storage.root.display().to_string(),
                assignment,
            },
        ),
        Err(rusqlite::Error::QueryReturnedNoRows) => ActionResult::skipped(
            action,
            format!("Worker assignment `{assignment_id}` was not found."),
            "Prepare an assignment before inspecting it.",
        ),
        Err(error) => {
            ActionResult::failed(action, "Could not inspect assignment.", error.to_string())
        }
    }
}

pub fn start_worker_execution(
    default_root: &Path,
    params: StartWorkerExecutionParams,
) -> ActionResult<WorkerAssignmentData> {
    let action = "start_worker_execution";
    let assignment_id = match clean_required("assignment_id", &params.assignment_id) {
        Ok(id) => id,
        Err(error) => {
            return ActionResult::failed(action, "Could not start worker execution.", error)
        }
    };
    let worker_session = clean_optional(params.worker_session);
    let mut storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open assignment storage.",
                error.to_string(),
            )
        }
    };
    let root = storage.storage.root.display().to_string();
    let transaction = match storage.connection.transaction() {
        Ok(transaction) => transaction,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not begin assignment start.",
                error.to_string(),
            )
        }
    };
    let assignment = match load_assignment(&transaction, &assignment_id) {
        Ok(assignment) => assignment,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return ActionResult::skipped(
                action,
                format!("Worker assignment `{assignment_id}` was not found."),
                "Prepare a worker assignment before starting execution.",
            )
        }
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect assignment.", error.to_string())
        }
    };
    if assignment.status != "prepared" {
        return ActionResult::skipped(
            action,
            format!(
                "Worker assignment `{assignment_id}` is `{}`.",
                assignment.status
            ),
            "Only prepared assignments can be started.",
        );
    }

    let task_updated = match transaction.execute(
        r#"
        UPDATE tasks
        SET status = 'running',
            started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1 AND status IN ('claimed', 'running')
        "#,
        [assignment.task_id.as_str()],
    ) {
        Ok(updated) => updated,
        Err(error) => {
            return ActionResult::failed(action, "Could not mark task running.", error.to_string())
        }
    };
    if task_updated == 0 {
        return ActionResult::failed(
            action,
            "Could not mark task running.",
            format!(
                "task `{}` must be claimed before the assignment can start",
                assignment.task_id
            ),
        );
    }
    let assignment_updated = match transaction.execute(
        r#"
        UPDATE worker_assignments
        SET status = 'running',
            worker_session = ?2,
            started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1 AND status = 'prepared'
        "#,
        params![assignment_id, worker_session],
    ) {
        Ok(updated) => updated,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not persist assignment start.",
                error.to_string(),
            )
        }
    };
    if assignment_updated == 0 {
        return ActionResult::failed(
            action,
            "Could not persist assignment start.",
            format!("assignment `{assignment_id}` was not prepared"),
        );
    }
    if let Err(error) = insert_task_event(
        &transaction,
        &assignment.task_id,
        "worker_started",
        &format!("Started worker assignment `{assignment_id}`."),
        Some(json!({
            "assignment_id": assignment_id,
            "worker": assignment.worker,
            "worker_session": worker_session
        })),
    ) {
        return ActionResult::failed(
            action,
            "Could not record worker start event.",
            error.to_string(),
        );
    }
    let assignment = match load_assignment(&transaction, &params.assignment_id) {
        Ok(assignment) => assignment,
        Err(error) => {
            return ActionResult::failed(action, "Could not reload assignment.", error.to_string())
        }
    };
    if let Err(error) = transaction.commit() {
        return ActionResult::failed(
            action,
            "Could not commit assignment start.",
            error.to_string(),
        );
    }
    ActionResult::completed(
        action,
        format!("Started worker assignment `{}`.", assignment.id),
        WorkerAssignmentData { root, assignment },
    )
}

pub fn record_worker_event(
    default_root: &Path,
    params: RecordWorkerEventParams,
) -> ActionResult<WorkerAssignmentEventData> {
    let action = "record_worker_event";
    let assignment_id = match clean_required("assignment_id", &params.assignment_id) {
        Ok(id) => id,
        Err(error) => return ActionResult::failed(action, "Could not record worker event.", error),
    };
    let event_type = match clean_event_type(&params.event_type) {
        Ok(event_type) => event_type,
        Err(error) => return ActionResult::failed(action, "Could not record worker event.", error),
    };
    let summary = match clean_required("summary", &params.summary) {
        Ok(summary) => summary,
        Err(error) => return ActionResult::failed(action, "Could not record worker event.", error),
    };
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open assignment storage.",
                error.to_string(),
            )
        }
    };
    let root = storage.storage.root.display().to_string();
    let assignment = match load_assignment(&storage.connection, &assignment_id) {
        Ok(assignment) => assignment,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return ActionResult::skipped(
                action,
                format!("Worker assignment `{assignment_id}` was not found."),
                "Prepare and start an assignment before recording worker events.",
            )
        }
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect assignment.", error.to_string())
        }
    };
    if assignment.status != "running" {
        return ActionResult::skipped(
            action,
            format!(
                "Worker assignment `{assignment_id}` is `{}`.",
                assignment.status
            ),
            "Record worker progress only after start_worker_execution.",
        );
    }
    let payload = if params.payload.is_empty() {
        json!({ "assignment_id": assignment_id })
    } else {
        json!({
            "assignment_id": assignment_id,
            "payload": Value::Object(params.payload.into_iter().collect())
        })
    };
    let event = match tasks::record_task_event(
        default_root,
        Some(root.as_str()),
        NewTaskEvent {
            task_id: assignment.task_id.clone(),
            sequence: None,
            event_type,
            summary: summary.clone(),
            payload: Some(payload),
        },
    ) {
        Ok(event) => event,
        Err(error) => {
            return ActionResult::failed(action, "Could not persist worker event.", error)
        }
    };
    ActionResult::completed(
        action,
        format!("Recorded worker event for assignment `{assignment_id}`."),
        WorkerAssignmentEventData {
            root,
            assignment_id,
            task_id: assignment.task_id,
            event,
        },
    )
}

pub fn complete_worker_execution(
    default_root: &Path,
    params: CompleteWorkerExecutionParams,
) -> ActionResult<WorkerAssignmentData> {
    let action = "complete_worker_execution";
    let assignment_id = match clean_required("assignment_id", &params.assignment_id) {
        Ok(id) => id,
        Err(error) => {
            return ActionResult::failed(action, "Could not complete worker execution.", error)
        }
    };
    let status = match clean_terminal_status(&params.status) {
        Ok(status) => status,
        Err(error) => {
            return ActionResult::failed(action, "Could not complete worker execution.", error)
        }
    };
    let summary = match clean_required("summary", &params.summary) {
        Ok(summary) => summary,
        Err(error) => {
            return ActionResult::failed(action, "Could not complete worker execution.", error)
        }
    };
    let changed_files = match clean_changed_files(params.changed_files) {
        Ok(files) => files,
        Err(error) => {
            return ActionResult::failed(action, "Could not complete worker execution.", error)
        }
    };
    let verification_status = clean_optional(params.verification_status);
    if status == "completed" && verification_status.is_none() {
        return ActionResult::failed(
            action,
            "Could not complete worker execution.",
            "verification_status is required when status is completed; use passed, failed, skipped, or not_run",
        );
    }
    let mut storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open assignment storage.",
                error.to_string(),
            )
        }
    };
    let root = storage.storage.root.display().to_string();
    let transaction = match storage.connection.transaction() {
        Ok(transaction) => transaction,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not begin assignment completion.",
                error.to_string(),
            )
        }
    };
    let assignment = match load_assignment(&transaction, &assignment_id) {
        Ok(assignment) => assignment,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return ActionResult::skipped(
                action,
                format!("Worker assignment `{assignment_id}` was not found."),
                "Prepare and start an assignment before completing execution.",
            )
        }
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect assignment.", error.to_string())
        }
    };
    if assignment.status != "running" {
        return ActionResult::skipped(
            action,
            format!(
                "Worker assignment `{assignment_id}` is `{}`.",
                assignment.status
            ),
            "Only running assignments can be completed.",
        );
    }
    if let Err(error) = validate_changed_files(&assignment.bundle.owned_surfaces, &changed_files) {
        return ActionResult::failed(action, "Worker result touched unowned files.", error);
    }
    let changed_files_json = match serde_json::to_string(&changed_files) {
        Ok(json) => json,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not encode result files.",
                error.to_string(),
            )
        }
    };
    let new_assignment_status = match status.as_str() {
        "completed" => "completed",
        "failed" => "failed",
        "cancelled" => "cancelled",
        _ => unreachable!("status validated"),
    };
    let assignment_updated = match transaction.execute(
        r#"
        UPDATE worker_assignments
        SET status = ?2,
            completed_at = CURRENT_TIMESTAMP,
            result_status = ?3,
            summary = ?4,
            changed_files_json = ?5,
            verification_status = ?6,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1 AND status = 'running'
        "#,
        params![
            assignment_id,
            new_assignment_status,
            status,
            summary,
            changed_files_json,
            verification_status
        ],
    ) {
        Ok(updated) => updated,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not persist assignment completion.",
                error.to_string(),
            )
        }
    };
    if assignment_updated == 0 {
        return ActionResult::failed(
            action,
            "Could not persist assignment completion.",
            format!("assignment `{assignment_id}` was not running"),
        );
    }
    if let Err(error) = insert_task_event(
        &transaction,
        &assignment.task_id,
        "worker_result",
        &format!("Worker assignment `{assignment_id}` finished with `{status}`."),
        Some(json!({
            "assignment_id": assignment_id,
            "status": status,
            "summary": summary,
            "changed_files": changed_files,
            "verification_status": verification_status
        })),
    ) {
        return ActionResult::failed(
            action,
            "Could not record worker result event.",
            error.to_string(),
        );
    }
    let task_updated = match transaction.execute(
        r#"
        UPDATE tasks
        SET status = ?2,
            finished_at = CURRENT_TIMESTAMP,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1 AND status IN ('claimed', 'running')
        "#,
        params![assignment.task_id, status],
    ) {
        Ok(updated) => updated,
        Err(error) => {
            return ActionResult::failed(action, "Could not finish task.", error.to_string())
        }
    };
    if task_updated == 0 {
        return ActionResult::failed(
            action,
            "Could not finish task.",
            format!(
                "task `{}` must be claimed or running before completion",
                assignment.task_id
            ),
        );
    }
    let assignment = match load_assignment(&transaction, &params.assignment_id) {
        Ok(assignment) => assignment,
        Err(error) => {
            return ActionResult::failed(action, "Could not reload assignment.", error.to_string())
        }
    };
    if let Err(error) = transaction.commit() {
        return ActionResult::failed(
            action,
            "Could not commit assignment completion.",
            error.to_string(),
        );
    }
    let mut result = ActionResult::completed(
        action,
        format!("Completed worker assignment `{}`.", assignment.id),
        WorkerAssignmentData { root, assignment },
    );
    if status == "completed" && verification_status.as_deref() != Some("passed") {
        result.next_action = Some(
            "Record verification evidence with record_verification_evidence, or rerun verification before claiming the work is reconciled."
                .to_string(),
        );
    }
    result
}

#[cfg(test)]
mod tests;
