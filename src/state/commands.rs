use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Command to choose backlog work and queue a durable task.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DispatchWorkCommand {
    pub summary: Option<String>,
    pub preferred_worker: Option<String>,
    pub source_item_id: Option<String>,
}

/// Command to prepare a worker handoff for a queued or claimed task.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PrepareAssignmentCommand {
    pub task_id: Option<String>,
    pub worker: Option<String>,
    pub claimant: String,
    pub execution_mode: Option<String>,
    pub base_ref: Option<String>,
    pub verification_command: Vec<String>,
}

/// Command to mark a prepared worker handoff as running.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StartExecutionCommand {
    pub assignment_id: String,
    pub worker_session: Option<String>,
}

/// Command to append a worker progress, tool, or result event.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct AppendWorkerEventCommand {
    pub assignment_id: String,
    pub event_type: String,
    pub summary: String,
    pub payload: BTreeMap<String, Value>,
}

/// Command to finish a worker assignment.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CompleteExecutionCommand {
    pub assignment_id: String,
    pub status: String,
    pub summary: String,
    pub changed_files: Vec<String>,
    pub verification_status: Option<String>,
}

/// Command to answer one pending approval.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ResolveApprovalCommand {
    pub approval_id: String,
    pub response: ApprovalResponse,
    pub responder: String,
    pub reason: Option<String>,
}

/// Command to acquire temporary ownership of project or task activity.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct AcquireLeaseCommand {
    pub scope: LeaseScope,
    pub target_id: String,
    pub owner: String,
    pub ttl_seconds: u64,
    pub metadata: BTreeMap<String, Value>,
}

/// Command to attach integration proof to completed work.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct IntegrateResultCommand {
    pub task_id: String,
    pub strategy: IntegrationStrategy,
    pub verifier: String,
    pub evidence_refs: Vec<String>,
}

/// Command to attach worker workspace metadata to one task.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct RecordWorkspaceCommand {
    pub task_id: String,
    pub path: String,
    pub branch: String,
    pub base_ref: String,
}

/// Command to remove worker workspace metadata from one task.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ClearWorkspaceCommand {
    pub task_id: String,
}

/// Command to record audit or verification evidence.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct RecordEvidenceCommand {
    pub id: Option<String>,
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
    pub kind: String,
    pub summary: String,
    pub refs: Vec<String>,
    pub metadata: BTreeMap<String, Value>,
}

/// Command to record an implementation finding or follow-up.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct RecordFindingCommand {
    pub id: Option<String>,
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
    pub source_finding_ref: Option<String>,
    pub title: String,
    pub summary: String,
    pub severity: Option<String>,
    pub required: Option<bool>,
    pub evidence_refs: Vec<String>,
    pub metadata: BTreeMap<String, Value>,
}

/// Command to update the disposition of one finding.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct UpdateFindingDispositionCommand {
    pub finding_id: String,
    pub status: String,
    pub owner: Option<String>,
    pub disposition_reason: Option<String>,
    pub evidence_refs: Vec<String>,
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalResponse {
    Approved,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LeaseScope {
    Project,
    Task,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationStrategy {
    MergeCommit,
    FastForward,
    Squash,
    External,
}

/// Query for safe-action guidance.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct NextSafeActionQuery {
    pub worker: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TaskQuery {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AssignmentQuery {
    pub assignment_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct ReplayEventsQuery {
    pub task_id: Option<String>,
    pub scope: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct EvidenceQuery {
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct FindingsQuery {
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
    pub status: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct ValidateFindingsQuery {
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct ReconcileProjectQuery {
    pub include_closed_items: bool,
}
