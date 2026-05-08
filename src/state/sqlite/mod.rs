use crate::{
    guidance,
    models::{ActionStatus, NextSafeActionParams, TaskRecord},
    state::{
        AcquireLeaseCommand, AppendWorkerEventCommand, ApprovalSnapshot, AssignmentQuery,
        AssignmentSnapshot, BackendCapabilities, BackendInfo, CompleteExecutionCommand,
        DispatchWorkCommand, DispatchWorkOutcome, EventReplaySnapshot, FindingsQuery,
        FindingsSnapshot, FindingsValidationSnapshot, IntegrateResultCommand, LeaseSnapshot,
        NextSafeActionQuery, PrepareAssignmentCommand, ProjectState, ProjectStateError,
        ReconcileProjectQuery, ReconcileSnapshot, ReplayEventsQuery, ResolveApprovalCommand,
        SafeActionSnapshot, StartExecutionCommand, StateResult, TaskLifecycleState, TaskQuery,
        TaskSnapshot, ValidateFindingsQuery, WorkerEventSnapshot, WorkerWorkspaceSnapshot,
    },
    storage::{self, StorageConnection, TaskStore},
};
use std::path::{Path, PathBuf};

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

    fn dispatch_work(&self, _command: DispatchWorkCommand) -> StateResult<DispatchWorkOutcome> {
        self.unsupported("dispatch_work")
    }

    fn prepare_assignment(
        &self,
        _command: PrepareAssignmentCommand,
    ) -> StateResult<AssignmentSnapshot> {
        self.unsupported("prepare_assignment")
    }

    fn start_execution(&self, _command: StartExecutionCommand) -> StateResult<AssignmentSnapshot> {
        self.unsupported("start_execution")
    }

    fn append_worker_event(
        &self,
        _command: AppendWorkerEventCommand,
    ) -> StateResult<WorkerEventSnapshot> {
        self.unsupported("append_worker_event")
    }

    fn complete_execution(
        &self,
        _command: CompleteExecutionCommand,
    ) -> StateResult<AssignmentSnapshot> {
        self.unsupported("complete_execution")
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

    fn inspect_assignment(&self, _query: AssignmentQuery) -> StateResult<AssignmentSnapshot> {
        self.unsupported("inspect_assignment")
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

fn task_snapshot(task: TaskRecord) -> TaskSnapshot {
    TaskSnapshot {
        id: task.id,
        source_item_id: task.source_item_id,
        title: task.title,
        state: task_state(&task.status),
        worker: task.worker,
        claimed_by: task.claimed_by,
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
    fn unmigrated_methods_return_unsupported() {
        let project = TempDir::new().expect("temp dir");
        let state = SqliteProjectState::open(project.path(), None).expect("state");

        let error = state
            .dispatch_work(DispatchWorkCommand {
                summary: None,
                preferred_worker: None,
            })
            .expect_err("unsupported");

        assert!(matches!(error, ProjectStateError::Unsupported { .. }));
    }
}
