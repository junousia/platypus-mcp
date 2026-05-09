use crate::{
    models::{
        ActionResult, ActionStatus, CompleteWorkerExecutionParams, InspectWorkerAssignmentParams,
        PrepareWorkerAssignmentParams, RecordWorkerEventParams, StartWorkerExecutionParams,
        TaskEventRecord, WorkerAssignment, WorkerAssignmentData, WorkerAssignmentEventData,
    },
    state::{
        sqlite::SqliteProjectState, AppendWorkerEventCommand, AssignmentLifecycleState,
        AssignmentQuery, AssignmentSnapshot, CompleteExecutionCommand, PrepareAssignmentCommand,
        ProjectState, ProjectStateError, StartExecutionCommand, WorkerEventSnapshot,
    },
};
use serde_json::Value;
use std::path::Path;

pub(crate) mod store;
pub(crate) mod validation;

use validation::{
    clean_changed_files, clean_event_type, clean_optional, clean_required, clean_terminal_status,
};

const DEFAULT_CLAIMANT: &str = "external-worker";

pub fn prepare_worker_assignment(
    default_root: &Path,
    params: PrepareWorkerAssignmentParams,
) -> ActionResult<WorkerAssignmentData> {
    let action = "prepare_worker_assignment";
    let claimant = clean_optional(params.claimant)
        .or_else(|| clean_optional(params.worker.clone()))
        .unwrap_or_else(|| DEFAULT_CLAIMANT.to_string());
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    match state.prepare_assignment(PrepareAssignmentCommand {
        task_id: clean_optional(params.task_id),
        worker: clean_optional(params.worker),
        claimant,
        base_ref: params.base_ref,
        verification_command: params.verification_command,
    }) {
        Ok(snapshot) if snapshot.reused_existing => ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: format!(
                "Task `{}` already has active assignment `{}`.",
                snapshot.task_id, snapshot.id
            ),
            next_action: Some(
                "Use inspect_worker_assignment, then start or complete the existing assignment."
                    .to_string(),
            ),
            data: Some(WorkerAssignmentData {
                root: state.root().display().to_string(),
                assignment: worker_assignment(snapshot),
            }),
            error: None,
        },
        Ok(snapshot) => ActionResult::completed(
            action,
            format!("Prepared worker assignment `{}`.", snapshot.id),
            WorkerAssignmentData {
                root: state.root().display().to_string(),
                assignment: worker_assignment(snapshot),
            },
        ),
        Err(ProjectStateError::NotFound { .. }) => ActionResult::skipped(
            action,
            "No queued task is available for assignment.",
            "Dispatch runnable backlog work first.",
        ),
        Err(error) if error.to_string().contains("worktree") => ActionResult::failed(
            action,
            "Could not prepare task worktree.",
            error.to_string(),
        ),
        Err(error) if error.to_string().contains("bundle") => ActionResult::failed(
            action,
            "Could not generate assignment bundle.",
            error.to_string(),
        ),
        Err(error) => state_error(action, "Could not prepare worker assignment.", error),
    }
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
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    match state.inspect_assignment(AssignmentQuery { assignment_id }) {
        Ok(snapshot) => ActionResult::completed(
            action,
            format!("Inspected worker assignment `{}`.", snapshot.id),
            WorkerAssignmentData {
                root: state.root().display().to_string(),
                assignment: worker_assignment(snapshot),
            },
        ),
        Err(ProjectStateError::NotFound { .. }) => ActionResult::skipped(
            action,
            format!(
                "Worker assignment `{}` was not found.",
                params.assignment_id
            ),
            "Prepare an assignment before inspecting it.",
        ),
        Err(error) => state_error(action, "Could not inspect assignment.", error),
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
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    match state.start_execution(StartExecutionCommand {
        assignment_id,
        worker_session,
    }) {
        Ok(snapshot) => ActionResult::completed(
            action,
            format!("Started worker assignment `{}`.", snapshot.id),
            WorkerAssignmentData {
                root: state.root().display().to_string(),
                assignment: worker_assignment(snapshot),
            },
        ),
        Err(ProjectStateError::NotFound { .. }) => ActionResult::skipped(
            action,
            format!(
                "Worker assignment `{}` was not found.",
                params.assignment_id
            ),
            "Prepare a worker assignment before starting execution.",
        ),
        Err(ProjectStateError::Conflict { message }) => {
            ActionResult::skipped(action, message, "Only prepared assignments can be started.")
        }
        Err(error) => state_error(action, "Could not start worker execution.", error),
    }
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
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    match state.append_worker_event(AppendWorkerEventCommand {
        assignment_id: assignment_id.clone(),
        event_type,
        summary,
        payload: params.payload,
    }) {
        Ok(snapshot) => ActionResult::completed(
            action,
            format!("Recorded worker event for assignment `{assignment_id}`."),
            WorkerAssignmentEventData {
                root: state.root().display().to_string(),
                assignment_id,
                task_id: snapshot.task_id.clone(),
                event: task_event_record(snapshot),
            },
        ),
        Err(ProjectStateError::NotFound { .. }) => ActionResult::skipped(
            action,
            format!("Worker assignment `{assignment_id}` was not found."),
            "Prepare and start an assignment before recording worker events.",
        ),
        Err(ProjectStateError::Conflict { message }) => ActionResult::skipped(
            action,
            message,
            "Record worker progress only after start_worker_execution.",
        ),
        Err(error) => state_error(action, "Could not record worker event.", error),
    }
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
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    let snapshot = match state.complete_execution(CompleteExecutionCommand {
        assignment_id: assignment_id.clone(),
        status: status.clone(),
        summary,
        changed_files,
        verification_status: verification_status.clone(),
    }) {
        Ok(snapshot) => snapshot,
        Err(ProjectStateError::NotFound { .. }) => {
            return ActionResult::skipped(
                action,
                format!("Worker assignment `{assignment_id}` was not found."),
                "Prepare and start an assignment before completing execution.",
            )
        }
        Err(ProjectStateError::Conflict { message }) => {
            return ActionResult::skipped(
                action,
                message,
                "Only running assignments can be completed.",
            )
        }
        Err(ProjectStateError::InvalidCommand { message })
            if message.contains("outside owned surfaces") =>
        {
            return ActionResult::failed(action, "Worker result touched unowned files.", message)
        }
        Err(error) => return state_error(action, "Could not complete worker execution.", error),
    };
    let mut result = ActionResult::completed(
        action,
        format!("Completed worker assignment `{}`.", snapshot.id),
        WorkerAssignmentData {
            root: state.root().display().to_string(),
            assignment: worker_assignment(snapshot),
        },
    );
    if status == "completed" && verification_status.as_deref() != Some("passed") {
        result.next_action = Some(
            "Record verification evidence with record_verification_evidence when useful, or integrate with allow_unverified=true when workflow policy permits."
                .to_string(),
        );
    }
    result
}

