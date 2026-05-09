use crate::{
    assignments::{
        store::{
            claim_task_for_assignment, insert_assignment, insert_task_event, load_assignment,
            record_handoff_failure_and_release, ClaimOutcome,
        },
        validation::validate_changed_files,
    },
    backlog, bundle, git_trailers,
    models::{
        ActionResult, ActionStatus, BacklogCandidate, BacklogListData, EventRecord, EvidenceRecord,
        FindingRecord, GenerateTaskBundleParams, LeaseRecord, RuntimeTransitionRecord,
        TaskBundleData, TaskEventRecord, TaskRecord, WorkerAssignment, WorktreeCreateParams,
    },
    state::{
        AcquireLeaseCommand, AppendWorkerEventCommand, ApprovalSnapshot, AssignmentLifecycleState,
        AssignmentQuery, AssignmentSnapshot, BackendCapabilities, BackendInfo,
        BacklogCandidateSnapshot, ClearWorkspaceCommand, CompleteExecutionCommand,
        DispatchWorkCommand, DispatchWorkOutcome, EventReplaySnapshot, EvidenceListSnapshot,
        EvidenceQuery, EvidenceSnapshot, FindingDispositionSnapshot, FindingSnapshot,
        FindingsQuery, FindingsSnapshot, FindingsValidationSnapshot, IntegrateResultCommand,
        LeaseSnapshot, LeaseState, NextSafeActionQuery, PrepareAssignmentCommand,
        ProjectEventSnapshot, ProjectState, ProjectStateError, ReconcileGap, ReconcileProjectQuery,
        ReconcileSnapshot, RecordEvidenceCommand, RecordFindingCommand, RecordWorkspaceCommand,
        ReplayEventsQuery, ResolveApprovalCommand, SafeActionSnapshot, StartExecutionCommand,
        StateResult, TaskLifecycleState, TaskQuery, TaskSnapshot, UpdateFindingDispositionCommand,
        ValidateFindingsQuery, WorkerEventSnapshot, WorkerWorkspaceSnapshot,
    },
    storage::{
        self, EventStore, LeaseStore, StorageConnection, TaskEventInsert, TaskInsert, TaskStore,
        TransitionStore,
    },
    workspace,
};
use rusqlite::{params, OptionalExtension, Row};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const EVIDENCE_DEFAULT_LIMIT: usize = 50;
const EVIDENCE_MAX_LIMIT: usize = 200;
const FINDINGS_DEFAULT_LIMIT: usize = 20;
const FINDINGS_MAX_LIMIT: usize = 200;
const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const VALID_EVIDENCE_KINDS: &[&str] = &[
    "commit",
    "verification",
    "file_summary",
    "worker_finding",
    "manager_disposition",
    "external_report",
    "note",
];
const VALID_FINDING_STATUSES: &[&str] = &[
    "open",
    "accepted",
    "resolved",
    "rejected",
    "deferred",
    "duplicate",
];

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

    fn acquire_lease(&self, command: AcquireLeaseCommand) -> StateResult<LeaseSnapshot> {
        let lease = self
            .connection
            .repository()
            .leases()
            .acquire(storage::LeaseInsert {
                scope: lease_scope(&command.scope).to_string(),
                target_id: command.target_id,
                owner: command.owner,
                ttl_seconds: command.ttl_seconds,
                metadata: command.metadata,
            })
            .map_err(map_repository_error)?;
        Ok(lease_snapshot(lease))
    }

    fn integrate_result(
        &self,
        _command: IntegrateResultCommand,
    ) -> StateResult<crate::state::IntegrationSnapshot> {
        self.unsupported("integrate_result")
    }

    fn record_workspace(&self, command: RecordWorkspaceCommand) -> StateResult<TaskSnapshot> {
        self.connection
            .connection
            .execute(
                r#"
                UPDATE tasks
                SET workspace_path = ?2,
                    workspace_branch = ?3,
                    workspace_base_ref = ?4,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1
                "#,
                params![
                    command.task_id,
                    command.path,
                    command.branch,
                    command.base_ref
                ],
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        self.inspect_task(TaskQuery {
            task_id: command.task_id,
        })
    }

    fn clear_workspace(&self, command: ClearWorkspaceCommand) -> StateResult<TaskSnapshot> {
        self.connection
            .connection
            .execute(
                r#"
                UPDATE tasks
                SET workspace_path = NULL,
                    workspace_branch = NULL,
                    workspace_base_ref = NULL,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1
                "#,
                [command.task_id.as_str()],
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        self.inspect_task(TaskQuery {
            task_id: command.task_id,
        })
    }

    fn next_safe_action(&self, _query: NextSafeActionQuery) -> StateResult<SafeActionSnapshot> {
        let root = self.root_string();
        let closed_item_ids = backlog::closed_item_ids(&self.root);

        if let Some(assignment) =
            active_assignment(&self.connection.connection, "running", &closed_item_ids)
                .map_err(|error| ProjectStateError::backend(error.to_string()))?
        {
            return Ok(safe_action(
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
            ));
        }
        if let Some(assignment) =
            active_assignment(&self.connection.connection, "prepared", &closed_item_ids)
                .map_err(|error| ProjectStateError::backend(error.to_string()))?
        {
            return Ok(safe_action(
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
            ));
        }
        if let Some(task) = queued_task(&self.connection.connection, &closed_item_ids)
            .map_err(|error| ProjectStateError::backend(error.to_string()))?
        {
            return Ok(safe_action(
                "prepare_worker_handoff",
                format!("Task `{}` is queued for worker execution.", task.id),
                "Prepare a single handoff object so the host can assign the worktree and bundle to a worker.",
                [
                    ("root", root.as_str()),
                    ("task_id", task.id.as_str()),
                    ("worker", task.worker.as_deref().unwrap_or("")),
                    ("claimant", "external-worker"),
                ],
            ));
        }
        if let Some(task) = claimed_task(&self.connection.connection, &closed_item_ids)
            .map_err(|error| ProjectStateError::backend(error.to_string()))?
        {
            return Ok(safe_action(
                "prepare_worker_handoff",
                format!(
                    "Task `{}` is already claimed and needs a worker handoff.",
                    task.id
                ),
                "Prepare a worker assignment for the claimed task before dispatching more backlog work.",
                [
                    ("root", root.as_str()),
                    ("task_id", task.id.as_str()),
                    ("worker", task.worker.as_deref().unwrap_or("")),
                    ("claimant", "external-worker"),
                ],
            ));
        }
        if let Some(task) = completed_task_without_verification(&self.connection.connection)
            .map_err(|error| ProjectStateError::backend(error.to_string()))?
        {
            return Ok(safe_action(
                "record_verification_evidence",
                format!("Completed task `{}` has no verification evidence.", task.id),
                "Record verification evidence before treating the work as reconciled.",
                [
                    ("root", root.as_str()),
                    ("source_item_id", task.source_item_id.as_str()),
                    ("source_task_id", task.id.as_str()),
                    ("summary", "Describe the verification result."),
                ],
            ));
        }
        if let Some(task) = completed_task_without_integration(&self.connection.connection)
            .map_err(|error| ProjectStateError::backend(error.to_string()))?
        {
            return Ok(safe_action(
                "integrate_worker_result",
                format!("Completed task `{}` has not been integrated.", task.id),
                "Integrate the completed verified worker result before reconciliation.",
                [("root", root.as_str()), ("task_id", task.id.as_str())],
            ));
        }

        let backlog = backlog::list_backlog(&self.root, Some(root.as_str()), Some(1));
        if let ActionResult {
            data: Some(BacklogListData { candidates, .. }),
            ..
        } = backlog
        {
            if let Some(candidate) = candidates.first() {
                return Ok(safe_action(
                    "dispatch_next_work",
                    format!("Backlog item `{}` is runnable.", candidate.item_id),
                    "Dispatch the next runnable backlog item to create a durable task.",
                    [("root", root.as_str()), ("summary", "")],
                ));
            }
        }

        Ok(safe_action(
            "create_backlog_item",
            "No queued task or runnable backlog item was found.".to_string(),
            "Create or draft a backlog item before dispatching work.",
            [
                ("root", root.as_str()),
                ("summary", "Describe the work item."),
            ],
        ))
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

    fn replay_events(&self, query: ReplayEventsQuery) -> StateResult<EventReplaySnapshot> {
        let limit = query.limit.unwrap_or(50).clamp(1, 200);
        let scope = query
            .scope
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let task_id = query
            .task_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let event_repository = self.connection.repository().events();
        let mut events = event_repository
            .list(scope, task_id, limit)
            .map_err(map_repository_error)?;
        if scope.is_none() || scope == Some("task") {
            let task_events = event_repository
                .list_task_events(task_id, limit)
                .map_err(map_repository_error)?;
            events.extend(task_events);
        }
        let transition_entity = if task_id.is_some() { task_id } else { None };
        let transitions = self
            .connection
            .repository()
            .transitions()
            .list(scope, transition_entity, limit)
            .map_err(map_repository_error)?;
        events.extend(transitions.into_iter().map(transition_to_event_record));
        events.sort_by(|left, right| {
            left.replay_order
                .cmp(&right.replay_order)
                .then_with(|| left.created_at.cmp(&right.created_at))
                .then_with(|| left.cursor.cmp(&right.cursor))
        });
        if events.len() > limit {
            events.truncate(limit);
        }
        Ok(EventReplaySnapshot {
            cursor: events.last().map(|event| event.cursor.clone()),
            events: events.into_iter().map(project_event_snapshot).collect(),
        })
    }

    fn record_evidence(&self, command: RecordEvidenceCommand) -> StateResult<EvidenceSnapshot> {
        let kind = command.kind.trim();
        let summary = command.summary.trim();
        if !VALID_EVIDENCE_KINDS.contains(&kind) {
            return Err(ProjectStateError::invalid_command("invalid evidence kind"));
        }
        if summary.is_empty() {
            return Err(ProjectStateError::invalid_command("summary is required"));
        }
        let id = match clean_optional(command.id) {
            Some(id) => id,
            None => next_evidence_id(&self.connection.connection)
                .map_err(|error| ProjectStateError::backend(error.to_string()))?,
        };
        self.connection
            .connection
            .execute(
                r#"
                INSERT INTO evidence(
                    id, source_item_id, source_task_id, kind, summary, refs_json, metadata_json
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                params![
                    id,
                    clean_optional(command.source_item_id),
                    clean_optional(command.source_task_id),
                    kind,
                    summary,
                    json_string(&clean_vec(command.refs)),
                    json_string(&command.metadata)
                ],
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        get_evidence(&self.connection.connection, &id)
            .map(evidence_snapshot)
            .map_err(|error| ProjectStateError::backend(error.to_string()))
    }

    fn list_evidence(&self, query: EvidenceQuery) -> StateResult<EvidenceListSnapshot> {
        let evidence = query_evidence(
            &self.connection.connection,
            query.source_item_id.as_deref(),
            query.source_task_id.as_deref(),
            query.kind.as_deref(),
            query
                .limit
                .unwrap_or(EVIDENCE_DEFAULT_LIMIT)
                .clamp(1, EVIDENCE_MAX_LIMIT),
        )
        .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        Ok(EvidenceListSnapshot {
            evidence: evidence.into_iter().map(evidence_snapshot).collect(),
        })
    }

    fn record_finding(&self, command: RecordFindingCommand) -> StateResult<FindingSnapshot> {
        let title = command.title.trim();
        let summary = command.summary.trim();
        if title.is_empty() {
            return Err(ProjectStateError::invalid_command("title is required"));
        }
        if summary.is_empty() {
            return Err(ProjectStateError::invalid_command("summary is required"));
        }
        let id = match clean_optional(command.id) {
            Some(id) => id,
            None => next_finding_id(
                &self.connection.connection,
                command.source_item_id.as_deref(),
                command.source_task_id.as_deref(),
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?,
        };
        if finding_exists(&self.connection.connection, &id)
            .map_err(|error| ProjectStateError::backend(error.to_string()))?
        {
            return Err(ProjectStateError::conflict(format!(
                "finding `{id}` already exists"
            )));
        }
        let required = command.required.unwrap_or(true);
        self.connection
            .connection
            .execute(
                r#"
                INSERT INTO findings(
                    id,
                    source_item_id,
                    source_task_id,
                    source_finding_ref,
                    title,
                    status,
                    severity,
                    required,
                    summary,
                    evidence_json,
                    metadata_json
                )
                VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7, ?8, ?9, ?10)
                "#,
                params![
                    id,
                    clean_optional(command.source_item_id),
                    clean_optional(command.source_task_id),
                    clean_optional(command.source_finding_ref),
                    title,
                    clean_optional(command.severity),
                    bool_to_i64(required),
                    summary,
                    json_string(&command.evidence_refs),
                    json_string(&command.metadata)
                ],
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        get_finding(&self.connection.connection, &id)
            .map(finding_snapshot)
            .map_err(|error| ProjectStateError::backend(error.to_string()))
    }

    fn list_findings(&self, query: FindingsQuery) -> StateResult<FindingsSnapshot> {
        let findings = query_findings(
            &self.connection.connection,
            query.source_item_id.as_deref(),
            query.source_task_id.as_deref(),
            query.status.as_deref(),
            query
                .limit
                .unwrap_or(FINDINGS_DEFAULT_LIMIT)
                .clamp(1, FINDINGS_MAX_LIMIT),
        )
        .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        Ok(FindingsSnapshot {
            findings: findings.into_iter().map(finding_snapshot).collect(),
        })
    }

    fn validate_findings(
        &self,
        query: ValidateFindingsQuery,
    ) -> StateResult<FindingsValidationSnapshot> {
        let unresolved_required = query_unresolved_required_findings(
            &self.connection.connection,
            query.source_item_id.as_deref(),
            query.source_task_id.as_deref(),
        )
        .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        Ok(FindingsValidationSnapshot {
            ok: unresolved_required.is_empty(),
            unresolved_required: unresolved_required
                .into_iter()
                .map(finding_snapshot)
                .collect(),
        })
    }

    fn update_finding_disposition(
        &self,
        command: UpdateFindingDispositionCommand,
    ) -> StateResult<FindingSnapshot> {
        let status = command.status.trim().to_ascii_lowercase();
        if !VALID_FINDING_STATUSES.contains(&status.as_str()) {
            return Err(ProjectStateError::invalid_command(format!(
                "invalid status `{}`",
                command.status
            )));
        }
        if !finding_exists(&self.connection.connection, &command.finding_id)
            .map_err(|error| ProjectStateError::backend(error.to_string()))?
        {
            return Err(ProjectStateError::not_found(format!(
                "finding `{}` does not exist",
                command.finding_id
            )));
        }
        self.connection
            .connection
            .execute(
                r#"
                UPDATE findings
                SET status = ?2,
                    owner = ?3,
                    disposition_reason = ?4,
                    evidence_json = ?5,
                    metadata_json = ?6,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1
                "#,
                params![
                    command.finding_id,
                    status,
                    clean_optional(command.owner),
                    clean_optional(command.disposition_reason),
                    json_string(&command.evidence_refs),
                    json_string(&command.metadata)
                ],
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        get_finding(&self.connection.connection, &command.finding_id)
            .map(finding_snapshot)
            .map_err(|error| ProjectStateError::backend(error.to_string()))
    }

    fn reconcile_project(&self, query: ReconcileProjectQuery) -> StateResult<ReconcileSnapshot> {
        let closed_item_ids = if query.include_closed_items {
            backlog::closed_item_ids(&self.root)
        } else {
            BTreeSet::new()
        };
        let completed_tasks = query_completed_tasks(&self.connection.connection)
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        let unresolved_required =
            query_unresolved_required_finding_summaries(&self.connection.connection)
                .map_err(|error| ProjectStateError::backend(error.to_string()))?;
        let mut gaps = Vec::new();

        for task in &completed_tasks {
            let verification = query_evidence(
                &self.connection.connection,
                None,
                Some(task.id.as_str()),
                Some("verification"),
                EVIDENCE_MAX_LIMIT,
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?;
            if verification.is_empty() {
                gaps.push(ReconcileGap {
                    kind: "missing_verification_evidence".to_string(),
                    source_item_id: Some(task.source_item_id.clone()),
                    source_task_id: Some(task.id.clone()),
                    summary: format!(
                        "Completed task `{}` for `{}` has no verification evidence.",
                        task.id, task.source_item_id
                    ),
                    next_action: "Record verification evidence or rerun verification.".to_string(),
                });
            }

            let integration_refs = query_evidence(
                &self.connection.connection,
                None,
                Some(task.id.as_str()),
                Some("commit"),
                EVIDENCE_MAX_LIMIT,
            )
            .map_err(|error| ProjectStateError::backend(error.to_string()))?
            .into_iter()
            .flat_map(|evidence| evidence.refs)
            .collect::<Vec<_>>();
            if integration_refs.is_empty() {
                gaps.push(ReconcileGap {
                    kind: "missing_integration_evidence".to_string(),
                    source_item_id: Some(task.source_item_id.clone()),
                    source_task_id: Some(task.id.clone()),
                    summary: format!(
                        "Completed task `{}` for `{}` has not been integrated.",
                        task.id, task.source_item_id
                    ),
                    next_action: "Integrate the worker result with integrate_worker_result."
                        .to_string(),
                });
                continue;
            }

            let mut closure_seen = false;
            let mut verification_seen = false;
            for commit in integration_refs
                .iter()
                .filter_map(|reference| commit_ref(reference))
            {
                match commit_trailers(&self.root, &commit) {
                    Ok(trailers) => {
                        if trailers.closes.contains(&task.source_item_id) {
                            closure_seen = true;
                        }
                        if trailers.verification_present {
                            verification_seen = true;
                        }
                    }
                    Err(error) => gaps.push(ReconcileGap {
                        kind: "invalid_integration_commit_ref".to_string(),
                        source_item_id: Some(task.source_item_id.clone()),
                        source_task_id: Some(task.id.clone()),
                        summary: format!(
                            "Integration evidence for task `{}` references commit `{commit}` that could not be inspected.",
                            task.id
                        ),
                        next_action: format!(
                            "Inspect or replace the invalid commit evidence reference: {error}"
                        ),
                    }),
                }
            }
            if !closure_seen {
                gaps.push(ReconcileGap {
                    kind: "missing_closure_trailer".to_string(),
                    source_item_id: Some(task.source_item_id.clone()),
                    source_task_id: Some(task.id.clone()),
                    summary: format!(
                        "Integrated task `{}` for `{}` has no matching Platypus-Closes trailer.",
                        task.id, task.source_item_id
                    ),
                    next_action:
                        "Create or fix an integration commit with Platypus-Closes for the source item."
                            .to_string(),
                });
            }
            if !verification_seen {
                gaps.push(ReconcileGap {
                    kind: "missing_verification_trailer".to_string(),
                    source_item_id: Some(task.source_item_id.clone()),
                    source_task_id: Some(task.id.clone()),
                    summary: format!(
                        "Integrated task `{}` for `{}` has no Platypus-Verification trailer.",
                        task.id, task.source_item_id
                    ),
                    next_action:
                        "Create or fix an integration commit with a Platypus-Verification trailer."
                            .to_string(),
                });
            }
        }

        for finding in &unresolved_required {
            gaps.push(ReconcileGap {
                kind: "unresolved_required_finding".to_string(),
                source_item_id: finding.source_item_id.clone(),
                source_task_id: finding.source_task_id.clone(),
                summary: format!(
                    "Required finding `{}` is unresolved: {}.",
                    finding.id, finding.title
                ),
                next_action: "Resolve, reject, defer, or mark the finding duplicate.".to_string(),
            });
        }

        Ok(ReconcileSnapshot {
            ok: gaps.is_empty(),
            closed_item_ids,
            completed_tasks: completed_tasks.len(),
            unresolved_required_findings: unresolved_required.len(),
            gaps,
        })
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

fn lease_snapshot(lease: LeaseRecord) -> LeaseSnapshot {
    LeaseSnapshot {
        id: lease.id,
        scope: lease.scope,
        target_id: lease.target_id,
        owner: lease.owner,
        state: match lease.status.as_str() {
            "expired" => LeaseState::Expired,
            "released" => LeaseState::Released,
            _ => LeaseState::Active,
        },
        metadata: lease.metadata,
        expires_at: lease.expires_at,
    }
}

fn lease_scope(scope: &crate::state::LeaseScope) -> &'static str {
    match scope {
        crate::state::LeaseScope::Project => "project",
        crate::state::LeaseScope::Task => "task",
    }
}

fn payload_object(payload: Option<Value>) -> BTreeMap<String, Value> {
    match payload {
        Some(Value::Object(object)) => object.into_iter().collect(),
        Some(value) => BTreeMap::from([("value".to_string(), value)]),
        None => BTreeMap::new(),
    }
}

fn safe_action<const N: usize>(
    recommended_tool: &str,
    summary: String,
    reason: &str,
    params: [(&str, &str); N],
) -> SafeActionSnapshot {
    SafeActionSnapshot {
        recommended_tool: recommended_tool.to_string(),
        summary,
        reason: reason.to_string(),
        params: map_params(params),
    }
}

fn map_params<const N: usize>(params: [(&str, &str); N]) -> BTreeMap<String, Value> {
    params
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (key.to_string(), Value::String(value.to_string())))
        .collect()
}

#[derive(Debug)]
struct AssignmentHint {
    id: String,
    task_id: String,
    source_item_id: String,
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
    closed_item_ids: &std::collections::BTreeSet<String>,
) -> rusqlite::Result<Option<AssignmentHint>> {
    let mut statement = connection.prepare(
        r#"
        SELECT worker_assignments.id, worker_assignments.task_id, tasks.source_item_id
        FROM worker_assignments
        JOIN tasks ON tasks.id = worker_assignments.task_id
        WHERE worker_assignments.status = ?1
        ORDER BY worker_assignments.updated_at ASC, worker_assignments.id ASC
        "#,
    )?;
    let rows = statement.query_map([status], |row| {
        Ok(AssignmentHint {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            source_item_id: row.get("source_item_id")?,
        })
    })?;
    for row in rows {
        let assignment = row?;
        if !closed_item_ids.contains(&assignment.source_item_id) {
            return Ok(Some(assignment));
        }
    }
    Ok(None)
}

fn queued_task(
    connection: &rusqlite::Connection,
    closed_item_ids: &std::collections::BTreeSet<String>,
) -> rusqlite::Result<Option<TaskHint>> {
    task_with_status(connection, "queued", "created_at", closed_item_ids)
}

fn claimed_task(
    connection: &rusqlite::Connection,
    closed_item_ids: &std::collections::BTreeSet<String>,
) -> rusqlite::Result<Option<TaskHint>> {
    task_with_status(connection, "claimed", "updated_at", closed_item_ids)
}

fn task_with_status(
    connection: &rusqlite::Connection,
    status: &str,
    order_column: &str,
    closed_item_ids: &std::collections::BTreeSet<String>,
) -> rusqlite::Result<Option<TaskHint>> {
    let query = format!(
        r#"
        SELECT id, source_item_id, worker
        FROM tasks
        WHERE status = ?1
        ORDER BY {order_column} ASC, id ASC
        "#
    );
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map([status], row_to_task_hint)?;
    for row in rows {
        let task = row?;
        if !closed_item_ids.contains(&task.source_item_id) {
            return Ok(Some(task));
        }
    }
    Ok(None)
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

fn completed_task_without_integration(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<Option<TaskHint>> {
    connection
        .query_row(
            r#"
            SELECT t.id, t.source_item_id, t.worker
            FROM tasks t
            WHERE t.status = 'completed'
              AND EXISTS (
                SELECT 1
                FROM evidence verification
                WHERE verification.source_task_id = t.id
                  AND verification.kind = 'verification'
              )
              AND NOT EXISTS (
                SELECT 1
                FROM evidence integration
                WHERE integration.source_task_id = t.id
                  AND integration.kind = 'commit'
              )
            ORDER BY t.updated_at ASC, t.id ASC
            LIMIT 1
            "#,
            [],
            row_to_task_hint,
        )
        .optional()
}

fn row_to_task_hint(row: &Row<'_>) -> rusqlite::Result<TaskHint> {
    Ok(TaskHint {
        id: row.get("id")?,
        source_item_id: row.get("source_item_id")?,
        worker: row.get("worker")?,
    })
}

fn transition_to_event_record(transition: RuntimeTransitionRecord) -> EventRecord {
    EventRecord {
        cursor: transition.cursor,
        event_type: transition.transition_type,
        scope: transition.domain.clone(),
        task_id: if transition.domain == "task" {
            Some(transition.entity_id)
        } else {
            None
        },
        summary: transition.summary,
        payload: transition.payload,
        created_at: transition.created_at,
        replay_order: transition.replay_order,
    }
}

fn project_event_snapshot(event: EventRecord) -> ProjectEventSnapshot {
    let cursor = event.cursor;
    let sequence = cursor
        .rsplit(':')
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(event.replay_order);
    ProjectEventSnapshot {
        cursor,
        sequence,
        scope: event.scope,
        task_id: event.task_id,
        event_type: event.event_type,
        summary: event.summary,
        payload: event.payload.map(|payload| payload_object(Some(payload))),
        created_at: event.created_at,
    }
}

fn evidence_snapshot(evidence: EvidenceRecord) -> EvidenceSnapshot {
    EvidenceSnapshot {
        id: evidence.id,
        source_item_id: evidence.source_item_id,
        source_task_id: evidence.source_task_id,
        kind: evidence.kind,
        summary: evidence.summary,
        refs: evidence.refs,
        metadata: evidence.metadata,
        created_at: evidence.created_at,
    }
}

fn finding_snapshot(finding: FindingRecord) -> FindingSnapshot {
    let disposition =
        finding
            .disposition_reason
            .as_ref()
            .map(|reason| FindingDispositionSnapshot {
                state: finding.status.clone(),
                reason: reason.clone(),
                evidence_refs: finding.evidence_refs.clone(),
            });
    FindingSnapshot {
        id: finding.id,
        source_item_id: finding.source_item_id,
        source_task_id: finding.source_task_id,
        source_finding_ref: finding.source_finding_ref,
        title: finding.title,
        status: finding.status,
        severity: finding.severity.unwrap_or_else(|| "medium".to_string()),
        required: finding.required,
        summary: finding.summary,
        owner: finding.owner,
        disposition_reason: finding.disposition_reason,
        evidence_refs: finding.evidence_refs,
        metadata: finding.metadata,
        disposition,
        created_at: finding.created_at,
        updated_at: finding.updated_at,
    }
}

fn query_evidence(
    connection: &rusqlite::Connection,
    source_item_id: Option<&str>,
    source_task_id: Option<&str>,
    kind: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<EvidenceRecord>> {
    let mut statement = connection.prepare(
        r#"
        SELECT id, source_item_id, source_task_id, kind, summary, refs_json, metadata_json, created_at
        FROM evidence
        WHERE (?1 IS NULL OR source_item_id = ?1)
          AND (?2 IS NULL OR source_task_id = ?2)
          AND (?3 IS NULL OR kind = ?3)
        ORDER BY created_at ASC, id ASC
        LIMIT ?4
        "#,
    )?;
    let rows = statement.query_map(
        params![source_item_id, source_task_id, kind, limit],
        row_to_evidence,
    )?;
    rows.collect()
}

fn get_evidence(connection: &rusqlite::Connection, id: &str) -> rusqlite::Result<EvidenceRecord> {
    connection.query_row(
        r#"
        SELECT id, source_item_id, source_task_id, kind, summary, refs_json, metadata_json, created_at
        FROM evidence
        WHERE id = ?1
        "#,
        [id],
        row_to_evidence,
    )
}

fn next_evidence_id(connection: &rusqlite::Connection) -> rusqlite::Result<String> {
    let existing: i64 =
        connection.query_row("SELECT COUNT(*) FROM evidence", [], |row| row.get(0))?;
    Ok(format!("EVD-{:03}", existing + 1))
}

fn row_to_evidence(row: &Row<'_>) -> rusqlite::Result<EvidenceRecord> {
    let refs_json: Option<String> = row.get("refs_json")?;
    let metadata_json: Option<String> = row.get("metadata_json")?;
    Ok(EvidenceRecord {
        id: row.get("id")?,
        source_item_id: row.get("source_item_id")?,
        source_task_id: row.get("source_task_id")?,
        kind: row.get("kind")?,
        summary: row.get("summary")?,
        refs: parse_json(refs_json),
        metadata: parse_json(metadata_json),
        created_at: row.get("created_at")?,
    })
}

fn query_findings(
    connection: &rusqlite::Connection,
    source_item_id: Option<&str>,
    source_task_id: Option<&str>,
    status: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<FindingRecord>> {
    let mut statement = connection.prepare(
        r#"
        SELECT *
        FROM findings
        WHERE (?1 IS NULL OR source_item_id = ?1)
          AND (?2 IS NULL OR source_task_id = ?2)
          AND (?3 IS NULL OR status = ?3)
        ORDER BY updated_at DESC, id DESC
        LIMIT ?4
        "#,
    )?;
    let rows = statement.query_map(
        params![source_item_id, source_task_id, status, limit],
        row_to_finding,
    )?;
    rows.collect()
}

fn query_unresolved_required_findings(
    connection: &rusqlite::Connection,
    source_item_id: Option<&str>,
    source_task_id: Option<&str>,
) -> rusqlite::Result<Vec<FindingRecord>> {
    let mut statement = connection.prepare(
        r#"
        SELECT *
        FROM findings
        WHERE required = 1
          AND status NOT IN ('resolved', 'rejected', 'duplicate')
          AND (?1 IS NULL OR source_item_id = ?1)
          AND (?2 IS NULL OR source_task_id = ?2)
        ORDER BY updated_at DESC, id DESC
        "#,
    )?;
    let rows = statement.query_map(params![source_item_id, source_task_id], row_to_finding)?;
    rows.collect()
}

fn get_finding(connection: &rusqlite::Connection, id: &str) -> rusqlite::Result<FindingRecord> {
    connection.query_row("SELECT * FROM findings WHERE id = ?1", [id], row_to_finding)
}

fn finding_exists(connection: &rusqlite::Connection, id: &str) -> rusqlite::Result<bool> {
    connection
        .query_row("SELECT 1 FROM findings WHERE id = ?1", [id], |_| Ok(()))
        .optional()
        .map(|value| value.is_some())
}

fn next_finding_id(
    connection: &rusqlite::Connection,
    source_item_id: Option<&str>,
    source_task_id: Option<&str>,
) -> rusqlite::Result<String> {
    let scope_prefix = source_item_id.or(source_task_id).unwrap_or("FIND");
    let prefix = safe_id_prefix(scope_prefix);
    let existing: i64 = connection.query_row(
        r#"
        SELECT COUNT(*)
        FROM findings
        WHERE (?1 IS NULL OR source_item_id = ?1)
          AND (?2 IS NULL OR source_task_id = ?2)
        "#,
        params![source_item_id, source_task_id],
        |row| row.get(0),
    )?;
    Ok(format!("{prefix}-F{:03}", existing + 1))
}

fn row_to_finding(row: &Row<'_>) -> rusqlite::Result<FindingRecord> {
    let evidence_json: Option<String> = row.get("evidence_json")?;
    let metadata_json: Option<String> = row.get("metadata_json")?;
    let required: i64 = row.get("required")?;
    Ok(FindingRecord {
        id: row.get("id")?,
        source_item_id: row.get("source_item_id")?,
        source_task_id: row.get("source_task_id")?,
        source_finding_ref: row.get("source_finding_ref")?,
        title: row.get("title")?,
        status: row.get("status")?,
        severity: row.get("severity")?,
        required: required != 0,
        summary: row.get("summary")?,
        owner: row.get("owner")?,
        disposition_reason: row.get("disposition_reason")?,
        evidence_refs: parse_json(evidence_json),
        metadata: parse_json(metadata_json),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

#[derive(Debug)]
struct CompletedTask {
    id: String,
    source_item_id: String,
}

fn query_completed_tasks(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<Vec<CompletedTask>> {
    let mut statement = connection.prepare(
        r#"
        SELECT id, source_item_id
        FROM tasks
        WHERE status = 'completed'
        ORDER BY updated_at ASC, id ASC
        "#,
    )?;
    let rows = statement.query_map([], |row| {
        Ok(CompletedTask {
            id: row.get("id")?,
            source_item_id: row.get("source_item_id")?,
        })
    })?;
    rows.collect()
}

fn query_unresolved_required_finding_summaries(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<Vec<FindingRecord>> {
    query_unresolved_required_findings(connection, None, None)
}

fn commit_ref(reference: &str) -> Option<String> {
    reference
        .trim()
        .strip_prefix("commit:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

#[derive(Debug)]
struct CommitTrailers {
    closes: BTreeSet<String>,
    verification_present: bool,
}

fn commit_trailers(root: &Path, commit: &str) -> Result<CommitTrailers, String> {
    let message = run_git(root, &["show", "-s", "--format=%B", commit])?;
    let trailers = git_trailers::parse_platypus_trailers(&message);
    Ok(CommitTrailers {
        closes: trailers.closes,
        verification_present: trailers.verification_present,
    })
}

fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run git: {error}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) if started.elapsed() < GIT_TIMEOUT => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                return Err("git command timed out".to_string());
            }
            Err(error) => return Err(format!("could not wait for git: {error}")),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("could not collect git output: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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

fn clean_vec(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn json_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn parse_json<T>(raw: Option<String>) -> T
where
    T: DeserializeOwned + Default,
{
    raw.and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn safe_id_prefix(value: &str) -> String {
    let prefix: String = value
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                Some(character.to_ascii_uppercase())
            } else if character == '-' || character == '_' {
                Some('-')
            } else {
                None
            }
        })
        .collect();
    let prefix = prefix.trim_matches('-');
    if prefix.is_empty() {
        "FIND".to_string()
    } else {
        prefix.to_string()
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
    use std::{collections::BTreeMap, fs, process::Command};
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
    fn records_evidence_and_findings_through_project_state() {
        let project = TempDir::new().expect("temp dir");
        let state = SqliteProjectState::open(project.path(), None).expect("state");

        let evidence = state
            .record_evidence(RecordEvidenceCommand {
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some("task-1".to_string()),
                kind: "verification".to_string(),
                summary: "make check passed".to_string(),
                refs: vec!["log:1".to_string()],
                metadata: BTreeMap::new(),
            })
            .expect("evidence");
        assert_eq!(evidence.id, "EVD-001");

        let finding = state
            .record_finding(RecordFindingCommand {
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some("task-1".to_string()),
                source_finding_ref: None,
                title: "Follow-up".to_string(),
                summary: "Document the follow-up.".to_string(),
                severity: Some("medium".to_string()),
                required: Some(true),
                evidence_refs: vec![format!("evidence:{}", evidence.id)],
                metadata: BTreeMap::new(),
            })
            .expect("finding");
        assert_eq!(finding.id, "PROJ-001-F001");

        let validation = state
            .validate_findings(ValidateFindingsQuery {
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: None,
            })
            .expect("validation");
        assert!(!validation.ok);
        assert_eq!(validation.unresolved_required[0].id, finding.id);

        let updated = state
            .update_finding_disposition(UpdateFindingDispositionCommand {
                finding_id: finding.id,
                status: "resolved".to_string(),
                owner: Some("manager".to_string()),
                disposition_reason: Some("covered".to_string()),
                evidence_refs: vec!["commit:HEAD".to_string()],
                metadata: BTreeMap::new(),
            })
            .expect("updated");
        assert_eq!(updated.status, "resolved");
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
        git(&project, &["add", "backlog"]);
        git(&project, &["commit", "-m", "Add backlog fixture"]);
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
