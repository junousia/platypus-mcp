use crate::models::{ExternalRef, TaskBundle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Backend identity and coordination guarantees.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BackendInfo {
    pub name: String,
    pub version: Option<String>,
    pub capabilities: BackendCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct BackendCapabilities {
    pub durable: bool,
    pub transactional_lifecycle: bool,
    pub stable_event_replay: bool,
    pub leases: bool,
    pub shared_coordination: bool,
    pub migrations: bool,
    pub offline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DispatchWorkOutcome {
    pub task: TaskSnapshot,
    pub candidate: BacklogCandidateSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct BacklogCandidateSnapshot {
    pub source: String,
    pub item_id: String,
    pub title: String,
    pub priority: String,
    pub area: String,
    pub item_type: String,
    pub suggested_worker: Option<String>,
    pub owned_surfaces: Vec<String>,
    pub external_refs: Vec<ExternalRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TaskSnapshot {
    pub id: String,
    pub source_item_id: String,
    pub title: String,
    pub status: String,
    pub state: TaskLifecycleState,
    pub worker: Option<String>,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub worker_workspace: Option<WorkerWorkspaceSnapshot>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WorkerWorkspaceSnapshot {
    pub path: String,
    pub branch: String,
    pub base_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLifecycleState {
    Queued,
    Claimed,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AssignmentSnapshot {
    pub id: String,
    pub task_id: String,
    pub worker: Option<String>,
    pub state: AssignmentLifecycleState,
    pub reused_existing: bool,
    pub assigned_by: Option<String>,
    pub worker_session: Option<String>,
    pub worktree_path: String,
    pub bundle: TaskBundle,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub result_status: Option<String>,
    pub summary: Option<String>,
    pub changed_files: Vec<String>,
    pub verification_status: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentLifecycleState {
    Prepared,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct WorkerEventSnapshot {
    pub assignment_id: String,
    pub task_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub summary: String,
    pub payload: BTreeMap<String, Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ApprovalSnapshot {
    pub id: String,
    pub state: ApprovalState,
    pub requested_by: String,
    pub responder: Option<String>,
    pub reason: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Pending,
    Approved,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct LeaseSnapshot {
    pub id: String,
    pub scope: String,
    pub target_id: String,
    pub owner: String,
    pub state: LeaseState,
    pub metadata: BTreeMap<String, Value>,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LeaseState {
    Active,
    Expired,
    Released,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct IntegrationSnapshot {
    pub task_id: String,
    pub source_item_id: String,
    pub strategy: String,
    pub evidence_refs: Vec<String>,
    pub integrated_by: String,
    pub integrated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SafeActionSnapshot {
    pub recommended_tool: String,
    pub reason: String,
    pub params: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EventReplaySnapshot {
    pub events: Vec<ProjectEventSnapshot>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ProjectEventSnapshot {
    pub sequence: i64,
    pub scope: String,
    pub task_id: Option<String>,
    pub event_type: String,
    pub summary: String,
    pub payload: BTreeMap<String, Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct FindingsSnapshot {
    pub findings: Vec<FindingSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct FindingSnapshot {
    pub id: String,
    pub source_item_id: String,
    pub source_task_id: Option<String>,
    pub title: String,
    pub severity: String,
    pub required: bool,
    pub disposition: Option<FindingDispositionSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct FindingDispositionSnapshot {
    pub state: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FindingsValidationSnapshot {
    pub ok: bool,
    pub unresolved_required: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ReconcileSnapshot {
    pub ok: bool,
    pub closed_item_ids: BTreeSet<String>,
    pub completed_tasks: usize,
    pub gaps: Vec<ReconcileGap>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ReconcileGap {
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
    pub summary: String,
    pub recovery: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_capabilities_default_to_false() {
        let capabilities = BackendCapabilities::default();

        assert!(!capabilities.durable);
        assert!(!capabilities.shared_coordination);
    }
}