fn state_error<T: schemars::JsonSchema + serde::Serialize>(
    action: &str,
    summary: &str,
    error: ProjectStateError,
) -> ActionResult<T> {
    ActionResult::failed(action, summary, error.to_string())
}

fn worker_assignment(snapshot: AssignmentSnapshot) -> WorkerAssignment {
    WorkerAssignment {
        id: snapshot.id,
        task_id: snapshot.task_id,
        worker: snapshot.worker,
        status: assignment_status(&snapshot.state).to_string(),
        assigned_by: snapshot.assigned_by,
        worktree_path: snapshot.worktree_path,
        bundle: snapshot.bundle,
        worker_session: snapshot.worker_session,
        started_at: snapshot.started_at,
        completed_at: snapshot.completed_at,
        result_status: snapshot.result_status,
        summary: snapshot.summary,
        changed_files: snapshot.changed_files,
        verification_status: snapshot.verification_status,
        created_at: snapshot.created_at,
        updated_at: snapshot.updated_at,
    }
}

fn assignment_status(state: &AssignmentLifecycleState) -> &'static str {
    match state {
        AssignmentLifecycleState::Prepared => "prepared",
        AssignmentLifecycleState::Running => "running",
        AssignmentLifecycleState::Completed => "completed",
        AssignmentLifecycleState::Failed => "failed",
        AssignmentLifecycleState::Cancelled => "cancelled",
    }
}

fn task_event_record(snapshot: WorkerEventSnapshot) -> TaskEventRecord {
    TaskEventRecord {
        task_id: snapshot.task_id,
        sequence: snapshot.sequence,
        event_type: snapshot.event_type,
        summary: snapshot.summary,
        payload: Some(Value::Object(snapshot.payload.into_iter().collect())),
        created_at: snapshot.created_at,
        replay_order: 0,
    }
}

#[cfg(test)]
mod tests;
