use crate::{
    assignments::{
        store::{
            claim_task_for_assignment, insert_assignment, insert_task_event, load_assignment,
            record_handoff_failure_and_release, ClaimOutcome,
        },
        validation::validate_changed_files,
    },
    backlog, bundle, guidance,
    models::{
        ActionResult, ActionStatus, BacklogCandidate, BacklogListData, GenerateTaskBundleParams,
        NextSafeActionParams, TaskBundleData, TaskEventRecord, TaskRecord, WorkerAssignment,
        WorktreeCreateParams,
    },
    state::{
        AcquireLeaseCommand, AppendWorkerEventCommand, ApprovalSnapshot, AssignmentLifecycleState,
        AssignmentQuery, AssignmentSnapshot, BackendCapabilities, BackendInfo,
        BacklogCandidateSnapshot, CompleteExecutionCommand, DispatchWorkCommand,
        DispatchWorkOutcome, EventReplaySnapshot, FindingsQuery, FindingsSnapshot,
        FindingsValidationSnapshot, IntegrateResultCommand, LeaseSnapshot, NextSafeActionQuery,
        PrepareAssignmentCommand, ProjectState, ProjectStateError, ReconcileProjectQuery,
        ReconcileSnapshot, ReplayEventsQuery, ResolveApprovalCommand, SafeActionSnapshot,
        StartExecutionCommand, StateResult, TaskLifecycleState, TaskQuery, TaskSnapshot,
        ValidateFindingsQuery, WorkerEventSnapshot, WorkerWorkspaceSnapshot,
    },
    storage::{self, LeaseStore, StorageConnection, TaskEventInsert, TaskInsert, TaskStore},
    workspace,
};
use rusqlite::params;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// SQLite-backed ProjectState implementation shell.
///
/// This type is the first concrete backend behind the domain-shaped state
/// boundary. It deliberately starts with safe inspection and explicit
/// unsupported responses for behavior that has not been migrated yet.
pub struct SqliteProjectState {
    root: PathBuf,
    connection: StorageConnection,
}

