use crate::{
    evidence,
    models::{
        ActionResult, ActionStatus, CompleteWorkerExecutionParams, InspectWorkerAssignmentParams,
        PrepareWorkerAssignmentParams, RecordVerificationEvidenceParams, RecordWorkerEventParams,
        RunTaskVerificationParams, StartWorkerExecutionParams, TaskEventRecord,
        TaskVerificationRunData, WorkerAssignment, WorkerAssignmentData, WorkerAssignmentEventData,
    },
    state::{
        sqlite::SqliteProjectState, AppendWorkerEventCommand, AssignmentLifecycleState,
        AssignmentQuery, AssignmentSnapshot, CompleteExecutionCommand, PrepareAssignmentCommand,
        ProjectEventSnapshot, ProjectState, ProjectStateError, ReplayEventsQuery,
        StartExecutionCommand, TaskQuery, WorkerEventSnapshot,
    },
    tasks,
    workers::run_harness_process,
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    time::Duration,
};

pub(crate) mod store;
pub(crate) mod validation;

use validation::{
    clean_changed_files, clean_event_type, clean_optional, clean_required, clean_terminal_status,
};

const DEFAULT_CLAIMANT: &str = "external-worker";
const DEFAULT_VERIFICATION_TIMEOUT_SECONDS: u64 = 60;
const MAX_CAPTURE_BYTES: usize = 8192;
const ALLOWED_VERIFICATION_EXECUTABLES: &[&str] =
    &["make", "cargo", "npm", "pnpm", "yarn", "bun", "deno", "uv"];

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
        Ok(snapshot) => {
            let assignment = worker_assignment(snapshot);
            if let Err(error) = ensure_owned_surface_dirs(&assignment) {
                return ActionResult::failed(
                    action,
                    "Could not prepare owned-surface directories.",
                    error,
                );
            }
            ActionResult::completed(
                action,
                format!("Prepared worker assignment `{}`.", assignment.id),
                WorkerAssignmentData {
                    root: state.root().display().to_string(),
                    assignment,
                },
            )
        }
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

fn ensure_owned_surface_dirs(assignment: &WorkerAssignment) -> Result<(), String> {
    let worktree = Path::new(&assignment.worktree_path);
    let canonical_worktree = fs::canonicalize(worktree).map_err(|error| {
        format!(
            "could not resolve assignment worktree `{}`: {error}",
            worktree.display()
        )
    })?;
    for surface in &assignment.bundle.owned_surfaces {
        let Some(create_target) = owned_surface_create_target(worktree, surface)? else {
            continue;
        };
        ensure_create_target_inside_worktree(worktree, &canonical_worktree, &create_target)?;
        if let Err(error) = fs::create_dir_all(&create_target) {
            return Err(format!(
                "could not create owned surface directory `{}`: {error}",
                create_target.display()
            ));
        }
        ensure_create_target_inside_worktree(worktree, &canonical_worktree, &create_target)?;
    }
    Ok(())
}

fn owned_surface_create_target(worktree: &Path, surface: &str) -> Result<Option<PathBuf>, String> {
    let trimmed = surface.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Ok(None);
    }
    let relative = Path::new(trimmed);
    if relative.is_absolute() {
        return Err(format!(
            "owned surface `{trimmed}` must be relative to the assignment worktree"
        ));
    }
    for component in relative.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(format!(
                "owned surface `{trimmed}` must stay inside the assignment worktree"
            ));
        }
    }
    let target = worktree.join(relative);
    let create_target = if trimmed.ends_with('/') {
        target
    } else if relative.extension().is_some() {
        target
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| worktree.to_path_buf())
    } else {
        target
    };
    Ok(Some(create_target))
}

fn ensure_create_target_inside_worktree(
    worktree: &Path,
    canonical_worktree: &Path,
    create_target: &Path,
) -> Result<(), String> {
    let existing = nearest_existing_ancestor(create_target, worktree)?;
    let canonical_existing = fs::canonicalize(&existing).map_err(|error| {
        format!(
            "could not resolve owned surface ancestor `{}`: {error}",
            existing.display()
        )
    })?;
    if !canonical_existing.starts_with(canonical_worktree) {
        return Err(format!(
            "owned surface directory `{}` escapes the assignment worktree",
            create_target.display()
        ));
    }
    Ok(())
}

