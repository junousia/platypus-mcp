//! Backend-independent project state boundary.
//!
//! This module defines the domain-shaped contract that runtime backends should
//! implement. It is intentionally not wired into the current SQLite-backed tool
//! paths yet; the initial slice gives future migration work a typed target that
//! is independent from any concrete persistence model.

mod commands;
mod error;
pub mod sqlite;
mod types;

pub use commands::*;
pub use error::{ProjectStateError, StateResult};
pub use types::*;

/// Durable state operations for one Platypus project.
///
/// Implementations must preserve the atomicity guarantees documented on each
/// command method. The trait is shaped around Platypus behavior, not storage
/// layout. Callers should build one command, invoke one method, and translate
/// the outcome into an MCP result.
pub trait ProjectState {
    /// Describe backend identity and supported coordination guarantees.
    fn describe_backend(&self) -> StateResult<BackendInfo>;

    /// Atomically choose runnable backlog work and create a queued task.
    fn dispatch_work(&self, command: DispatchWorkCommand) -> StateResult<DispatchWorkOutcome>;

    /// Atomically claim a task, prepare assignment metadata, and append audit
    /// events. If preparation cannot complete, the implementation must leave a
    /// retryable task state and replayable failure event.
    fn prepare_assignment(
        &self,
        command: PrepareAssignmentCommand,
    ) -> StateResult<AssignmentSnapshot>;

    /// Atomically mark a prepared assignment and its task as running.
    fn start_execution(&self, command: StartExecutionCommand) -> StateResult<AssignmentSnapshot>;

    /// Atomically append bounded worker progress for a running assignment.
    fn append_worker_event(
        &self,
        command: AppendWorkerEventCommand,
    ) -> StateResult<WorkerEventSnapshot>;

    /// Atomically persist worker result metadata and finish the task.
    fn complete_execution(
        &self,
        command: CompleteExecutionCommand,
    ) -> StateResult<AssignmentSnapshot>;

    /// Atomically move one pending approval into an approved or denied state.
    fn resolve_approval(&self, command: ResolveApprovalCommand) -> StateResult<ApprovalSnapshot>;

    /// Atomically acquire a project or task lease when no active conflict
    /// exists.
    fn acquire_lease(&self, command: AcquireLeaseCommand) -> StateResult<LeaseSnapshot>;

    /// Atomically attach integration proof to a completed worker result.
    fn integrate_result(&self, command: IntegrateResultCommand)
        -> StateResult<IntegrationSnapshot>;

    /// Inspect the next safe domain operation for the current project state.
    fn next_safe_action(&self, query: NextSafeActionQuery) -> StateResult<SafeActionSnapshot>;

    /// Inspect one task snapshot.
    fn inspect_task(&self, query: TaskQuery) -> StateResult<TaskSnapshot>;

    /// Inspect one worker assignment snapshot.
    fn inspect_assignment(&self, query: AssignmentQuery) -> StateResult<AssignmentSnapshot>;

    /// Replay durable project, task, worker, approval, and integration events.
    fn replay_events(&self, query: ReplayEventsQuery) -> StateResult<EventReplaySnapshot>;

    /// List findings attached to backlog items or tasks.
    fn list_findings(&self, query: FindingsQuery) -> StateResult<FindingsSnapshot>;

    /// Validate finding disposition requirements for a backlog item or task.
    fn validate_findings(
        &self,
        query: ValidateFindingsQuery,
    ) -> StateResult<FindingsValidationSnapshot>;

    /// Reconcile backlog closure, task state, findings, evidence, and
    /// integration proof into one project health snapshot.
    fn reconcile_project(&self, query: ReconcileProjectQuery) -> StateResult<ReconcileSnapshot>;
}

#[cfg(test)]
mod tests {
    #[test]
    fn state_boundary_avoids_backend_specific_terms() {
        let public_sources = [
            include_str!("commands.rs"),
            include_str!("types.rs"),
            include_str!("error.rs"),
        ]
        .join("\n")
        .to_ascii_lowercase();

        let tokens = public_sources
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .collect::<Vec<_>>();
        for forbidden in ["rusqlite", "query_row", "table", "crud"] {
            assert!(
                !tokens.contains(&forbidden),
                "state boundary should not expose `{forbidden}`"
            );
        }
    }
}