impl SqliteProjectState {
    pub fn open(default_root: &Path, root: Option<&str>) -> StateResult<Self> {
        let connection = storage::connect(default_root, root)
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        Ok(Self {
            root: connection.storage.root.clone(),
            connection,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn unsupported<T>(&self, method: &str) -> StateResult<T> {
        Err(ProjectStateError::unsupported(
            method,
            "ProjectState behavior is not migrated to the SQLite backend shell yet",
        ))
    }

    fn connect_storage(&self) -> StateResult<StorageConnection> {
        storage::connect(&self.root, None)
            .map_err(|error| ProjectStateError::backend(error.to_string()))
    }

    fn root_string(&self) -> String {
        self.root.display().to_string()
    }

    fn record_handoff_failure(&self, task_id: &str, claimant: &str, stage: &str, detail: &str) {
        if let Ok(mut storage) = self.connect_storage() {
            let _ = record_handoff_failure_and_release(
                &mut storage.connection,
                task_id,
                claimant,
                stage,
                detail,
            );
        }
    }
}

impl ProjectState for SqliteProjectState {
    fn describe_backend(&self) -> StateResult<BackendInfo> {
        Ok(BackendInfo {
            name: "sqlite".to_string(),
            version: Some(storage::SCHEMA_VERSION.to_string()),
            capabilities: BackendCapabilities {
                durable: true,
                transactional_lifecycle: true,
                stable_event_replay: true,
                leases: true,
                shared_coordination: false,
                migrations: true,
                offline: true,
            },
        })
    }

    fn dispatch_work(&self, command: DispatchWorkCommand) -> StateResult<DispatchWorkOutcome> {
        let root = self.root_string();
        let backlog = backlog::list_backlog(&self.root, Some(root.as_str()), Some(100));
        let BacklogListData { candidates, .. } = match backlog {
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
                return Err(ProjectStateError::backend(error.unwrap_or(summary)));
            }
        };
        let candidate = candidates
            .into_iter()
            .find(|candidate| {
                command
                    .preferred_worker
                    .as_deref()
                    .is_none_or(|worker| candidate.suggested_worker.as_deref() == Some(worker))
            })
            .ok_or_else(|| ProjectStateError::not_found("no runnable backlog items to dispatch"))?;

        let storage = self.connect_storage()?;
        match storage
            .repository()
            .leases()
            .active_conflict("project", "root", None)
            .map_err(map_repository_error)?
        {
            Some(lease) => {
                return Err(ProjectStateError::conflict(format!(
                    "project is leased by `{}` until {}",
                    lease.owner, lease.expires_at
                )));
            }
            None => {}
        }
        let task = storage
            .repository()
            .tasks()
            .create(TaskInsert {
                source_item_id: candidate.item_id.clone(),
                title: candidate.title.clone(),
                worker: candidate.suggested_worker.clone(),
            })
            .map_err(|error| match error {
                storage::RepositoryError::Conflict { .. } => ProjectStateError::conflict(format!(
                    "active task already exists for backlog item `{}`",
                    candidate.item_id
                )),
                other => map_repository_error(other),
            })?;
        let summary = command.summary.unwrap_or_else(|| {
            format!(
                "Queued {} for external worker execution.",
                candidate.item_id
            )
        });
        storage
            .repository()
            .tasks()
            .record_event(TaskEventInsert {
                task_id: task.id.clone(),
                sequence: Some(1),
                event_type: "task_queued".to_string(),
                summary,
                payload: Some(json!({
                    "source": candidate.source,
                    "item_id": candidate.item_id,
                    "worker": candidate.suggested_worker,
                    "status": "queued"
                })),
            })
            .map_err(map_repository_error)?;

        Ok(DispatchWorkOutcome {
            task: task_snapshot(task),
            candidate: candidate_snapshot(candidate),
        })
    }

    fn prepare_assignment(
        &self,
        command: PrepareAssignmentCommand,
    ) -> StateResult<AssignmentSnapshot> {
        let root = self.root_string();
        let claimant = if command.claimant.trim().is_empty() {
            return Err(ProjectStateError::invalid_command("claimant is required"));
        } else {
            command.claimant.trim().to_string()
        };
        let mut storage = self.connect_storage()?;
        let task = match claim_task_for_assignment(
            &mut storage.connection,
            command.task_id.as_deref(),
            command.worker.as_deref(),
            claimant.as_str(),
        )
        .map_err(ProjectStateError::backend)?
        {
            ClaimOutcome::Claimed(task) => task,
            ClaimOutcome::Existing(assignment) => return Ok(assignment_snapshot(assignment, true)),
            ClaimOutcome::NoTask => {
                return Err(ProjectStateError::not_found(
                    "no queued task is available for assignment",
                ));
            }
        };
        drop(storage);

        let worktree = match workspace::worktree_create(
            &self.root,
            WorktreeCreateParams {
                root: Some(root.clone()),
                task_id: task.id.clone(),
                base_ref: command.base_ref,
            },
        ) {
            ActionResult {
                status: ActionStatus::Completed | ActionStatus::Skipped,
                data: Some(data),
                ..
            } => data,
            ActionResult { error, summary, .. } => {
                self.record_handoff_failure(
                    &task.id,
                    claimant.as_str(),
                    "creating the worktree",
                    &error.clone().unwrap_or_else(|| summary.clone()),
                );
                return Err(ProjectStateError::backend(format!(
                    "worktree preparation failed: {}",
                    error.unwrap_or(summary)
                )));
            }
        };
        let bundle = match bundle::generate_task_bundle(
            &self.root,
            GenerateTaskBundleParams {
                root: Some(root),
                task_id: task.id.clone(),
                verification_command: command.verification_command,
            },
        ) {
            ActionResult {
                status: ActionStatus::Completed,
                data: Some(TaskBundleData { bundle, .. }),
                ..
            } => bundle,
            ActionResult { error, summary, .. } => {
                self.record_handoff_failure(
                    &task.id,
                    claimant.as_str(),
                    "generating the worker bundle",
                    &error.clone().unwrap_or_else(|| summary.clone()),
                );
                return Err(ProjectStateError::backend(format!(
                    "bundle generation failed: {}",
                    error.unwrap_or(summary)
                )));
            }
        };
        let storage = self.connect_storage().inspect_err(|error| {
            self.record_handoff_failure(
                &task.id,
                claimant.as_str(),
                "reopening assignment storage",
                &error.to_string(),
            );
        })?;
        let assignment = insert_assignment(
            &storage.connection,
            &task,
            &worktree,
            &bundle,
            Some(claimant.as_str()),
        )
        .inspect_err(|error| {
            self.record_handoff_failure(
                &task.id,
                claimant.as_str(),
                "persisting the worker assignment",
                error,
            );
        })
        .map_err(ProjectStateError::backend)?;
        storage
            .repository()
            .tasks()
            .record_event(TaskEventInsert {
                task_id: task.id,
                sequence: None,
                event_type: "worker_assignment_prepared".to_string(),
                summary: format!("Prepared worker assignment `{}`.", assignment.id),
                payload: Some(json!({
                    "assignment_id": assignment.id,
                    "worker": assignment.worker,
                    "worktree_path": assignment.worktree_path
                })),
            })
            .map_err(map_repository_error)?;
        Ok(assignment_snapshot(assignment, false))
    }

    fn start_execution(&self, command: StartExecutionCommand) -> StateResult<AssignmentSnapshot> {
        if command.assignment_id.trim().is_empty() {
            return Err(ProjectStateError::invalid_command(
                "assignment_id is required",
            ));
        }
        let mut storage = self.connect_storage()?;
        let transaction = storage
            .connection
            .transaction()
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        let assignment = load_assignment(&transaction, &command.assignment_id)
            .map_err(map_assignment_load_error)?;
        if assignment.status != "prepared" {
            return Err(ProjectStateError::conflict(format!(
                "worker assignment `{}` is `{}`; only prepared assignments can be started",
                command.assignment_id, assignment.status
            )));
        }
        let task_updated = transaction
            .execute(
                r#"
                UPDATE tasks
                SET status = 'running',
                    started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1 AND status IN ('claimed', 'running')
                "#,
                [assignment.task_id.as_str()],
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        if task_updated == 0 {
            return Err(ProjectStateError::conflict(format!(
                "task `{}` must be claimed before the assignment can start",
                assignment.task_id
            )));
        }
        let assignment_updated = transaction
            .execute(
                r#"
                UPDATE worker_assignments
                SET status = 'running',
                    worker_session = ?2,
                    started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1 AND status = 'prepared'
                "#,
                params![command.assignment_id, command.worker_session],
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        if assignment_updated == 0 {
            return Err(ProjectStateError::conflict(format!(
                "assignment `{}` was not prepared",
                command.assignment_id
            )));
        }
        insert_task_event(
            &transaction,
            &assignment.task_id,
            "worker_started",
            &format!("Started worker assignment `{}`.", command.assignment_id),
            Some(json!({
                "assignment_id": command.assignment_id,
                "worker": assignment.worker,
                "worker_session": command.worker_session
            })),
        )
        .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        let assignment = load_assignment(&transaction, &command.assignment_id)
            .map_err(map_assignment_load_error)?;
        transaction
            .commit()
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        Ok(assignment_snapshot(assignment, false))
    }

    fn append_worker_event(
        &self,
        command: AppendWorkerEventCommand,
    ) -> StateResult<WorkerEventSnapshot> {
        if command.assignment_id.trim().is_empty() {
            return Err(ProjectStateError::invalid_command(
                "assignment_id is required",
            ));
        }
        if command.event_type.trim().is_empty() {
            return Err(ProjectStateError::invalid_command("event_type is required"));
        }
        if command.summary.trim().is_empty() {
            return Err(ProjectStateError::invalid_command("summary is required"));
        }
        let storage = self.connect_storage()?;
        let assignment = load_assignment(&storage.connection, &command.assignment_id)
            .map_err(map_assignment_load_error)?;
        if assignment.status != "running" {
            return Err(ProjectStateError::conflict(format!(
                "worker assignment `{}` is `{}`; record progress only after start_execution",
                command.assignment_id, assignment.status
            )));
        }
        let payload = if command.payload.is_empty() {
            json!({ "assignment_id": command.assignment_id })
        } else {
            json!({
                "assignment_id": command.assignment_id,
                "payload": command.payload
            })
        };
        let event = storage
            .repository()
            .tasks()
            .record_event(TaskEventInsert {
                task_id: assignment.task_id,
                sequence: None,
                event_type: command.event_type,
                summary: command.summary,
                payload: Some(payload),
            })
            .map_err(map_repository_error)?;
        Ok(worker_event_snapshot(command.assignment_id, event))
    }

    fn complete_execution(
        &self,
        command: CompleteExecutionCommand,
    ) -> StateResult<AssignmentSnapshot> {
        if command.assignment_id.trim().is_empty() {
            return Err(ProjectStateError::invalid_command(
                "assignment_id is required",
            ));
        }
        if command.summary.trim().is_empty() {
            return Err(ProjectStateError::invalid_command("summary is required"));
        }
        if command.status == "completed" && command.verification_status.is_none() {
            return Err(ProjectStateError::invalid_command(
                "verification_status is required when status is completed; use passed, failed, skipped, or not_run",
            ));
        }
        let new_assignment_status = match command.status.as_str() {
            "completed" => "completed",
            "failed" => "failed",
            "cancelled" => "cancelled",
            _ => {
                return Err(ProjectStateError::invalid_command(
                    "status must be completed, failed, or cancelled",
                ));
            }
        };
        let changed_files_json =
            serde_json::to_string(&command.changed_files).map_err(ProjectStateError::from)?;
        let mut storage = self.connect_storage()?;
        let transaction = storage
            .connection
            .transaction()
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        let assignment = load_assignment(&transaction, &command.assignment_id)
            .map_err(map_assignment_load_error)?;
        if assignment.status != "running" {
            return Err(ProjectStateError::conflict(format!(
                "worker assignment `{}` is `{}`; only running assignments can be completed",
                command.assignment_id, assignment.status
            )));
        }
        validate_changed_files(&assignment.bundle.owned_surfaces, &command.changed_files)
            .map_err(ProjectStateError::invalid_command)?;
        let assignment_updated = transaction
            .execute(
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
                    command.assignment_id,
                    new_assignment_status,
                    command.status,
                    command.summary,
                    changed_files_json,
                    command.verification_status
                ],
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        if assignment_updated == 0 {
            return Err(ProjectStateError::conflict(format!(
                "assignment `{}` was not running",
                command.assignment_id
            )));
        }
        insert_task_event(
            &transaction,
            &assignment.task_id,
            "worker_result",
            &format!(
                "Worker assignment `{}` finished with `{}`.",
                command.assignment_id, command.status
            ),
            Some(json!({
                "assignment_id": command.assignment_id,
                "status": command.status,
                "summary": command.summary,
                "changed_files": command.changed_files,
                "verification_status": command.verification_status
            })),
        )
        .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        let task_updated = transaction
            .execute(
                r#"
                UPDATE tasks
                SET status = ?2,
                    finished_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1 AND status IN ('claimed', 'running')
                "#,
                params![assignment.task_id, command.status],
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        if task_updated == 0 {
            return Err(ProjectStateError::conflict(format!(
                "task `{}` must be claimed or running before completion",
                assignment.task_id
            )));
        }
        let assignment = load_assignment(&transaction, &command.assignment_id)
            .map_err(map_assignment_load_error)?;
        transaction
            .commit()
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        Ok(assignment_snapshot(assignment, false))
    }

    fn resolve_approval(&self, _command: ResolveApprovalCommand) -> StateResult<ApprovalSnapshot> {
        self.unsupported("resolve_approval")
    }

    fn acquire_lease(&self, _command: AcquireLeaseCommand) -> StateResult<LeaseSnapshot> {
        self.unsupported("acquire_lease")
    }

    fn integrate_result(
        &self,
        _command: IntegrateResultCommand,
    ) -> StateResult<crate::state::IntegrationSnapshot> {
        self.unsupported("integrate_result")
    }

    fn next_safe_action(&self, _query: NextSafeActionQuery) -> StateResult<SafeActionSnapshot> {
        let root = self.root.display().to_string();
        let result = guidance::next_safe_action(
            &self.root,
            NextSafeActionParams {
                root: Some(root.clone()),
            },
        );
        match result {
            crate::models::ActionResult {
                status: ActionStatus::Completed,
                data: Some(data),
                ..
            } => Ok(SafeActionSnapshot {
                recommended_tool: data.recommended_tool,
                reason: data.reason,
                params: data.params,
            }),
            crate::models::ActionResult { summary, error, .. } => {
                Err(ProjectStateError::backend(error.unwrap_or(summary)))
            }
        }
    }

    fn inspect_task(&self, query: TaskQuery) -> StateResult<TaskSnapshot> {
        let task = self
            .connection
            .repository()
            .tasks()
            .get(&query.task_id)
            .map_err(map_repository_error)?;
        Ok(task_snapshot(task))
    }

    fn inspect_assignment(&self, query: AssignmentQuery) -> StateResult<AssignmentSnapshot> {
        let storage = self.connect_storage()?;
        let assignment = load_assignment(&storage.connection, &query.assignment_id)
            .map_err(map_assignment_load_error)?;
        Ok(assignment_snapshot(assignment, false))
    }

    fn replay_events(&self, _query: ReplayEventsQuery) -> StateResult<EventReplaySnapshot> {
        self.unsupported("replay_events")
    }

    fn list_findings(&self, _query: FindingsQuery) -> StateResult<FindingsSnapshot> {
        self.unsupported("list_findings")
    }

    fn validate_findings(
        &self,
        _query: ValidateFindingsQuery,
    ) -> StateResult<FindingsValidationSnapshot> {
        self.unsupported("validate_findings")
    }

    fn reconcile_project(&self, _query: ReconcileProjectQuery) -> StateResult<ReconcileSnapshot> {
        self.unsupported("reconcile_project")
    }
}

fn map_repository_error(error: storage::RepositoryError) -> ProjectStateError {
    match error {
        storage::RepositoryError::NotFound => ProjectStateError::not_found("task was not found"),
        storage::RepositoryError::Conflict { message } => ProjectStateError::conflict(message),
        storage::RepositoryError::Serialization { message } => {
            ProjectStateError::Serialization { message }
        }
        storage::RepositoryError::Backend { message } => ProjectStateError::backend(message),
    }
}

fn map_assignment_load_error(error: rusqlite::Error) -> ProjectStateError {
    match error {
        rusqlite::Error::QueryReturnedNoRows => {
            ProjectStateError::not_found("worker assignment was not found")
        }
        other => ProjectStateError::backend(other.to_string()),
    }
}

fn candidate_snapshot(candidate: BacklogCandidate) -> BacklogCandidateSnapshot {
    BacklogCandidateSnapshot {
        source: candidate.source,
        item_id: candidate.item_id,
        title: candidate.title,
        priority: candidate.priority,
        area: candidate.area,
        item_type: candidate.item_type,
        suggested_worker: candidate.suggested_worker,
        owned_surfaces: candidate.owned_surfaces,
        external_refs: candidate.external_refs,
    }
}

fn task_snapshot(task: TaskRecord) -> TaskSnapshot {
    TaskSnapshot {
        id: task.id,
        source_item_id: task.source_item_id,
        title: task.title,
        status: task.status.clone(),
        state: task_state(&task.status),
        worker: task.worker,
        claimed_by: task.claimed_by,
        claimed_at: task.claimed_at,
        started_at: task.started_at,
        finished_at: task.finished_at,
        worker_workspace: match (
            task.workspace_path,
            task.workspace_branch,
            task.workspace_base_ref,
        ) {
            (Some(path), Some(branch), Some(base_ref)) => Some(WorkerWorkspaceSnapshot {
                path,
                branch,
                base_ref,
            }),
            _ => None,
        },
        created_at: task.created_at,
        updated_at: task.updated_at,
    }
}

fn assignment_snapshot(assignment: WorkerAssignment, reused_existing: bool) -> AssignmentSnapshot {
    AssignmentSnapshot {
        id: assignment.id,
        task_id: assignment.task_id,
        worker: assignment.worker,
        state: assignment_state(&assignment.status),
        reused_existing,
        assigned_by: assignment.assigned_by,
        worker_session: assignment.worker_session,
        worktree_path: assignment.worktree_path,
        bundle: assignment.bundle,
        started_at: assignment.started_at,
        completed_at: assignment.completed_at,
        result_status: assignment.result_status,
        summary: assignment.summary,
        changed_files: assignment.changed_files,
        verification_status: assignment.verification_status,
        created_at: assignment.created_at,
        updated_at: assignment.updated_at,
    }
}

fn assignment_state(status: &str) -> AssignmentLifecycleState {
    match status {
        "running" => AssignmentLifecycleState::Running,
        "completed" => AssignmentLifecycleState::Completed,
        "failed" => AssignmentLifecycleState::Failed,
        "cancelled" => AssignmentLifecycleState::Cancelled,
        _ => AssignmentLifecycleState::Prepared,
    }
}

fn worker_event_snapshot(assignment_id: String, event: TaskEventRecord) -> WorkerEventSnapshot {
    WorkerEventSnapshot {
        assignment_id,
        task_id: event.task_id,
        sequence: event.sequence,
        event_type: event.event_type,
        summary: event.summary,
        payload: payload_object(event.payload),
        created_at: event.created_at,
    }
}

fn payload_object(payload: Option<Value>) -> BTreeMap<String, Value> {
    match payload {
        Some(Value::Object(object)) => object.into_iter().collect(),
        Some(value) => BTreeMap::from([("value".to_string(), value)]),
        None => BTreeMap::new(),
    }
}

fn task_state(status: &str) -> TaskLifecycleState {
    match status {
        "claimed" => TaskLifecycleState::Claimed,
        "running" => TaskLifecycleState::Running,
        "completed" => TaskLifecycleState::Completed,
        "failed" => TaskLifecycleState::Failed,
        "cancelled" => TaskLifecycleState::Cancelled,
        _ => TaskLifecycleState::Queued,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{create_task_record, NewTask};
    use std::{fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn opens_sqlite_project_state_backend() {
        let project = TempDir::new().expect("temp dir");

        let state = SqliteProjectState::open(project.path(), None).expect("state");
        let info = state.describe_backend().expect("backend info");

        assert_eq!(state.root(), project.path().canonicalize().unwrap());
        assert_eq!(info.name, "sqlite");
        assert!(info.capabilities.durable);
        assert!(!info.capabilities.shared_coordination);
    }

    #[test]
    fn inspects_task_through_project_state() {
        let project = TempDir::new().expect("temp dir");
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "State task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");

        let state = SqliteProjectState::open(project.path(), None).expect("state");
        let snapshot = state
            .inspect_task(TaskQuery { task_id: task.id })
            .expect("task snapshot");

        assert_eq!(snapshot.source_item_id, "PROJ-001");
        assert_eq!(snapshot.state, TaskLifecycleState::Queued);
    }

    #[test]
    fn duplicate_active_dispatch_fails_safely_through_project_state() {
        let project = project_with_backlog();
        let state = SqliteProjectState::open(project.path(), None).expect("state");

        let first = state
            .dispatch_work(DispatchWorkCommand {
                summary: None,
                preferred_worker: None,
            })
            .expect("first dispatch");
        assert_eq!(first.task.source_item_id, "PROJ-001");

        let error = state
            .dispatch_work(DispatchWorkCommand {
                summary: None,
                preferred_worker: None,
            })
            .expect_err("duplicate active task should fail");

        assert!(
            matches!(error, ProjectStateError::Conflict { .. }),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn duplicate_active_assignment_returns_existing_snapshot_through_project_state() {
        let project = project_with_backlog();
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "State assignment".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        let state = SqliteProjectState::open(project.path(), None).expect("state");

        let first = state
            .prepare_assignment(PrepareAssignmentCommand {
                task_id: Some(task.id.clone()),
                worker: None,
                claimant: "tester".to_string(),
                base_ref: None,
                verification_command: Vec::new(),
            })
            .expect("first assignment");
        let second = state
            .prepare_assignment(PrepareAssignmentCommand {
                task_id: Some(task.id),
                worker: None,
                claimant: "tester".to_string(),
                base_ref: None,
                verification_command: Vec::new(),
            })
            .expect("existing assignment");

        assert_eq!(second.id, first.id);
        assert!(second.reused_existing);
        assert_eq!(second.state, AssignmentLifecycleState::Prepared);
    }

    #[test]
    fn assignment_lifecycle_runs_through_project_state() {
        let project = project_with_backlog();
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "State lifecycle".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        let state = SqliteProjectState::open(project.path(), None).expect("state");

        let prepared = state
            .prepare_assignment(PrepareAssignmentCommand {
                task_id: Some(task.id),
                worker: Some("coder".to_string()),
                claimant: "tester".to_string(),
                base_ref: None,
                verification_command: vec!["make".to_string(), "check".to_string()],
            })
            .expect("prepare");
        assert_eq!(prepared.state, AssignmentLifecycleState::Prepared);

        let started = state
            .start_execution(StartExecutionCommand {
                assignment_id: prepared.id.clone(),
                worker_session: Some("session-1".to_string()),
            })
            .expect("start");
        assert_eq!(started.state, AssignmentLifecycleState::Running);

        let event = state
            .append_worker_event(AppendWorkerEventCommand {
                assignment_id: prepared.id.clone(),
                event_type: "worker_progress".to_string(),
                summary: "README updated.".to_string(),
                payload: BTreeMap::new(),
            })
            .expect("worker event");
        assert_eq!(event.event_type, "worker_progress");

        let completed = state
            .complete_execution(CompleteExecutionCommand {
                assignment_id: prepared.id.clone(),
                status: "completed".to_string(),
                summary: "Done.".to_string(),
                changed_files: vec!["README.md".to_string()],
                verification_status: Some("passed".to_string()),
            })
            .expect("complete");
        assert_eq!(completed.state, AssignmentLifecycleState::Completed);

        let task = state
            .inspect_task(TaskQuery {
                task_id: completed.task_id,
            })
            .expect("task");
        assert_eq!(task.state, TaskLifecycleState::Completed);
    }

    fn project_with_backlog() -> TempDir {
        let project = TempDir::new().expect("temp dir");
        git(&project, &["init"]);
        git(&project, &["config", "user.name", "Platypus Test"]);
        git(
            &project,
            &["config", "user.email", "platypus@example.invalid"],
        );
        fs::write(project.path().join("README.md"), "# Test\n").expect("readme");
        git(&project, &["add", "README.md"]);
        git(&project, &["commit", "-m", "Initial commit"]);
        fs::create_dir_all(project.path().join("backlog/items")).expect("items");
        fs::write(
            project.path().join("backlog/items/PROJ-001.md"),
            r#"---
id: PROJ-001
title: State lifecycle
priority: P0
type: foundation
area: execution
epic: general
depends_on: []
suggested_worker: coder
owned_surfaces:
- README.md
---

# PROJ-001 State lifecycle

## Goal

Verify the ProjectState lifecycle path.

## Implementation Contract

Edit only the assigned owned surface.

## Acceptance

- Assignment lifecycle completes.
"#,
        )
        .expect("item");
        project
    }

    fn git(project: &TempDir, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(project.path())
            .output()
            .expect("git command");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