fn nearest_existing_ancestor(path: &Path, floor: &Path) -> Result<PathBuf, String> {
    let mut current = path.to_path_buf();
    loop {
        if current.exists() {
            return Ok(current);
        }
        if current == floor {
            return Ok(current);
        }
        let Some(parent) = current.parent() else {
            return Err(format!(
                "owned surface path `{}` does not have a valid parent",
                path.display()
            ));
        };
        current = parent.to_path_buf();
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
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    let assignment_id = match resolve_assignment_id(
        &state,
        params.assignment_id.as_deref(),
        params.task_id.as_deref(),
        &[AssignmentLifecycleState::Prepared],
        "prepared",
    ) {
        Ok(id) => id,
        Err(error) => {
            return ActionResult::failed(action, "Could not start worker execution.", error)
        }
    };
    let worker_session = clean_optional(params.worker_session);
    match state.start_execution(StartExecutionCommand {
        assignment_id: assignment_id.clone(),
        worker_session,
    }) {
        Ok(snapshot) => ActionResult {
            action: action.to_string(),
            status: ActionStatus::Completed,
            summary: format!("Started worker assignment `{}`.", snapshot.id),
            next_action: Some(format!(
                "Run edits inside `{}` (the assignment worktree), then record progress or complete the task.",
                snapshot.worktree_path
            )),
            data: Some(WorkerAssignmentData {
                root: state.root().display().to_string(),
                assignment: worker_assignment(snapshot),
            }),
            error: None,
        },
        Err(ProjectStateError::NotFound { .. }) => ActionResult::skipped(
            action,
            format!("Worker assignment `{assignment_id}` was not found."),
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
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    let assignment_id = match resolve_assignment_id(
        &state,
        params.assignment_id.as_deref(),
        params.task_id.as_deref(),
        &[
            AssignmentLifecycleState::Prepared,
            AssignmentLifecycleState::Running,
        ],
        "prepared or running",
    ) {
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
            "Prepared and running assignments both accept worker progress. Start the task first only when your worker needs a running lease.",
        ),
        Err(error) => state_error(action, "Could not record worker event.", error),
    }
}

pub fn complete_worker_execution(
    default_root: &Path,
    params: CompleteWorkerExecutionParams,
) -> ActionResult<WorkerAssignmentData> {
    let action = "complete_worker_execution";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    let allow_prepared_lookup = params.auto_start_if_prepared.unwrap_or(true);
    let allowed_states = if allow_prepared_lookup {
        vec![
            AssignmentLifecycleState::Running,
            AssignmentLifecycleState::Prepared,
        ]
    } else {
        vec![AssignmentLifecycleState::Running]
    };
    let allowed_label = if allow_prepared_lookup {
        "running or prepared"
    } else {
        "running"
    };
    let assignment_id = match resolve_assignment_id(
        &state,
        params.assignment_id.as_deref(),
        params.task_id.as_deref(),
        &allowed_states,
        allowed_label,
    ) {
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
    let assignment_snapshot = match state.inspect_assignment(AssignmentQuery {
        assignment_id: assignment_id.clone(),
    }) {
        Ok(snapshot) => snapshot,
        Err(error) => return state_error(action, "Could not inspect assignment.", error),
    };
    let changed_files =
        match normalize_changed_files(params.changed_files, &assignment_snapshot.worktree_path) {
            Ok(files) => files,
            Err(error) => {
                return ActionResult::failed(action, "Could not complete worker execution.", error)
            }
        };
    let verification_status = if status == "completed" {
        Some(clean_optional(params.verification_status).unwrap_or_else(|| "not_run".to_string()))
    } else {
        clean_optional(params.verification_status)
    };
    let mut completion = state.complete_execution(CompleteExecutionCommand {
        assignment_id: assignment_id.clone(),
        status: status.clone(),
        summary: summary.clone(),
        changed_files: changed_files.clone(),
        verification_status: verification_status.clone(),
    });
    if matches!(completion, Err(ProjectStateError::Conflict { .. }))
        && params.auto_start_if_prepared.unwrap_or(true)
    {
        if let Ok(snapshot) = state.inspect_assignment(AssignmentQuery {
            assignment_id: assignment_id.clone(),
        }) {
            if snapshot.state == AssignmentLifecycleState::Prepared {
                let _ = state.start_execution(StartExecutionCommand {
                    assignment_id: assignment_id.clone(),
                    worker_session: None,
                });
                completion = state.complete_execution(CompleteExecutionCommand {
                    assignment_id: assignment_id.clone(),
                    status: status.clone(),
                    summary,
                    changed_files,
                    verification_status: verification_status.clone(),
                });
            }
        }
    }
    let snapshot = match completion {
        Ok(snapshot) => snapshot,
        Err(ProjectStateError::NotFound { .. }) => {
            return ActionResult {
                action: action.to_string(),
                status: ActionStatus::Failed,
                summary: format!("Worker assignment `{assignment_id}` was not found."),
                next_action: Some(
                    "Prepare and start an assignment before completing execution.".to_string(),
                ),
                data: None,
                error: Some("worker assignment was not found".to_string()),
            }
        }
        Err(ProjectStateError::Conflict { message }) => {
            return ActionResult {
                action: action.to_string(),
                status: ActionStatus::Failed,
                summary: message,
                next_action: Some(
                    "Only running assignments can be completed. Call start_worker_task first, or retry complete_worker_task with auto_start_if_prepared=true when you are finishing in one session.".to_string(),
                ),
                data: None,
                error: Some("assignment is not running".to_string()),
            }
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
        format!(
            "Completed worker assignment `{}`. Verification status `{}` means {}.",
            snapshot.id,
            verification_status.as_deref().unwrap_or("not_recorded"),
            verification_status_explanation(verification_status.as_deref())
        ),
        WorkerAssignmentData {
            root: state.root().display().to_string(),
            assignment: worker_assignment(snapshot),
        },
    );
    if status == "completed" && verification_status.as_deref() != Some("passed") {
        result.next_action = Some(
            "Run verification with run_task_verification for this assignment or task, then record verification evidence when useful, or integrate with allow_unverified=true when workflow policy permits."
                .to_string(),
        );
    }
    result
}

fn verification_status_explanation(status: Option<&str>) -> &'static str {
    match status {
        Some("passed") => "an explicit check passed",
        Some("failed") => "an explicit check failed",
        Some("skipped") => "work was completed but no formal check was run",
        Some("not_run") => "no verification command was run or recorded",
        _ => "no verification status was recorded for this terminal state",
    }
}

fn normalize_changed_files(files: Vec<String>, worktree_path: &str) -> Result<Vec<String>, String> {
    let worktree = PathBuf::from(worktree_path);
    let worktree_tail = worktree_tail(&worktree);
    let mut normalized = Vec::new();
    for file in files {
        let mut candidate = file.trim().replace('\\', "/");
        if candidate.is_empty() {
            continue;
        }
        if Path::new(&candidate).is_absolute() {
            let absolute = PathBuf::from(&candidate);
            if let Ok(stripped) = absolute.strip_prefix(&worktree) {
                candidate = stripped.to_string_lossy().replace('\\', "/");
            } else {
                return Err(
                    "changed_files absolute paths must be inside the assignment worktree"
                        .to_string(),
                );
            }
        } else if let Some(tail) = &worktree_tail {
            let tail_prefix = format!("{tail}/");
            if candidate == *tail {
                candidate.clear();
            } else if candidate.starts_with(&tail_prefix) {
                candidate = candidate[tail_prefix.len()..].to_string();
            }
        }
        if candidate.is_empty() {
            continue;
        }
        let candidate_path = Path::new(&candidate);
        for component in candidate_path.components() {
            if matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            ) {
                return Err("changed_files cannot contain path traversal".to_string());
            }
        }
        normalized.push(candidate);
    }
    clean_changed_files(normalized)
}

fn worktree_tail(worktree: &Path) -> Option<String> {
    let mut components = worktree
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    if components.len() < 3 {
        return None;
    }
    let mut start = None;
    for (index, component) in components.iter().enumerate() {
        if *component == ".platy" && components.get(index + 1) == Some(&"worktrees") {
            start = Some(index);
            break;
        }
    }
    let start = start.unwrap_or(components.len().saturating_sub(3));
    components = components[start..].to_vec();
    Some(components.join("/"))
}

pub fn run_task_verification(
    default_root: &Path,
    params: RunTaskVerificationParams,
) -> ActionResult<TaskVerificationRunData> {
    let action = "run_task_verification";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    let assignment_id = match resolve_assignment_id(
        &state,
        params.assignment_id.as_deref(),
        params.task_id.as_deref(),
        &[
            AssignmentLifecycleState::Prepared,
            AssignmentLifecycleState::Running,
            AssignmentLifecycleState::Completed,
            AssignmentLifecycleState::Failed,
            AssignmentLifecycleState::Cancelled,
        ],
        "prepared, running, completed, failed, or cancelled",
    ) {
        Ok(id) => id,
        Err(error) => {
            return ActionResult::failed(action, "Could not run task verification.", error)
        }
    };
    let snapshot = match state.inspect_assignment(AssignmentQuery {
        assignment_id: assignment_id.clone(),
    }) {
        Ok(snapshot) => snapshot,
        Err(error) => return state_error(action, "Could not inspect assignment.", error),
    };
    let command = executable_command(snapshot.bundle.verification_command.clone());
    if command.is_empty() {
        return ActionResult::completed(
            action,
            format!(
                "Verification command is not configured for assignment `{}`.",
                snapshot.id
            ),
            TaskVerificationRunData {
                root: state.root().display().to_string(),
                assignment_id: snapshot.id,
                task_id: snapshot.task_id,
                verification_command: command,
                status: "skipped".to_string(),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                timed_out: false,
            },
        );
    }
    let executable = command[0].clone();
    if let Err(error) = ensure_verification_command_allowed(&executable) {
        let mut result = ActionResult::failed(
            action,
            "Verification command executable is not allowed.",
            error,
        );
        result.next_action = Some(format!(
            "Use one of the allowed verification executables: {}.",
            ALLOWED_VERIFICATION_EXECUTABLES.join(", ")
        ));
        return result;
    }
    let args = command[1..].to_vec();
    let timeout = Duration::from_secs(
        params
            .timeout_seconds
            .unwrap_or(DEFAULT_VERIFICATION_TIMEOUT_SECONDS)
            .clamp(1, 600),
    );
    let outcome = run_harness_process(
        Path::new(&executable),
        &args,
        &snapshot.worktree_path,
        "",
        timeout,
    );
    let (status, exit_code, stdout_raw, stderr_raw, timed_out) = match outcome {
        Ok(output) => (
            if output.success { "passed" } else { "failed" }.to_string(),
            output.exit_code,
            output.stdout,
            output.stderr,
            false,
        ),
        Err(error) if error.contains("timed out") => {
            ("timed_out".to_string(), None, String::new(), error, true)
        }
        Err(error) => ("failed".to_string(), None, String::new(), error, false),
    };
    let (stdout, stdout_truncated) = truncate_output(stdout_raw, MAX_CAPTURE_BYTES);
    let (stderr, stderr_truncated) = truncate_output(stderr_raw, MAX_CAPTURE_BYTES);

    let task = match state.inspect_task(TaskQuery {
        task_id: snapshot.task_id.clone(),
    }) {
        Ok(task) => task,
        Err(error) => {
            return state_error(action, "Could not inspect task for verification.", error)
        }
    };

    let event_summary = format!("Verification command finished with status `{status}`.");
    let _ = tasks::record_task_event(
        default_root,
        Some(state.root().display().to_string().as_str()),
        tasks::NewTaskEvent {
            task_id: snapshot.task_id.clone(),
            sequence: None,
            event_type: "verification_run".to_string(),
            summary: event_summary.clone(),
            payload: Some(serde_json::json!({
                "assignment_id": snapshot.id,
                "status": status,
                "command": command,
                "timed_out": timed_out
            })),
        },
    );

    if status == "passed" {
        let _ = evidence::record_verification_evidence(
            default_root,
            RecordVerificationEvidenceParams {
                root: Some(state.root().display().to_string()),
                id: None,
                source_item_id: Some(task.source_item_id),
                source_task_id: Some(task.id.clone()),
                summary: event_summary,
                refs: vec![format!("assignment:{}", snapshot.id)],
                metadata: std::collections::BTreeMap::new(),
            },
        );
    }

    let mut result = ActionResult::completed(
        action,
        format!(
            "Verification command finished with `{status}` for assignment `{}`.",
            snapshot.id
        ),
        TaskVerificationRunData {
            root: state.root().display().to_string(),
            assignment_id: snapshot.id,
            task_id: task.id.clone(),
            verification_command: command,
            status: status.clone(),
            exit_code,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            timed_out,
        },
    );
    if status != "passed" {
        result.next_action = Some(
            "Review stdout/stderr and worker changes, then retry run_task_verification or continue with complete_worker_task and integrate_worker_result using allow_unverified=true when policy permits."
                .to_string(),
        );
    }
    result
}

fn truncate_output(value: String, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }
    let mut boundary = max_bytes.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    (value[..boundary].to_string(), true)
}

fn executable_command(values: Vec<String>) -> Vec<String> {
    let values = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if values.len() != 1 {
        return values;
    }
    split_command_line(&values[0]).unwrap_or(values)
}

fn ensure_verification_command_allowed(executable: &str) -> Result<(), String> {
    if executable.is_empty() {
        return Err("verification command executable cannot be empty".to_string());
    }
    if Path::new(executable).is_absolute()
        || executable.contains('/')
        || executable.contains('\\')
        || executable.contains(std::path::MAIN_SEPARATOR)
    {
        return Err(
            "verification command executable must be a bare allowlisted command name".to_string(),
        );
    }
    if ALLOWED_VERIFICATION_EXECUTABLES.contains(&executable) {
        Ok(())
    } else {
        Err(format!(
            "verification command executable `{executable}` is not in the allowlist"
        ))
    }
}

fn split_command_line(value: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut saw_whitespace = false;

    for character in value.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(quote_char) = quote {
            if character == quote_char {
                quote = None;
            } else {
                current.push(character);
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            continue;
        }
        if character.is_whitespace() {
            saw_whitespace = true;
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(character);
    }

    if escaped {
        current.push('\\');
    }
    if quote.is_some() {
        return None;
    }
    if !current.is_empty() {
        args.push(current);
    }
    if saw_whitespace && !args.is_empty() {
        Some(args)
    } else {
        None
    }
}

fn resolve_assignment_id(
    state: &SqliteProjectState,
    assignment_id: Option<&str>,
    task_id: Option<&str>,
    allowed_states: &[AssignmentLifecycleState],
    allowed_label: &str,
) -> Result<String, String> {
    if let Some(value) = assignment_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
    {
        return Ok(value);
    }
    let task_id = task_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "assignment_id or task_id is required".to_string())?;
    resolve_assignment_id_from_task(state, task_id, allowed_states, allowed_label)
}

fn resolve_assignment_id_from_task(
    state: &SqliteProjectState,
    task_id: &str,
    allowed_states: &[AssignmentLifecycleState],
    allowed_label: &str,
) -> Result<String, String> {
    let replay = state
        .replay_events(ReplayEventsQuery {
            task_id: Some(task_id.to_string()),
            scope: None,
            limit: Some(200),
        })
        .map_err(|error| error.to_string())?;
    let mut seen = BTreeSet::new();
    let mut assignment_ids = Vec::new();
    for event in replay.events {
        if let Some(assignment_id) = assignment_id_from_event(&event) {
            if seen.insert(assignment_id.clone()) {
                assignment_ids.push(assignment_id);
            }
        }
    }
    assignment_ids.reverse();
    for assignment_id in assignment_ids {
        let snapshot = match state.inspect_assignment(AssignmentQuery {
            assignment_id: assignment_id.clone(),
        }) {
            Ok(snapshot) => snapshot,
            Err(_) => continue,
        };
        if allowed_states.contains(&snapshot.state) {
            return Ok(assignment_id);
        }
    }
    Err(format!(
        "no assignment in `{allowed_label}` state was found for task `{task_id}`"
    ))
}

fn assignment_id_from_event(event: &ProjectEventSnapshot) -> Option<String> {
    event.payload.as_ref().and_then(|payload| {
        payload
            .get("assignment_id")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
    })
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
