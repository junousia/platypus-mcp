use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

fn example_goal() -> String {
    "Build a FastAPI and React web app with a tested vertical slice.".to_string()
}

fn example_title() -> String {
    "Implement the first webapp vertical slice".to_string()
}

fn example_item_id() -> String {
    "PROJ-001".to_string()
}

fn example_item_ids() -> Vec<String> {
    vec!["PROJ-001".to_string(), "PROJ-002".to_string()]
}

fn example_id_prefix() -> String {
    "PROJ".to_string()
}

fn example_epic_id() -> String {
    "webapp".to_string()
}

fn example_client_key() -> String {
    "frontend_foundation".to_string()
}

fn example_task_id() -> String {
    "PROJ-001-T001".to_string()
}

fn example_assignment_id() -> String {
    "PROJ-001-T001-A001".to_string()
}

fn example_worker() -> String {
    "coder".to_string()
}

fn example_verification_command() -> Vec<String> {
    vec!["make check".to_string()]
}

fn example_owned_surfaces() -> Vec<String> {
    vec!["backend/".to_string(), "frontend/".to_string()]
}

fn example_event_type() -> String {
    "worker.progress".to_string()
}

fn example_summary() -> String {
    "Implemented the requested change and recorded verification evidence.".to_string()
}

fn example_true() -> bool {
    true
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PingParams {
    /// Message text for this request or result.
    pub message: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RootParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BacklogPrioritySchema {
    P0,
    P1,
    P2,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BacklogItemTypeSchema {
    Foundation,
    Feature,
    Safety,
    Ux,
    Test,
    Docs,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EpicStatusSchema {
    Active,
    Archived,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionModeSchema {
    Auto,
    ManualHandoff,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskPlanModeSchema {
    Minimal,
    Standard,
    Full,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BacklogExecutionPathSchema {
    DirectEdit,
    WorkerHandoff,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanningGateSchema {
    None,
    TaskPlan,
    ApprovedTaskPlan,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LeaseScopeSchema {
    Project,
    Task,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LeaseStatusSchema {
    Active,
    Released,
    Expired,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationStrategySchema {
    MergeCommit,
    FastForward,
    Squash,
    ApplyChangedFiles,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecisionSchema {
    Approve,
    Deny,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatusSchema {
    Pending,
    Approved,
    Denied,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkerTerminalStatusSchema {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatusSchema {
    Passed,
    Failed,
    Skipped,
    NotRun,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostActionKindSchema {
    DirectEdit,
    RunInWorktree,
    VerifyOrRecordRisk,
    ResolveFindings,
    IntegrateResult,
    InspectOrRecover,
    Done,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkQueueStateSchema {
    Ready,
    DirectReady,
    PlanningBlocked,
    ApprovalBlocked,
    DependencyBlocked,
    ConfigBlocked,
    WorkspaceBlocked,
    Active,
    CompletedPendingIntegration,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkExecutionPathSchema {
    DirectEdit,
    WorkerHandoff,
    Blocked,
    Active,
    PendingIntegration,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreparedStateSchema {
    NotPrepared,
    DirectPrepared,
    WorktreePrepared,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKindSchema {
    Commit,
    Verification,
    FileSummary,
    WorkerFinding,
    ManagerDisposition,
    ExternalReport,
    Note,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeveritySchema {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingDispositionStatusSchema {
    Open,
    Accepted,
    Resolved,
    Rejected,
    Deferred,
    Duplicate,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DispatchReadyWorkParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Restrict dispatch to this backlog item id instead of selecting from the
    /// global runnable queue.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub item_id: Option<String>,
    /// Maximum number of tasks to include or process.
    #[schemars(range(min = 1, max = 10))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub max_tasks: Option<usize>,
    /// Worker name associated with this item.
    pub worker: Option<String>,
    /// Name recorded as the task claimant.
    pub claimant: Option<String>,
    /// Execution mode for this dispatch. Omit this or use manual_handoff to
    /// prepare lifecycle-safe worktree handoffs for an external MCP host or
    /// human-managed worker. Platypus does not launch or configure workers.
    #[schemars(with = "Option<ExecutionModeSchema>")]
    pub execution_mode: Option<String>,
    /// Whether to claim tasks, create worktrees, and persist worker handoff bundles during dispatch.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub prepare_handoffs: Option<bool>,
    /// Whether to immediately mark prepared assignments as running in the local
    /// lifecycle state machine. This does not launch an external worker process.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub auto_start: Option<bool>,
    /// Whether dispatch should auto-commit tracked planning artifacts when local
    /// dirt is limited to `backlog/items/*.md` and `backlog/plans/*.yaml`.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub auto_commit_artifacts: Option<bool>,
    /// Deprecated compatibility hint. Durable planning approval policy now
    /// comes from `workflow.execution` or backlog item `planning_gate`.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub require_planning_approval: Option<bool>,
    /// Preview dispatchable work without mutating task, assignment, or Git
    /// lifecycle state.
    #[schemars(example = example_true())]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub dry_run: Option<bool>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Verification command to run or record for this item.
    pub verification_command: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrepareWorkParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Restrict preparation to this backlog item id instead of selecting from
    /// the global runnable queue.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub item_id: Option<String>,
    /// Maximum number of tasks to prepare for host-run execution.
    #[schemars(range(min = 1, max = 10))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub max_tasks: Option<usize>,
    /// Worker name associated with this item.
    #[schemars(example = example_worker())]
    pub worker: Option<String>,
    /// Name recorded as the task claimant.
    pub claimant: Option<String>,
    /// Execution mode to prepare. Omit this for manual_handoff, which prepares
    /// a worktree for the MCP host or a human-managed worker. Platypus does not
    /// launch or configure workers.
    #[schemars(with = "Option<ExecutionModeSchema>")]
    pub execution_mode: Option<String>,
    /// Deprecated compatibility hint. Durable task-plan policy now comes from
    /// `workflow.execution` or backlog item `planning_gate`.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub require_task_plan: Option<bool>,
    /// Deprecated compatibility hint. Durable approval policy now comes from
    /// `workflow.execution` or backlog item `planning_gate`.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub require_planning_approval: Option<bool>,
    /// Whether preparation should auto-commit tracked backlog and task-plan
    /// artifacts when those are the only manager workspace changes.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub auto_commit_artifacts: Option<bool>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Verification command to run or record for this item.
    #[schemars(example = example_verification_command())]
    pub verification_command: Vec<String>,
    /// Include the full inspect_work_queue snapshot in the response. Omit or
    /// set false for a compact host handoff.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub include_queue_snapshot: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectWorkQueueParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
    /// Deprecated compatibility hint. Durable task-plan policy now comes from
    /// `workflow.execution` or backlog item `planning_gate`.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub require_task_plan: Option<bool>,
    /// Deprecated compatibility hint. Durable approval policy now comes from
    /// `workflow.execution` or backlog item `planning_gate`.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub require_planning_approval: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectSessionParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Maximum number of queue records to return.
    #[schemars(range(min = 1, max = 200))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
    /// Deprecated compatibility hint. Durable task-plan policy now comes from
    /// `workflow.execution` or backlog item `planning_gate`.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub require_task_plan: Option<bool>,
    /// Deprecated compatibility hint. Durable approval policy now comes from
    /// `workflow.execution` or backlog item `planning_gate`.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub require_planning_approval: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectItemParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Backlog item identifier to inspect.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub item_id: String,
    /// Deprecated compatibility hint. Durable task-plan policy now comes from
    /// `workflow.execution` or backlog item `planning_gate`.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub require_task_plan: Option<bool>,
    /// Deprecated compatibility hint. Durable approval policy now comes from
    /// `workflow.execution` or backlog item `planning_gate`.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub require_planning_approval: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InitProjectParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Project name written into initialized Platypus configuration.
    pub project_name: Option<String>,
    /// Whether to overwrite an existing file or record.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LimitParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AcquireLeaseParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Scope for this record or operation.
    #[schemars(with = "LeaseScopeSchema")]
    pub scope: String,
    /// Target identifier for this record.
    pub target_id: String,
    /// Owner name or repository owner, depending on context.
    pub owner: String,
    /// Time to live in seconds.
    #[schemars(range(min = 1, max = 86400))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_u64")]
    pub ttl_seconds: Option<u64>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_map")]
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListLeasesParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Scope for this record or operation.
    #[schemars(with = "Option<LeaseScopeSchema>")]
    pub scope: Option<String>,
    /// Target identifier for this record.
    pub target_id: Option<String>,
    /// Lifecycle or result status for this record.
    #[schemars(with = "Option<LeaseStatusSchema>")]
    pub status: Option<String>,
    /// Whether expired records should be included.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub include_expired: Option<bool>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenewLeaseParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Lease identifier.
    pub lease_id: String,
    /// Owner name or repository owner, depending on context.
    pub owner: String,
    /// Time to live in seconds.
    #[schemars(range(min = 1, max = 86400))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_u64")]
    pub ttl_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReleaseLeaseParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Lease identifier.
    pub lease_id: String,
    /// Owner name or repository owner, depending on context.
    pub owner: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ValidateBacklogParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Whether validation errors should be included.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub include_errors: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RequestPlanningApprovalParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Backlog item IDs covered by this planning approval.
    #[schemars(example = example_item_ids())]
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    pub item_ids: Vec<String>,
    /// Person or agent requesting the approval.
    pub requested_by: Option<String>,
    /// Human-readable summary of the plan or backlog tranche being approved.
    pub summary: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectDependencyGraphParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Backlog item ID to focus on. When set, the graph includes that item,
    /// its dependency ancestors, and its dependent descendants.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub focus_item_id: Option<String>,
    /// Whether closed backlog items should be included as graph nodes.
    ///
    /// Closed state is derived from reachable Platypus-Closes Git trailers.
    #[schemars(example = example_true())]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub include_closed: Option<bool>,
    /// Maximum number of graph nodes to return after focus filtering.
    ///
    /// Defaults to 200 and is capped at 500.
    #[schemars(range(min = 1, max = 500))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DraftExternalBacklogItemsParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// External provider name.
    pub provider: String,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec")]
    /// External work records supplied by the host client.
    pub records: Vec<ExternalWorkRecord>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Owned code or documentation surfaces for this item.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Vec<String>,
    #[serde(default)]
    /// Verification command to run or record for this item.
    #[schemars(example = example_verification_command())]
    pub verification_command: Vec<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportGitHubIssuesParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Owner name or repository owner, depending on context.
    pub owner: String,
    /// Repository name in the external provider.
    pub repo: String,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec")]
    /// Issue records supplied by the host client.
    pub issues: Vec<GitHubIssueRecord>,
    /// State or status value.
    pub state: Option<String>,
    /// Identifier prefix used when allocating new local backlog item IDs.
    #[schemars(example = example_id_prefix(), pattern(r"^[A-Z]+$"))]
    pub id_prefix: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Owned code or documentation surfaces for this item.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Vec<String>,
    #[serde(default)]
    /// Verification command to run or record for this item.
    #[schemars(example = example_verification_command())]
    pub verification_command: Vec<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DraftExternalReportParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Source backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub source_item_id: String,
    /// Source task identifier.
    #[schemars(example = example_task_id())]
    pub source_task_id: Option<String>,
    /// External provider name.
    pub provider: Option<String>,
    /// Kind or category for this record.
    pub kind: Option<String>,
    /// External system identifier.
    pub external_id: Option<String>,
    /// Type of external report to draft or record.
    pub report_type: Option<String>,
    /// Human-readable title or short label.
    pub title: Option<String>,
    /// Maximum number of evidence records to include.
    #[schemars(range(min = 1, max = 200))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub evidence_limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RequestExternalReportApprovalParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Draft payload prepared for approval or dispatch.
    pub draft: ExternalReportDraft,
    /// Person or agent requesting the approval.
    pub requested_by: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordExternalReportDispatchParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Approval identifier.
    pub approval_id: String,
    /// External provider name.
    pub provider: String,
    /// Kind or category for this record.
    pub kind: String,
    /// External system identifier.
    pub external_id: String,
    /// Type of external report to draft or record.
    pub report_type: String,
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    /// Provider reference or URL produced by a successful external dispatch.
    pub outbound_ref: Option<String>,
    /// Error message returned when the action failed.
    pub error: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_map")]
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[schemars(inline)]
#[serde(deny_unknown_fields)]
pub struct GitHubIssueRecord {
    /// Issue or record number in the source system.
    pub number: u64,
    /// Human-readable title or short label.
    pub title: String,
    /// Body text supplied by or generated for the external record.
    pub body: Option<String>,
    /// State or status value.
    pub state: Option<String>,
    /// Canonical URL for this record.
    pub url: Option<String>,
    #[serde(default)]
    /// Labels or tags associated with this record.
    pub labels: Vec<String>,
    #[serde(default)]
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
    /// Timestamp when this record was last updated.
    pub updated_at: Option<String>,
    /// Hash of the source payload or content.
    pub source_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[schemars(inline)]
#[serde(deny_unknown_fields)]
pub struct ExternalWorkRecord {
    /// Kind or category for this record.
    pub kind: String,
    /// Stable local record identifier.
    pub id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Body text supplied by or generated for the external record.
    pub body: Option<String>,
    /// Canonical URL for this record.
    pub url: Option<String>,
    /// Provider-specific locator or reference.
    pub locator: Option<String>,
    #[serde(default)]
    /// Labels or tags associated with this record.
    pub labels: Vec<String>,
    #[serde(default)]
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
    /// Hash of the source payload or content.
    pub source_hash: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateBacklogItemParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Stable local record identifier. Usually omit this and let Platypus allocate
    /// the next ID from id_prefix; provide it only when mirroring an existing
    /// external identifier or preserving a human-chosen sequence.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub id: Option<String>,
    /// Identifier prefix used when allocating new local backlog item IDs when id
    /// is omitted. Use short project or domain prefixes such as PROJ, WEB, API,
    /// DOC, or OPS.
    #[schemars(example = example_id_prefix(), pattern(r"^[A-Z]+$"))]
    pub id_prefix: Option<String>,
    #[serde(default)]
    /// Human-readable title or short label.
    #[schemars(example = example_title())]
    pub title: String,
    /// Backlog priority. Supported values: P0, P1, P2.
    #[schemars(description = "Backlog priority. Supported values: P0, P1, P2.")]
    #[schemars(with = "Option<BacklogPrioritySchema>")]
    pub priority: Option<String>,
    /// Backlog item type. Supported values: foundation, feature, safety, ux, test, docs.
    #[serde(rename = "type")]
    #[schemars(
        description = "Backlog item type. Supported values: foundation, feature, safety, ux, test, docs."
    )]
    #[schemars(with = "Option<BacklogItemTypeSchema>")]
    /// Backlog item type.
    pub item_type: Option<String>,
    /// Primary area or surface for this item.
    pub area: Option<String>,
    /// Epic or grouping identifier for this item.
    pub epic: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Dependencies that must be satisfied first.
    #[schemars(inner(pattern(r"^[A-Z]+-[0-9]{3}$")))]
    pub depends_on: Vec<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Relative paths or top-level areas the work is expected to touch. Use broad
    /// directories for early scaffolding, for example `backend/` and `frontend/`.
    /// These values guide planning mode and later changed-file validation; use an
    /// empty list only when the surface is genuinely unknown.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Vec<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec")]
    /// External references tied to this record.
    pub external_refs: Vec<ExternalRef>,
    /// Durable execution path for this item. Omit to use workflow.execution
    /// defaults. Use direct_edit for manager-workspace edits and worker_handoff
    /// for isolated worktree handoff.
    #[schemars(with = "Option<BacklogExecutionPathSchema>")]
    pub execution_path: Option<String>,
    /// Durable planning gate for this item. Omit to use the default gate for
    /// its execution_path. Use none, task_plan, or approved_task_plan.
    #[schemars(with = "Option<PlanningGateSchema>")]
    pub planning_gate: Option<String>,
    #[serde(default)]
    /// Goal text that drives this request or record.
    #[schemars(example = example_goal())]
    pub goal: String,
    /// Implementation contract text for this backlog item.
    pub implementation_contract: Option<String>,
    /// Optional contract text for this backlog item.
    pub contract: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Acceptance criteria for this item.
    pub acceptance: Vec<String>,
    /// Optional notes for this item.
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateBacklogItemsParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Identifier prefix used when allocating new local backlog item IDs when an
    /// item id is omitted. Item-level id_prefix overrides this value.
    #[schemars(example = example_id_prefix(), pattern(r"^[A-Z]+$"))]
    pub id_prefix: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec")]
    /// Backlog items to create atomically. If any item is invalid, no item files
    /// are written.
    pub items: Vec<CreateBacklogItemsEntry>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(inline)]
pub struct CreateBacklogItemsEntry {
    /// Caller-provided key for this item inside the batch. Use it from
    /// depends_on_keys to express dependencies between new items before IDs are
    /// allocated.
    #[schemars(example = example_client_key())]
    pub client_key: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Client keys from this same batch that this item depends on. Platypus
    /// resolves these keys to allocated or explicit backlog item IDs before
    /// writing files.
    pub depends_on_keys: Vec<String>,
    /// Stable local record identifier. Usually omit this and let Platypus allocate
    /// the next ID from id_prefix; provide it only when mirroring an existing
    /// external identifier or preserving a human-chosen sequence.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub id: Option<String>,
    /// Identifier prefix used when allocating this item ID when id is omitted.
    #[schemars(example = example_id_prefix(), pattern(r"^[A-Z]+$"))]
    pub id_prefix: Option<String>,
    #[serde(default)]
    /// Human-readable title or short label.
    #[schemars(example = example_title())]
    pub title: String,
    /// Backlog priority. Supported values: P0, P1, P2.
    #[schemars(with = "Option<BacklogPrioritySchema>")]
    pub priority: Option<String>,
    /// Backlog item type. Supported values: foundation, feature, safety, ux, test, docs.
    #[serde(rename = "type")]
    #[schemars(with = "Option<BacklogItemTypeSchema>")]
    pub item_type: Option<String>,
    /// Primary area or surface for this item.
    pub area: Option<String>,
    /// Epic or grouping identifier for this item.
    pub epic: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Existing backlog item IDs that must be satisfied before this item.
    #[schemars(inner(pattern(r"^[A-Z]+-[0-9]{3}$")))]
    pub depends_on: Vec<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Relative paths or top-level areas the work is expected to touch.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Vec<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec")]
    /// External references tied to this record.
    pub external_refs: Vec<ExternalRef>,
    /// Durable execution path for this item. Omit to use workflow.execution
    /// defaults. Use direct_edit for manager-workspace edits and worker_handoff
    /// for isolated worktree handoff.
    #[schemars(with = "Option<BacklogExecutionPathSchema>")]
    pub execution_path: Option<String>,
    /// Durable planning gate for this item. Omit to use the default gate for
    /// its execution_path. Use none, task_plan, or approved_task_plan.
    #[schemars(with = "Option<PlanningGateSchema>")]
    pub planning_gate: Option<String>,
    #[serde(default)]
    /// Goal text that drives this request or record.
    #[schemars(example = example_goal())]
    pub goal: String,
    /// Implementation contract text for this backlog item.
    pub implementation_contract: Option<String>,
    /// Optional contract text for this backlog item.
    pub contract: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Acceptance criteria for this item.
    pub acceptance: Vec<String>,
    /// Optional notes for this item.
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateBacklogItemParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub item_id: String,
    /// Human-readable title or short label.
    #[schemars(example = example_title())]
    pub title: Option<String>,
    /// Backlog priority. Supported values: P0, P1, P2.
    #[schemars(with = "Option<BacklogPrioritySchema>")]
    pub priority: Option<String>,
    /// Backlog item type. Supported values: foundation, feature, safety, ux, test, docs.
    #[serde(rename = "type")]
    #[schemars(with = "Option<BacklogItemTypeSchema>")]
    pub item_type: Option<String>,
    /// Primary area or surface for this item.
    pub area: Option<String>,
    /// Epic or grouping identifier for this item.
    pub epic: Option<String>,
    /// Existing backlog item IDs that must be satisfied before this item.
    #[schemars(inner(pattern(r"^[A-Z]+-[0-9]{3}$")))]
    pub depends_on: Option<Vec<String>>,
    /// Relative paths or top-level areas the work is expected to touch.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Option<Vec<String>>,
    /// External references tied to this record.
    pub external_refs: Option<Vec<ExternalRef>>,
    /// Durable execution path for this item. Use direct_edit for
    /// manager-workspace edits and worker_handoff for isolated worktree handoff.
    #[schemars(with = "Option<BacklogExecutionPathSchema>")]
    pub execution_path: Option<String>,
    /// Durable planning gate for this item. Use none, task_plan, or approved_task_plan.
    #[schemars(with = "Option<PlanningGateSchema>")]
    pub planning_gate: Option<String>,
    /// Goal text that drives this request or record.
    #[schemars(example = example_goal())]
    pub goal: Option<String>,
    /// Implementation contract text for this backlog item.
    pub implementation_contract: Option<String>,
    /// Optional alias for implementation_contract.
    pub contract: Option<String>,
    /// Acceptance criteria for this item. Provide at least one criterion when updating this section.
    pub acceptance: Option<Vec<String>>,
    /// Optional notes section. Empty string removes notes when present.
    pub notes: Option<String>,
    /// Allow updates to items already closed by Git trailer or direct completion.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub force_closed: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateEpicParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Epic identifier used as the backlog/epics/<id>.md filename. Use only
    /// letters, digits, `_`, or `-`; do not include path separators.
    #[schemars(example = example_epic_id(), pattern(r"^[A-Za-z0-9_-]+$"))]
    pub id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Epic lifecycle status. Supported values: active, archived.
    #[schemars(with = "Option<EpicStatusSchema>")]
    pub status: Option<String>,
    /// Backlog priority. Supported values: P0, P1, P2.
    #[schemars(with = "Option<BacklogPrioritySchema>")]
    pub priority: Option<String>,
    /// Primary area or surface for this epic. Defaults to the epic id.
    pub area: Option<String>,
    /// Optional markdown body text written below the epic heading.
    pub description: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CreatedEpicData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Epic returned by this request.
    pub epic: EpicRecord,
    /// Whether this tool call created the file, record, or workspace.
    pub created: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListEpicsData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Epics returned by this request.
    pub epics: Vec<EpicRecord>,
    /// Number of records returned.
    pub returned: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EpicRecord {
    /// Epic or grouping identifier.
    pub id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Backlog priority for this item.
    pub priority: String,
    /// Primary area or surface for this item.
    pub area: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, PartialEq, Eq)]
#[schemars(inline)]
#[serde(deny_unknown_fields)]
pub struct ExternalRef {
    /// External provider name.
    pub provider: String,
    /// Kind or category for this record.
    pub kind: String,
    /// Stable local record identifier.
    pub id: String,
    /// Canonical URL for this record.
    pub url: Option<String>,
    /// Provider-specific locator or reference.
    pub locator: Option<String>,
    /// Timestamp when the external record was imported locally.
    pub imported_at: Option<String>,
    /// Hash of the source payload or content.
    pub source_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[schemars(inline)]
#[serde(deny_unknown_fields)]
pub struct ExternalReportDraft {
    /// Durable report key.
    pub report_key: String,
    /// External provider name.
    pub provider: String,
    /// Kind or category for this record.
    pub kind: String,
    /// External system identifier.
    pub external_id: String,
    /// External URL for the source record.
    pub external_url: Option<String>,
    /// Type of external report to draft or record.
    pub report_type: String,
    /// Source backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub source_item_id: String,
    /// Source task identifier.
    pub source_task_id: Option<String>,
    /// Human-readable title or short label.
    pub title: String,
    /// Body text supplied by or generated for the external record.
    pub body: String,
    /// References for local.
    pub local_refs: Vec<String>,
    /// References for evidence.
    pub evidence_refs: Vec<String>,
    /// Safety notes explaining approval, audit, or redaction concerns.
    pub safety_notes: Vec<String>,
    /// Whether dispatching this draft requires explicit approval.
    pub requires_approval: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskPlanQueryParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub item_id: Option<String>,
    /// Whether validation errors should be included.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub include_errors: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskPlanItemParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub item_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteTaskPlanParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub item_id: String,
    /// Task plan state or file for this item.
    pub plan: TaskPlanFile,
    /// Whether to overwrite an existing file or record.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectTaskEventsParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: String,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectTaskParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClaimNextTaskParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Worker name associated with this item.
    pub worker: Option<String>,
    /// Name recorded as the task claimant.
    pub claimant: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorktreeCreateParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: String,
    /// Git base reference.
    pub base_ref: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorktreeStatusParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorktreeDiffParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectIntegrationGatesParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier whose integration readiness should be inspected.
    #[schemars(example = example_task_id())]
    pub task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorktreeCleanupParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: String,
    /// Whether to force the operation.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub force: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IntegrateWorkerResultParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: String,
    /// Override workflow.integration.merge_style for this integration.
    ///
    /// Supported values: merge_commit, fast_forward, squash, apply_changed_files.
    #[schemars(with = "Option<IntegrationStrategySchema>")]
    pub strategy: Option<String>,
    /// Permit integration without separately recorded verification evidence.
    ///
    /// This is explicit so low-risk work can move without weakening strict project policy.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub allow_unverified: Option<bool>,
    /// Remove the task worktree after successful integration when it is clean.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub cleanup_after: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateTaskBundleParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: String,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Verification command to run or record for this item.
    pub verification_command: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrepareWorkerAssignmentParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: Option<String>,
    /// Worker name associated with this item.
    pub worker: Option<String>,
    /// Name recorded as the task claimant.
    pub claimant: Option<String>,
    /// Execution mode to record on the prepared assignment bundle.
    ///
    /// Supported value is manual_handoff. Omit this for host-managed handoffs.
    #[schemars(with = "Option<ExecutionModeSchema>")]
    pub execution_mode: Option<String>,
    /// Git base reference.
    pub base_ref: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Verification command to run or record for this item.
    pub verification_command: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectWorkerAssignmentParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Worker assignment identifier.
    #[schemars(example = example_assignment_id())]
    pub assignment_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartWorkerExecutionParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Worker assignment identifier.
    #[schemars(example = example_assignment_id())]
    pub assignment_id: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: Option<String>,
    /// Worker session identifier.
    pub worker_session: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordWorkerEventParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Worker assignment identifier.
    #[schemars(example = example_assignment_id())]
    pub assignment_id: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: Option<String>,
    /// Event type name used for filtering and replay.
    #[schemars(example = example_event_type(), pattern(r"^[A-Za-z0-9_.:-]{1,80}$"))]
    pub event_type: String,
    /// Human-readable summary of the record or result.
    #[schemars(example = example_summary())]
    pub summary: String,
    #[serde(default, deserialize_with = "crate::compat::deserialize_map")]
    /// Structured payload for this record.
    pub payload: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteWorkerExecutionParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Worker assignment identifier.
    #[schemars(example = example_assignment_id())]
    pub assignment_id: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: Option<String>,
    /// Lifecycle or result status for this record.
    #[schemars(with = "WorkerTerminalStatusSchema")]
    pub status: String,
    /// Human-readable summary of the record or result.
    #[schemars(example = example_summary())]
    pub summary: String,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Files changed by the worker, relative to the task worktree.
    pub changed_files: Vec<String>,
    /// Verification status for the task or result.
    #[schemars(with = "Option<VerificationStatusSchema>")]
    pub verification_status: Option<String>,
    /// Whether completion may auto-start a prepared assignment before marking
    /// it terminal. Defaults to true so same-session MCP hosts can finish a
    /// prepared task without a separate start_worker_task call.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub auto_start_if_prepared: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(inline)]
pub struct FinishWorkFindingInput {
    /// Human-readable title or short label.
    #[schemars(example = "Worker found missing verification coverage")]
    pub title: String,
    /// Human-readable summary of the finding and why it matters.
    #[schemars(example = example_summary())]
    pub summary: String,
    /// Severity level for this follow-up finding.
    #[schemars(with = "Option<FindingSeveritySchema>")]
    pub severity: Option<String>,
    /// Whether this finding must receive an explicit disposition before the work can be considered done.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub required: Option<bool>,
    /// Owner name or repository owner, depending on context.
    pub owner: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// References for evidence.
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FinishWorkParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Backlog item identifier for direct-work recovery guidance. Direct work
    /// should be completed with complete_backlog_item rather than finish_work.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub item_id: Option<String>,
    /// Worker assignment identifier.
    #[schemars(example = example_assignment_id())]
    pub assignment_id: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: Option<String>,
    /// Lifecycle or result status for this worker result. Omit for completed.
    #[schemars(with = "Option<WorkerTerminalStatusSchema>")]
    pub status: Option<String>,
    /// Human-readable summary of the worker result.
    #[schemars(example = example_summary())]
    pub summary: String,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Files changed by the worker, relative to the task worktree. Omit to let
    /// Platypus infer the list from the recorded worktree diff.
    pub changed_files: Vec<String>,
    /// Verification status for the worker result.
    #[schemars(with = "Option<VerificationStatusSchema>")]
    pub verification_status: Option<String>,
    /// Summary of verification that should be recorded as evidence when the
    /// worker reports verification passed.
    pub verification_summary: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// References such as commands, files, commits, URLs, or evidence IDs.
    pub verification_refs: Vec<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec")]
    /// Findings or follow-up work discovered while implementing this task.
    pub findings: Vec<FinishWorkFindingInput>,
    /// Set true only when the worker explicitly checked for follow-up findings
    /// and found none. This does not accept, resolve, or waive required
    /// findings supplied in the same call; required findings must still be
    /// dispositioned before final integration.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub findings_reviewed: Option<bool>,
    /// Whether completion may auto-start a prepared assignment before marking
    /// it terminal. Defaults to true so same-session MCP hosts can finish a
    /// prepared task without a separate start_worker_task call.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub auto_start_if_prepared: Option<bool>,
    /// Whether to integrate the completed task immediately when verification
    /// and finding gates permit it.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub integrate_if_ready: Option<bool>,
    /// Permit integration without separately recorded verification evidence.
    ///
    /// This is explicit so low-risk work can move without weakening strict project policy.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub allow_unverified: Option<bool>,
    /// Override workflow.integration.merge_style for this integration.
    #[schemars(with = "Option<IntegrationStrategySchema>")]
    pub integration_strategy: Option<String>,
    /// Remove the task worktree after successful integration when it is clean.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub cleanup_after: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteBacklogItemParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub item_id: String,
    /// Human-readable summary of the completed direct work.
    #[schemars(example = example_summary())]
    pub summary: String,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Files changed by the direct host work, relative to the manager workspace.
    pub changed_files: Vec<String>,
    /// Verification status for this direct work.
    #[schemars(with = "Option<VerificationStatusSchema>")]
    pub verification_status: Option<String>,
    /// Summary of verification that should be recorded as evidence.
    pub verification_summary: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// References such as commands, files, commits, URLs, or evidence IDs.
    pub verification_refs: Vec<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Existing evidence identifiers or references that support this completion.
    pub evidence_refs: Vec<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// Existing finding identifiers or references reviewed for this completion.
    pub finding_refs: Vec<String>,
    /// Whether Platypus should create a Git commit for the supplied changed_files
    /// with Platypus-Closes and Platypus-Verification trailers.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub commit: Option<bool>,
    /// Optional subject for the closure commit when commit=true.
    pub commit_message: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunTaskVerificationParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Worker assignment identifier.
    #[schemars(example = example_assignment_id())]
    pub assignment_id: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: Option<String>,
    /// Maximum execution time in seconds for the verification command.
    #[schemars(range(min = 1, max = 600))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_u64")]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ApprovalListParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Lifecycle or result status for this record.
    #[schemars(with = "Option<ApprovalStatusSchema>")]
    pub status: Option<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ApprovalRespondParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Approval identifier.
    pub approval_id: String,
    /// Approval decision. Supported values are approve and deny.
    #[schemars(with = "ApprovalDecisionSchema")]
    pub decision: String,
    /// Name of the person or agent responding.
    pub responder: Option<String>,
    /// Human-readable reason for the decision or result.
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EventsReplayParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: Option<String>,
    /// Scope for this record or operation.
    pub scope: Option<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StorageCapabilityProbeParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordEvidenceParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Stable local record identifier.
    pub id: Option<String>,
    /// Source backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub source_item_id: Option<String>,
    /// Source task identifier.
    #[schemars(example = example_task_id())]
    pub source_task_id: Option<String>,
    /// Kind or category for this record. Supported values: commit,
    /// verification, file_summary, worker_finding, manager_disposition,
    /// external_report, note. Use `note` for generic evidence when unsure.
    #[schemars(
        description = "Kind or category for this record. Supported values: commit, verification, file_summary, worker_finding, manager_disposition, external_report, note. Use note for generic evidence when unsure."
    )]
    #[schemars(with = "EvidenceKindSchema")]
    pub kind: String,
    /// Human-readable summary of the record or result.
    #[schemars(example = example_summary())]
    pub summary: String,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// References such as commands, files, commits, URLs, or evidence IDs.
    pub refs: Vec<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_map")]
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordVerificationEvidenceParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Stable local record identifier.
    pub id: Option<String>,
    /// Source backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub source_item_id: Option<String>,
    /// Source task identifier.
    #[schemars(example = example_task_id())]
    pub source_task_id: Option<String>,
    /// Human-readable summary of the record or result.
    #[schemars(example = example_summary())]
    pub summary: String,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// References such as commands, files, commits, URLs, or evidence IDs.
    pub refs: Vec<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_map")]
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListEvidenceParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Source backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub source_item_id: Option<String>,
    /// Source task identifier.
    #[schemars(example = example_task_id())]
    pub source_task_id: Option<String>,
    /// Kind or category for this record.
    #[schemars(with = "Option<EvidenceKindSchema>")]
    pub kind: Option<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReconcileParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorkflowConfigParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkflowIntegrationConfig {
    /// Configured merge style for integration.
    pub merge_style: String,
    /// Whether a clean manager workspace is required.
    pub require_clean_manager_workspace: bool,
    /// Whether verification evidence is required.
    pub require_verification_evidence: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkflowDispatchConfig {
    /// Whether dispatch auto-commits backlog planning artifacts by default.
    pub auto_commit_artifacts_default: bool,
}

#[derive(Debug, Serialize, Clone, JsonSchema)]
pub struct WorkflowExecutionConfig {
    /// Default execution path for backlog items that do not specify one.
    #[schemars(with = "BacklogExecutionPathSchema")]
    pub default_path: String,
    /// Planning gate used by direct_edit items when they do not specify one.
    #[schemars(with = "PlanningGateSchema")]
    pub direct_planning_gate: String,
    /// Planning gate used by worker_handoff items when they do not specify one.
    #[schemars(with = "PlanningGateSchema")]
    pub worker_planning_gate: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkflowConfigData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Effective workflow integration policy.
    pub integration: WorkflowIntegrationConfig,
    /// Effective workflow dispatch policy.
    pub dispatch: WorkflowDispatchConfig,
    /// Effective workflow execution policy.
    pub execution: WorkflowExecutionConfig,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CommitPlanningArtifactsParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Restrict allowed planning artifacts to these backlog item IDs. When
    /// omitted, all backlog/items, backlog/plans, and backlog/epics artifacts
    /// may be committed.
    #[schemars(example = example_item_ids())]
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    pub item_ids: Vec<String>,
    /// Also allow project-level scaffold/config artifacts such as platy.yaml,
    /// AGENTS.md, CLAUDE.md, WORKFLOW.md, backlog/README.md, templates, and
    /// .gitignore. Defaults to false.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub include_project_config: Option<bool>,
    /// Commit message. Defaults to a scoped Platypus planning message.
    pub message: Option<String>,
    /// Preview accepted and rejected paths without staging or committing.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub dry_run: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CommitPlanningArtifactsData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Paths accepted as Platypus planning artifacts.
    pub accepted_paths: Vec<String>,
    /// Paths rejected because they are outside the allowed planning artifact set.
    pub rejected_paths: Vec<String>,
    /// Git commit hash created by this operation.
    pub commit: Option<String>,
    /// Whether this tool call changed repository state.
    pub committed: bool,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendWorkerGuidanceParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: String,
    /// Message text for this request or result.
    #[schemars(example = "Focus on the smallest tested slice and record any follow-up findings.")]
    pub message: String,
    /// Person or agent that authored this guidance or message.
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordFindingParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Stable local record identifier.
    pub id: Option<String>,
    /// Source backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub source_item_id: Option<String>,
    /// Source task identifier.
    #[schemars(example = example_task_id())]
    pub source_task_id: Option<String>,
    /// Reference to the source finding.
    pub source_finding_ref: Option<String>,
    /// Human-readable title or short label.
    #[schemars(example = "Worker found missing verification coverage")]
    pub title: String,
    /// Human-readable summary of the record or result.
    #[schemars(example = example_summary())]
    pub summary: String,
    /// Severity level.
    #[schemars(with = "Option<FindingSeveritySchema>")]
    pub severity: Option<String>,
    /// Whether this finding or item must receive an explicit disposition before completion.
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_bool")]
    pub required: Option<bool>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// References for evidence.
    pub evidence_refs: Vec<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_map")]
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListFindingsParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Source backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub source_item_id: Option<String>,
    /// Source task identifier.
    #[schemars(example = example_task_id())]
    pub source_task_id: Option<String>,
    /// Lifecycle or result status for this record.
    #[schemars(with = "Option<FindingDispositionStatusSchema>")]
    pub status: Option<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 100))]
    #[serde(default, deserialize_with = "crate::compat::deserialize_option_usize")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ValidateFindingsParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Source backlog item identifier.
    pub source_item_id: Option<String>,
    /// Source task identifier.
    #[schemars(example = example_task_id())]
    pub source_task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateFindingDispositionParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Finding identifier.
    pub finding_id: String,
    /// Lifecycle or result status for this record.
    #[schemars(with = "FindingDispositionStatusSchema")]
    pub status: String,
    /// Owner name or repository owner, depending on context.
    pub owner: Option<String>,
    /// Reason recorded for the finding disposition decision.
    pub disposition_reason: Option<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_vec_string")]
    /// References for evidence.
    pub evidence_refs: Vec<String>,
    #[serde(default, deserialize_with = "crate::compat::deserialize_map")]
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Completed,
    Skipped,
    Failed,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ActionResult<T: Serialize + JsonSchema> {
    /// Stable action name that produced this result.
    pub action: String,
    /// Lifecycle or result status for this record.
    pub status: ActionStatus,
    /// Human-readable summary of the record or result.
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Suggested normal continuation after a completed or skipped result.
    pub next_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Recovery action for failed or blocked results.
    pub recovery_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Structured success payload for this action.
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Error message returned when the action failed.
    pub error: Option<String>,
}

impl<T: Serialize + JsonSchema> ActionResult<T> {
    pub fn completed(action: &str, summary: impl Into<String>, data: T) -> Self {
        Self {
            action: action.to_string(),
            status: ActionStatus::Completed,
            summary: summary.into(),
            next_action: None,
            recovery_action: None,
            data: Some(data),
            error: None,
        }
    }

    pub fn skipped(action: &str, summary: impl Into<String>, next_action: &str) -> Self {
        Self {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: summary.into(),
            next_action: Some(next_action.to_string()),
            recovery_action: None,
            data: None,
            error: None,
        }
    }

    pub fn failed(action: &str, summary: impl Into<String>, error: impl Into<String>) -> Self {
        let recovery_action = "Inspect the request and project setup.".to_string();
        Self {
            action: action.to_string(),
            status: ActionStatus::Failed,
            summary: summary.into(),
            next_action: Some(recovery_action.clone()),
            recovery_action: Some(recovery_action),
            data: None,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Serialize, Clone, JsonSchema)]
pub struct HostAction {
    /// Host action category that describes what the MCP client or worker should
    /// do next.
    #[schemars(with = "HostActionKindSchema")]
    pub kind: String,
    /// Human-readable summary of the recommended host action.
    pub summary: String,
    /// Step-by-step instructions for the MCP host, worker, or human operator.
    pub instructions: Vec<String>,
    /// Task identifier when the action relates to a task.
    #[schemars(example = example_task_id())]
    pub task_id: Option<String>,
    /// Worker assignment identifier when the action relates to an assignment.
    #[schemars(example = example_assignment_id())]
    pub assignment_id: Option<String>,
    /// Worker name associated with this action.
    pub worker: Option<String>,
    /// Filesystem path to the task worktree or target workspace.
    pub worktree_path: Option<String>,
    /// Generated task bundle for this action, when a worker handoff exists.
    pub bundle: Option<TaskBundle>,
    /// Recommended Platypus MCP tools to call after this action.
    pub next_tools: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PrepareWorkData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Summary of the queue or dispatch state inspected by this operation.
    pub queue_summary: String,
    /// Preparation state reached by this call.
    #[schemars(with = "PreparedStateSchema")]
    pub prepared_state: String,
    /// Backlog item identifiers selected by this preparation call.
    pub selected_item_ids: Vec<String>,
    /// Number of queue items ready to dispatch before preparation.
    pub ready_count: usize,
    /// Number of queue items blocked before dispatch.
    pub blocked_count: usize,
    /// High-level actions for the MCP host or human operator.
    pub host_actions: Vec<HostAction>,
    /// Dispatch result when a worktree handoff was prepared.
    pub dispatch: Option<DispatchReadyWorkData>,
    /// Queue inspection result when the caller requested a full queue snapshot.
    pub queue: Option<WorkQueueData>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FinishWorkData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Worker assignment returned or updated by this operation.
    pub assignment: Option<WorkerAssignment>,
    /// Bounded worktree diff inspected while finishing.
    pub worktree_changes: Option<WorktreeDiffData>,
    /// Evidence records returned or created by this operation.
    pub evidence: Vec<EvidenceRecord>,
    /// Finding records returned or created by this operation.
    pub findings: Vec<FindingRecord>,
    /// Integration result when finish_work integrated the task.
    pub integration: Option<WorkerResultIntegrationData>,
    /// Reconciliation result after integration.
    pub reconciliation: Option<ReconciliationData>,
    /// High-level action for the MCP host or human operator.
    pub host_action: HostAction,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CompleteBacklogItemData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Backlog item identifier.
    pub item_id: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    /// Files changed by the direct host work, relative to the manager workspace.
    pub changed_files: Vec<String>,
    /// Evidence records returned or created by this operation.
    pub evidence: Vec<EvidenceRecord>,
    /// Project event recorded for the direct completion.
    pub event: Option<EventRecord>,
    /// Git commit hash when complete_backlog_item created a closure commit.
    pub commit: Option<String>,
    /// Whether this item is now considered closed by runtime completion state
    /// or Git trailer policy.
    pub closed: bool,
    /// High-level action for the MCP host or human operator.
    pub host_action: HostAction,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PingData {
    /// Echoed message value.
    pub echo: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectStatusData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Whether platy.yaml exists in the project root.
    pub platy_yaml: bool,
    /// Whether the backlog directory exists in the project root.
    pub backlog_dir: bool,
    /// Whether Git metadata exists for this project.
    pub git_metadata: bool,
    /// Number of local backlog items discovered.
    pub backlog_items: usize,
    /// Number of backlog items currently runnable.
    pub runnable_backlog_items: usize,
    /// Whether durable task state is supported by the configured backend.
    pub tasks_supported: bool,
    /// Whether durable finding state is supported by the configured backend.
    pub findings_supported: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkQueueData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Whether dispatch readiness should require task-plan artifacts for non-direct work.
    pub require_task_plan: bool,
    /// Number of queue items ready to dispatch.
    pub ready_count: usize,
    /// Number of queue items blocked before dispatch.
    pub blocked_count: usize,
    /// Number of returned backlog items that already have active task lifecycle
    /// state or completed task results awaiting integration.
    pub active_count: usize,
    /// Returned backlog item identifiers that already have active task
    /// lifecycle state or completed task results awaiting integration.
    pub active_item_ids: Vec<String>,
    /// Active or pending task identifiers that block dispatch for returned
    /// backlog items.
    pub active_task_ids: Vec<String>,
    /// Whole-backlog inventory summary for queue context, including items that
    /// are not returned as runnable queue candidates.
    pub inventory: WorkQueueInventorySummary,
    /// Warnings that should be resolved before dispatching work.
    pub preflight_warnings: Vec<String>,
    /// Compatibility warnings for deprecated queue inputs or policy overrides.
    pub policy_warnings: Vec<String>,
    /// Recommended Platypus MCP tool to call next.
    pub recommended_tool: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    /// Human-readable reason for the decision or result.
    pub reason: String,
    /// Suggested parameters for the recommended tool call.
    pub params: BTreeMap<String, Value>,
    /// Items or records in this response.
    pub items: Vec<WorkQueueItem>,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct WorkQueueItem {
    /// One-based position in the returned queue.
    pub position: usize,
    /// Backlog candidate chosen or inspected by this operation.
    pub candidate: BacklogCandidate,
    /// Planning classification for this backlog candidate.
    pub planning: PlanningClassification,
    /// Task plan state or file for this item.
    pub plan: WorkQueuePlanState,
    /// Planning approval state when queue inspection requires planning approval.
    pub planning_approval: Option<PlanningApprovalState>,
    /// Durable execution policy resolved from backlog item frontmatter and
    /// workflow.execution defaults.
    pub effective_policy: EffectiveExecutionPolicy,
    /// Queue state for this item. Values include ready, direct_ready,
    /// planning_blocked, approval_blocked, dependency_blocked, config_blocked,
    /// workspace_blocked, active, and completed_pending_integration.
    #[schemars(with = "WorkQueueStateSchema")]
    pub queue_state: String,
    /// Execution path implied by the current queue state.
    #[schemars(with = "WorkExecutionPathSchema")]
    pub execution_path: String,
    /// Tool that closes this item once implementation work is done, when known.
    pub completion_tool: Option<String>,
    /// Whether a task plan is required before this item can use a worktree
    /// worker handoff path.
    pub task_plan_required_for_worktree: bool,
    /// Human-readable explanation of why direct or planned work applies.
    pub execution_guidance: String,
    /// Active or pending lifecycle task id for this backlog item, when one
    /// currently blocks new dispatch.
    pub task_id: Option<String>,
    /// Active worker assignment id for this backlog item, when one exists.
    pub assignment_id: Option<String>,
    /// Whether this item can be dispatched now.
    pub ready_to_dispatch: bool,
    /// Recommended Platypus MCP tool to call next.
    pub recommended_tool: String,
    /// Human-readable reason for the decision or result.
    pub reason: String,
}

#[derive(Debug, Serialize, Clone, JsonSchema)]
pub struct EffectiveExecutionPolicy {
    /// Effective execution path for this item.
    #[schemars(with = "BacklogExecutionPathSchema")]
    pub execution_path: String,
    /// Effective planning gate for this item.
    #[schemars(with = "PlanningGateSchema")]
    pub planning_gate: String,
    /// Source of this effective policy.
    pub source: String,
    /// Human-readable reason for the policy resolution.
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct PlanningApprovalState {
    /// Backlog item identifier.
    pub item_id: String,
    /// Whether this backlog item currently requires planning approval.
    pub required: bool,
    /// Whether an approved planning approval covers this backlog item.
    pub approved: bool,
    /// Approval identifier that currently applies, if any.
    pub approval_id: Option<String>,
    /// Approval lifecycle status that currently applies.
    pub status: Option<String>,
    /// Human-readable reason for the planning approval decision.
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct WorkQueuePlanState {
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Filesystem path for the local file or workspace.
    pub path: Option<String>,
    /// Workflow mode or execution mode.
    pub mode: Option<String>,
    /// Number of tasks represented by this plan or result.
    pub task_count: usize,
    /// Number of requirements represented by this plan.
    pub requirement_count: usize,
    /// Validation or processing errors.
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize, Clone, JsonSchema)]
pub struct PlanningClassification {
    /// Backlog item identifier.
    pub item_id: String,
    /// Deterministic planning requirement for this queue view.
    pub required_mode: String,
    /// Required planning artifact path, if the caller requested one.
    pub required_artifact: Option<String>,
    /// Human-readable reasons derived from explicit caller policy or state.
    pub reasons: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkQueueInventorySummary {
    /// Total number of backlog items before queue filtering.
    pub total_count: usize,
    /// Backlog items that are open and have all dependencies closed.
    pub runnable_count: usize,
    /// Backlog items blocked by open dependencies.
    pub dependency_blocked_count: usize,
    /// Backlog items closed by Git trailers or recorded direct completion.
    pub closed_count: usize,
    /// Backlog items with active task lifecycle state.
    pub active_lifecycle_count: usize,
    /// Backlog items with completed task results waiting for integration.
    pub pending_integration_count: usize,
    /// Dependency-blocked backlog items returned for visibility.
    pub dependency_blocked_items: Vec<BacklogInventoryItem>,
    /// Closed backlog items returned for visibility.
    pub closed_items: Vec<BacklogInventoryItem>,
    /// Item IDs with active task lifecycle state.
    pub active_lifecycle_item_ids: Vec<String>,
    /// Task IDs with completed results awaiting integration.
    pub pending_integration_task_ids: Vec<String>,
    /// Whether any inventory lists were truncated by the queue limit.
    pub truncated: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BacklogItemMarkdownState {
    /// Filesystem path for the backlog item markdown file.
    pub path: String,
    /// Markdown section headings discovered in the backlog item body.
    pub sections: Vec<String>,
    /// External references tied to this record.
    pub external_refs: Vec<ExternalRef>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct InspectItemData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Backlog inventory state for this item.
    pub item: BacklogInventoryItem,
    /// Markdown file state and section headings for this item.
    pub markdown: BacklogItemMarkdownState,
    /// Queue view for this item when it is currently runnable or in an active
    /// task lifecycle.
    pub queue: Option<WorkQueueItem>,
    /// Task plan state or file for this item.
    pub plan: WorkQueuePlanState,
    /// Planning classification for this backlog item.
    pub planning: PlanningClassification,
    /// Planning approval state when requested by this inspection.
    pub planning_approval: Option<PlanningApprovalState>,
    /// Current queue or lifecycle state for this item.
    pub queue_state: String,
    /// Active or pending lifecycle task id for this backlog item, when one
    /// currently exists.
    pub task_id: Option<String>,
    /// Active worker assignment id for this backlog item, when one exists.
    pub assignment_id: Option<String>,
    /// Whether this item can be dispatched or prepared now.
    pub ready_to_dispatch: bool,
    /// Recommended Platypus MCP tool to call next for this item.
    pub recommended_tool: String,
    /// Human-readable reason for the recommendation.
    pub reason: String,
    /// Suggested parameters for the recommended tool call.
    pub params: BTreeMap<String, Value>,
    /// Finding records attached to this backlog item.
    pub findings: Vec<FindingRecord>,
    /// Evidence records attached to this backlog item.
    pub evidence: Vec<EvidenceRecord>,
    /// Number of finding records returned.
    pub finding_count: usize,
    /// Number of evidence records returned.
    pub evidence_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DoctorSnapshotData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Whether the operation succeeded.
    pub ok: bool,
    /// Diagnostic checks included in this result.
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct InspectSessionData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Whether inspected setup and queue state are ready for normal workflow
    /// continuation.
    pub ok: bool,
    /// Project setup diagnostics, when inspection could collect them.
    pub doctor: Option<DoctorSnapshotData>,
    /// Project and backlog status, when inspection could collect it.
    pub status: Option<ProjectStatusData>,
    /// Effective workflow configuration, when inspection could collect it.
    pub workflow: Option<WorkflowConfigData>,
    /// Current work queue snapshot, when inspection could collect it.
    pub queue: Option<WorkQueueData>,
    /// Non-fatal errors encountered while collecting the session snapshot.
    pub errors: Vec<String>,
    /// Recommended Platypus MCP tool to call next.
    pub recommended_tool: String,
    /// Human-readable summary of the session snapshot.
    pub summary: String,
    /// Human-readable reason for the recommendation.
    pub reason: String,
    /// Suggested parameters for the recommended tool call.
    pub params: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DoctorCheck {
    /// Name of this check, profile, or record.
    pub name: String,
    /// Lifecycle or result status for this record.
    pub status: DoctorCheckStatus,
    /// Human-readable summary of the record or result.
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Suggested next action.
    pub next_action: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DoctorCheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectScaffoldData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Whether this tool call created the file, record, or workspace.
    pub created: usize,
    /// Number of records skipped by this operation.
    pub skipped: usize,
    /// Scaffold entries inspected or written by this operation.
    pub entries: Vec<ScaffoldEntry>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ScaffoldEntry {
    /// Filesystem path for the local file or workspace.
    pub path: String,
    /// Kind or category for this record.
    pub kind: String,
    /// Lifecycle or result status for this record.
    pub status: ScaffoldEntryStatus,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScaffoldEntryStatus {
    Created,
    Skipped,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct BacklogCandidate {
    /// Source system or origin for this record.
    pub source: String,
    /// Backlog item identifier.
    pub item_id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Backlog priority for this item.
    pub priority: String,
    #[serde(rename = "type")]
    /// Backlog item type.
    pub item_type: String,
    /// Primary area or surface for this item.
    pub area: String,
    /// Owned code or documentation surfaces for this item.
    pub owned_surfaces: Vec<String>,
    /// External references tied to this record.
    pub external_refs: Vec<ExternalRef>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BacklogListData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Runnable backlog candidates returned by this request.
    pub candidates: Vec<BacklogCandidate>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BacklogInventoryData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Items or records in this response.
    pub items: Vec<BacklogInventoryItem>,
    /// Total number of records in the current scope.
    pub total: usize,
    /// Number of records returned.
    pub returned: usize,
    /// Whether the returned output was truncated.
    pub truncated: bool,
    /// Whether this backlog item is runnable now.
    pub runnable: usize,
    /// Whether this backlog item is closed.
    pub closed: usize,
    /// Number of blocked items.
    pub blocked: usize,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct BacklogInventoryItem {
    /// Backlog item identifier.
    pub item_id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Backlog priority for this item.
    pub priority: String,
    #[serde(rename = "type")]
    /// Backlog item type.
    pub item_type: String,
    /// Primary area or surface for this item.
    pub area: String,
    /// Owned code or documentation surfaces for this item.
    pub owned_surfaces: Vec<String>,
    /// Dependencies that must be satisfied first.
    pub depends_on: Vec<String>,
    /// Dependencies that remain open for this item.
    pub open_dependencies: Vec<String>,
    /// Whether this backlog item is closed.
    pub closed: bool,
    /// Whether this backlog item is runnable now.
    pub runnable: bool,
    /// Human-readable reason for the decision or result.
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BacklogDependencyGraphData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Focus item used to scope the graph, when requested.
    pub focus_item_id: Option<String>,
    /// Whether closed backlog items are included as graph nodes.
    pub include_closed: bool,
    /// Maximum number of graph nodes requested after focus filtering.
    pub limit: usize,
    /// Total number of graph nodes in scope before applying the limit.
    pub total: usize,
    /// Number of graph nodes returned.
    pub returned: usize,
    /// Whether the graph output was truncated by the limit.
    pub truncated: bool,
    /// Backlog item graph nodes returned in deterministic item ID order.
    pub nodes: Vec<BacklogDependencyNode>,
    /// Directed graph edges from dependency item to dependent item.
    pub edges: Vec<BacklogDependencyEdge>,
    /// Node IDs with no dependency edges inside the returned graph.
    pub roots: Vec<String>,
    /// Node IDs with no dependent edges inside the returned graph.
    pub leaves: Vec<String>,
    /// Dependency-first order for acyclic nodes in the returned graph.
    ///
    /// If cycles are present this order is partial and cycle details are
    /// reported in `cycles`.
    pub topological_order: Vec<String>,
    /// Open node IDs whose dependencies are all closed or absent.
    pub runnable_nodes: Vec<String>,
    /// Node IDs closed by reachable Platypus-Closes Git trailers.
    pub closed_nodes: Vec<String>,
    /// Open node IDs blocked by open or missing dependencies.
    pub blocked_nodes: Vec<String>,
    /// Dependency references that point to no backlog item.
    pub missing_dependencies: Vec<BacklogMissingDependency>,
    /// Detected dependency cycles. Each cycle repeats the first node at the end.
    pub cycles: Vec<Vec<String>>,
    /// Validation issues found while building the graph.
    pub validation_errors: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BacklogDependencyNode {
    /// Backlog item identifier.
    pub item_id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Backlog priority for this item.
    pub priority: String,
    #[serde(rename = "type")]
    /// Backlog item type.
    pub item_type: String,
    /// Primary area or surface for this item.
    pub area: String,
    /// Dependencies declared by this backlog item.
    pub depends_on: Vec<String>,
    /// Backlog item IDs that depend on this item.
    pub dependents: Vec<String>,
    /// Dependencies that currently prevent this item from being runnable.
    pub blocked_by: Vec<String>,
    /// Declared dependencies that do not resolve to backlog items.
    pub missing_dependencies: Vec<String>,
    /// Whether this backlog item is closed.
    pub closed: bool,
    /// Whether this backlog item is runnable now.
    pub runnable: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BacklogDependencyEdge {
    /// Backlog item ID that must be completed first.
    pub dependency: String,
    /// Backlog item ID that is blocked by the dependency.
    pub dependent: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BacklogMissingDependency {
    /// Backlog item that declares the missing dependency.
    pub item_id: String,
    /// Dependency item ID that was referenced but not found.
    pub missing_dependency: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BacklogValidationData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Whether the operation succeeded.
    pub ok: bool,
    /// Number of backlog item files inspected.
    pub item_count: usize,
    /// Number of backlog epic files inspected.
    pub epic_count: usize,
    /// Validation or processing errors.
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExternalBacklogDraftData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// External provider name.
    pub provider: String,
    /// Draft backlog items produced by this operation.
    pub drafts: Vec<ExternalBacklogDraft>,
    /// Number of records returned.
    pub returned: usize,
    /// Number of records skipped by this operation.
    pub skipped: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExternalBacklogDraft {
    /// Candidate identifier.
    pub candidate_id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Objective for this candidate or record.
    pub objective: String,
    /// Backlog priority for this item.
    pub priority: String,
    #[serde(rename = "type")]
    /// Backlog item type.
    pub item_type: String,
    /// Primary area or surface for this item.
    pub area: String,
    /// Owned code or documentation surfaces for this item.
    pub owned_surfaces: Vec<String>,
    /// Verification command to run or record for this item.
    pub verification_command: Vec<String>,
    /// External reference mapped into the local backlog candidate.
    pub external_ref: ExternalRef,
    /// Labels or tags associated with this record.
    pub labels: Vec<String>,
    /// Reasons explaining how the external record was mapped.
    pub mapping_reasons: Vec<String>,
    /// Number of records skipped by this operation.
    pub skipped: bool,
    /// Reason the record was skipped.
    pub skip_reason: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GitHubIssueImportData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Owner name or repository owner, depending on context.
    pub owner: String,
    /// Repository name in the external provider.
    pub repo: String,
    /// External records imported into local backlog snapshots.
    pub imported: Vec<GitHubIssueImportRecord>,
    /// Number of records skipped by this operation.
    pub skipped: Vec<GitHubIssueSkipRecord>,
    /// Number of external records imported.
    pub imported_count: usize,
    /// Number of external records skipped.
    pub skipped_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExternalReportDraftData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Draft payload prepared for approval or dispatch.
    pub draft: ExternalReportDraft,
    /// Evidence records returned or created by this operation.
    pub evidence: Vec<EvidenceRecord>,
    /// Task record returned or updated by this operation.
    pub task: Option<TaskRecord>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExternalReportApprovalData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Approval record returned or updated by this operation.
    pub approval: ApprovalRecord,
    /// Durable report key.
    pub report_key: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PlanningApprovalData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Approval record returned or updated by this operation.
    pub approval: ApprovalRecord,
    /// Planning approval state for each covered backlog item.
    pub states: Vec<PlanningApprovalState>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExternalReportDispatchData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Approval record returned or updated by this operation.
    pub approval: ApprovalRecord,
    /// Durable report key.
    pub report_key: String,
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Provider reference or URL produced by a successful external dispatch.
    pub outbound_ref: Option<String>,
    /// Related event record.
    pub event: EventRecord,
    /// Evidence records returned or created by this operation.
    pub evidence: EvidenceRecord,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GitHubIssueImportRecord {
    /// Backlog item identifier.
    pub item_id: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
    /// External issue reference or number.
    pub issue: String,
    /// Canonical URL for this record.
    pub url: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GitHubIssueSkipRecord {
    /// External issue reference or number.
    pub issue: String,
    /// Human-readable reason for the decision or result.
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CreatedBacklogItemData {
    /// Backlog item identifier.
    pub item_id: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
    /// Whether this tool call created the file, record, or workspace.
    pub created: bool,
    /// Validation result after the planning write completed.
    pub validation: BacklogValidationData,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CreatedBacklogItemsData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Backlog items created by this batch.
    pub items: Vec<CreatedBacklogBatchItem>,
    /// Number of records created.
    pub created: usize,
    /// Validation result after the planning write completed.
    pub validation: BacklogValidationData,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CreatedBacklogBatchItem {
    /// Caller-provided key for this item inside the batch.
    pub client_key: Option<String>,
    /// Backlog item identifier.
    pub item_id: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
    /// Dependencies resolved to backlog item IDs.
    pub depends_on: Vec<String>,
    /// Whether this tool call created the file, record, or workspace.
    pub created: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct UpdatedBacklogItemData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Backlog item identifier.
    pub item_id: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
    /// Whether this item was already closed when the update was requested.
    pub closed: bool,
    /// Fields changed by this update.
    pub changed_fields: Vec<String>,
    /// Validation result after the planning write completed.
    pub validation: BacklogValidationData,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskPlanListData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Task plans returned by this request.
    pub plans: Vec<TaskPlanSummary>,
    /// Number of records returned.
    pub returned: usize,
}

#[derive(Debug, Serialize, Clone, JsonSchema)]
pub struct TaskPlanSummary {
    /// Backlog item identifier.
    pub item_id: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
    /// Workflow mode or execution mode.
    pub mode: Option<String>,
    /// Number of tasks represented by this plan or result.
    pub task_count: usize,
    /// Number of requirements represented by this plan.
    pub requirement_count: usize,
    /// Whether the plan or record validates successfully.
    pub valid: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskPlanData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Backlog item identifier.
    pub item_id: String,
    /// Filesystem path for the local file or workspace.
    pub path: Option<String>,
    /// Task plan state or file for this item.
    pub plan: TaskPlanFile,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskPlanWriteData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Backlog item identifier.
    pub item_id: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
    /// Whether this tool call created the file, record, or workspace.
    pub created: bool,
    /// Whether an existing file was overwritten.
    pub overwritten: bool,
    /// Validation result after the planning write completed.
    pub validation: TaskPlanValidationData,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskPlanValidationData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Whether the operation succeeded.
    pub ok: bool,
    /// Number of task plan files inspected.
    pub plan_count: usize,
    /// Number of tasks represented by this plan or result.
    pub task_count: usize,
    /// Validation or processing errors.
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[schemars(inline)]
#[serde(deny_unknown_fields)]
pub struct TaskPlanFile {
    /// Backlog item identifier.
    pub item_id: String,
    #[serde(default = "default_task_plan_version")]
    /// Task plan schema version.
    pub version: u32,
    /// Workflow mode or execution mode.
    #[schemars(with = "TaskPlanModeSchema")]
    pub mode: String,
    #[serde(default)]
    /// Requirements captured by the task plan.
    pub requirements: Vec<TaskPlanRequirement>,
    /// Design section of the task plan.
    pub design: TaskPlanDesign,
    #[serde(default)]
    /// Planned implementation tasks.
    pub tasks: Vec<PlannedTask>,
}

fn default_task_plan_version() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[schemars(inline)]
#[serde(deny_unknown_fields)]
pub struct TaskPlanRequirement {
    /// Stable local record identifier.
    pub id: String,
    /// Requirement text.
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[schemars(inline)]
#[serde(deny_unknown_fields)]
pub struct TaskPlanDesign {
    /// Human-readable summary of the record or result.
    pub summary: String,
    #[serde(default)]
    /// Owned code or documentation surfaces for this item.
    pub owned_surfaces: Vec<String>,
    /// Optional notes for this item.
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[schemars(inline)]
#[serde(deny_unknown_fields)]
pub struct PlannedTask {
    /// Stable local record identifier.
    pub id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Goal text that drives this request or record.
    pub goal: String,
    #[serde(default)]
    /// References for requirement.
    pub requirement_refs: Vec<String>,
    #[serde(default)]
    /// Dependencies that must be satisfied first.
    pub depends_on: Vec<String>,
    #[serde(default)]
    /// Owned code or documentation surfaces for this item.
    pub owned_surfaces: Vec<String>,
    #[serde(default)]
    /// Verification commands or evidence expected for this planned task.
    pub verification: Vec<String>,
    #[serde(default)]
    /// Acceptance criteria for this item.
    pub acceptance: Vec<String>,
    /// Optional notes for this item.
    pub notes: Option<Vec<String>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingRecordData {
    /// Finding record returned or updated by this operation.
    pub finding: FindingRecord,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingListData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Finding records returned by this operation.
    pub findings: Vec<FindingRecord>,
    /// Number of records returned.
    pub returned: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingValidationData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Whether the operation succeeded.
    pub ok: bool,
    /// Number of required findings still open without an accepted disposition.
    pub unresolved_required_count: usize,
    /// Required findings that still need disposition.
    pub unresolved_required: Vec<FindingRecord>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingDispositionData {
    /// Finding record returned or updated by this operation.
    pub finding: FindingRecord,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct FindingRecord {
    /// Stable local record identifier.
    pub id: String,
    /// Source backlog item identifier.
    pub source_item_id: Option<String>,
    /// Source task identifier.
    pub source_task_id: Option<String>,
    /// Reference to the source finding.
    pub source_finding_ref: Option<String>,
    /// Human-readable title or short label.
    pub title: String,
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Severity level.
    pub severity: Option<String>,
    /// Whether this finding or item must receive an explicit disposition before completion.
    pub required: bool,
    /// Human-readable summary of the record or result.
    pub summary: String,
    /// Owner name or repository owner, depending on context.
    pub owner: Option<String>,
    /// Reason recorded for the finding disposition decision.
    pub disposition_reason: Option<String>,
    /// References for evidence.
    pub evidence_refs: Vec<String>,
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
    /// Timestamp when this record was created.
    pub created_at: String,
    /// Timestamp when this record was last updated.
    pub updated_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskEventListData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Task identifier.
    pub task_id: String,
    /// Event records in this response.
    pub events: Vec<TaskEventRecord>,
    /// Number of records returned.
    pub returned: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkerGuidanceData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Task identifier.
    pub task_id: String,
    /// Person or agent that authored this guidance or message.
    pub author: String,
    /// Message text for this request or result.
    pub message: String,
    /// Related event record.
    pub event: TaskEventRecord,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct TaskEventRecord {
    /// Task identifier.
    pub task_id: String,
    /// Monotonic sequence number within the task event stream.
    pub sequence: i64,
    /// Event type name used for filtering and replay.
    pub event_type: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Structured payload for this record.
    pub payload: Option<Value>,
    /// Timestamp when this record was created.
    pub created_at: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub replay_order: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DispatchNextWorkData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Backlog candidate chosen or inspected by this operation.
    pub candidate: BacklogCandidate,
    /// Task record returned or updated by this operation.
    pub task: TaskRecord,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DispatchReadyWorkData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Number of tasks requested for processing.
    pub requested: usize,
    /// Number of runnable items available before dispatch.
    pub available_before_dispatch: usize,
    /// Number of backlog items selected for dispatch.
    pub selected: usize,
    /// Number of tasks queued by dispatch.
    pub dispatched: usize,
    /// Number of worker handoffs prepared.
    pub prepared: usize,
    /// Number of prepared handoffs marked running via auto_start.
    pub started: usize,
    /// Number of items that failed during this operation.
    pub failed: usize,
    /// Reason the batch stopped.
    pub stopped_reason: String,
    /// Execution mode applied or requested for this dispatch.
    #[schemars(with = "ExecutionModeSchema")]
    pub execution_mode: String,
    /// Warnings that should be resolved before dispatching work.
    pub preflight_warnings: Vec<String>,
    /// Items or records in this response.
    pub items: Vec<DispatchReadyWorkItem>,
}

#[derive(Debug, Serialize, Clone, JsonSchema)]
pub struct DispatchReadyWorkItem {
    /// Backlog item identifier.
    pub item_id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Human-readable reason for the decision or result.
    pub reason: String,
    /// Task record returned or updated by this operation.
    pub task: Option<TaskRecord>,
    /// Worker assignment returned or updated by this operation.
    pub assignment: Option<WorkerAssignment>,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct TaskRecord {
    /// Stable local record identifier.
    pub id: String,
    /// Source backlog item identifier.
    pub source_item_id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Worker name associated with this item.
    pub worker: Option<String>,
    /// Name that claimed the task.
    pub claimed_by: Option<String>,
    /// Timestamp when the task was claimed.
    pub claimed_at: Option<String>,
    /// Timestamp when the task or assignment started.
    pub started_at: Option<String>,
    /// Timestamp when the task or assignment finished.
    pub finished_at: Option<String>,
    /// Path to the worker workspace.
    pub workspace_path: Option<String>,
    /// Branch name for the worker workspace.
    pub workspace_branch: Option<String>,
    /// Base ref used for the workspace.
    pub workspace_base_ref: Option<String>,
    /// Timestamp when this record was created.
    pub created_at: String,
    /// Timestamp when this record was last updated.
    pub updated_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskRecordData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Task record returned or updated by this operation.
    pub task: TaskRecord,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorktreeData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Task identifier.
    pub task_id: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
    /// Git branch name.
    pub branch: String,
    /// Git base reference.
    pub base_ref: String,
    /// True when the worktree exists and is ready for worker execution.
    #[schemars(description = "True when the worktree exists and is ready for worker execution.")]
    pub ready: bool,
    /// True when the recorded worktree path exists on disk.
    pub exists: bool,
    /// True only when this tool call created a new worktree. False can mean the worktree already existed.
    #[schemars(
        description = "True only when this tool call created a new worktree. False can mean the worktree already existed."
    )]
    /// Whether this tool call created the file, record, or workspace.
    pub created: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorktreeDiffData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Task identifier.
    pub task_id: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
    /// Whether the workspace has local changes.
    pub dirty: bool,
    /// Whether the workspace has meaningful worker changes.
    pub meaningful_changes: bool,
    /// Explanation of the current workspace state.
    pub state_reason: String,
    /// Changed files or file records.
    pub files: Vec<WorktreeDiffFile>,
    /// Bounded Git diff output from the task worktree.
    pub diff: String,
    /// Whether the returned output was truncated.
    pub truncated: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorktreeDiffFile {
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorktreeCleanupData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Task identifier.
    pub task_id: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
    /// Whether the workspace or record was removed.
    pub removed: bool,
    /// Whether the operation was forced.
    pub forced: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkerResultIntegrationData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Task identifier.
    pub task_id: String,
    /// Source backlog item identifier.
    pub source_item_id: String,
    /// Configured merge style for integration.
    pub merge_style: String,
    /// Git branch name.
    pub branch: String,
    /// Git commit hash.
    pub commit: String,
    /// Whether the task worktree was cleaned up after integration.
    pub cleaned_up: bool,
    /// Cleanup error if post-integration cleanup failed.
    pub cleanup_error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IntegrationGate {
    /// Stable gate name for clients that want to group or style results.
    pub name: String,
    /// Gate status: ready, blocked, warning, or unknown.
    pub status: String,
    /// Whether this gate currently blocks integration.
    pub blocking: bool,
    /// Human-readable summary of the gate result.
    pub summary: String,
    /// Recommended MCP tool to call next for this gate, if any.
    pub recommended_tool: Option<String>,
    /// Suggested next action for this gate.
    pub next_action: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IntegrationGateData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Task identifier.
    pub task_id: String,
    /// Source backlog item identifier, if the task exists.
    pub source_item_id: Option<String>,
    /// Whether all blocking integration gates are ready.
    pub ok: bool,
    /// Integration gates enforced or reported by integrate_worker_result.
    pub gates: Vec<IntegrationGate>,
    /// Suggested next action.
    pub next_action: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskBundleData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Generated task bundle for this assignment or task.
    pub bundle: TaskBundle,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, PartialEq, Eq)]
pub struct WorkerCompletionContract {
    /// Human-readable summary of how the worker should report completion.
    pub summary: String,
    /// Required fields or concepts the worker must include in its final result.
    pub required_fields: Vec<String>,
    /// Rule for reporting changed files relative to the assignment worktree.
    pub changed_files_rule: String,
    /// Rule for reporting verification outcome and evidence.
    pub verification_rule: String,
    /// Rule for reporting follow-up findings or explicitly confirming none.
    pub findings_rule: String,
    /// Rule for relating the result back to task acceptance criteria.
    pub acceptance_rule: String,
}

pub fn default_worker_completion_contract() -> WorkerCompletionContract {
    WorkerCompletionContract {
        summary: "Prefer `finish_work` for host-managed work; use `complete_worker_task` only for the lower-level assignment lifecycle. Include summary, changed files, verification status, and findings disposition.".to_string(),
        required_fields: vec![
            "status".to_string(),
            "summary".to_string(),
            "changed_files".to_string(),
            "verification_status".to_string(),
            "findings or findings_reviewed=true".to_string(),
        ],
        changed_files_rule: "Report paths relative to the assignment worktree. Omit changed_files only when the host will infer them from inspect_worktree_changes.".to_string(),
        verification_rule: "Report passed, failed, skipped, or not_run. Include verification_summary and verification_refs when a check passed or was deliberately skipped.".to_string(),
        findings_rule: "Record limitations, follow-up work, and impediments as findings. Set findings_reviewed=true only after checking that no follow-up finding is needed.".to_string(),
        acceptance_rule: "Summarize how the result satisfies each acceptance criterion, or record a required finding when it does not.".to_string(),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, PartialEq, Eq)]
pub struct TaskBundle {
    /// Task identifier.
    pub task_id: String,
    /// Backlog item identifier.
    pub item_id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Worker name associated with this item.
    pub worker: Option<String>,
    /// Path to the worker workspace.
    pub workspace_path: String,
    /// Goal text that drives this request or record.
    pub goal: String,
    /// Implementation contract text for this backlog item.
    pub implementation_contract: String,
    /// Acceptance criteria for this item.
    pub acceptance: Vec<String>,
    /// Backlog item dependencies included in the worker brief.
    pub dependencies: Vec<String>,
    /// Owned code or documentation surfaces for this item.
    pub owned_surfaces: Vec<String>,
    /// Verification command to run or record for this item.
    pub verification_command: Vec<String>,
    /// Execution mode recorded for this assignment bundle.
    ///
    /// `manual_handoff` means an MCP host or human-managed worker is expected
    /// to run the assignment through start_worker_task, complete_worker_task,
    /// evidence recording, and integration.
    #[serde(default = "crate::execution_mode::default_assignment_execution_mode")]
    #[schemars(with = "ExecutionModeSchema")]
    pub execution_mode: String,
    /// Worker completion contract describing how to return result data safely.
    #[serde(default = "default_worker_completion_contract")]
    pub completion_contract: WorkerCompletionContract,
    /// Human-readable worker brief generated from the task and backlog item.
    pub brief: String,
}

#[derive(Debug, Serialize, Clone, JsonSchema)]
pub struct WorkerAssignmentData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Worker assignment returned or updated by this operation.
    pub assignment: WorkerAssignment,
}

#[derive(Debug, Serialize, Clone, JsonSchema)]
pub struct WorkerAssignment {
    /// Stable local record identifier.
    pub id: String,
    /// Task identifier.
    pub task_id: String,
    /// Worker name associated with this item.
    pub worker: Option<String>,
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Name of the agent that created the assignment.
    pub assigned_by: Option<String>,
    /// Filesystem path to the task worktree.
    pub worktree_path: String,
    /// Generated task bundle for this assignment or task.
    pub bundle: TaskBundle,
    /// Execution mode recorded for this assignment.
    #[schemars(with = "ExecutionModeSchema")]
    pub execution_mode: String,
    /// Worker session identifier.
    pub worker_session: Option<String>,
    /// Timestamp when the task or assignment started.
    pub started_at: Option<String>,
    /// Timestamp when the task or assignment completed.
    pub completed_at: Option<String>,
    /// Terminal result status reported by the worker.
    pub result_status: Option<String>,
    /// Human-readable summary of the record or result.
    pub summary: Option<String>,
    /// Files changed by the worker, relative to the task worktree.
    pub changed_files: Vec<String>,
    /// Verification status for the task or result.
    pub verification_status: Option<String>,
    /// Timestamp when this record was created.
    pub created_at: String,
    /// Timestamp when this record was last updated.
    pub updated_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkerAssignmentEventData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Worker assignment identifier.
    pub assignment_id: String,
    /// Task identifier.
    pub task_id: String,
    /// Related event record.
    pub event: TaskEventRecord,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskVerificationRunData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Worker assignment identifier.
    pub assignment_id: String,
    /// Task identifier.
    pub task_id: String,
    /// Verification command executed or inspected.
    pub verification_command: Vec<String>,
    /// Verification execution status.
    ///
    /// Values: passed, failed, skipped, timed_out.
    pub status: String,
    /// Exit code from command execution when available.
    pub exit_code: Option<i32>,
    /// Captured standard output (possibly truncated).
    pub stdout: String,
    /// Captured standard error (possibly truncated).
    pub stderr: String,
    /// Whether stdout was truncated.
    pub stdout_truncated: bool,
    /// Whether stderr was truncated.
    pub stderr_truncated: bool,
    /// Whether this run exceeded the timeout and was terminated.
    pub timed_out: bool,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct ApprovalRecord {
    /// Stable local record identifier.
    pub id: String,
    /// Scope for this record or operation.
    pub scope: String,
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    /// Person or agent requesting the approval.
    pub requested_by: Option<String>,
    /// Approval response text.
    pub response: Option<String>,
    /// Name of the person or agent responding.
    pub responder: Option<String>,
    /// Human-readable reason for the decision or result.
    pub reason: Option<String>,
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
    /// Timestamp when this record was created.
    pub created_at: String,
    /// Timestamp for responded.
    pub responded_at: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ApprovalListData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Approval records returned by this request.
    pub approvals: Vec<ApprovalRecord>,
    /// Number of records returned.
    pub returned: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ApprovalResponseData {
    /// Approval record returned or updated by this operation.
    pub approval: ApprovalRecord,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct EventRecord {
    /// Opaque cursor for event replay ordering.
    pub cursor: String,
    /// Event type name used for filtering and replay.
    pub event_type: String,
    /// Scope for this record or operation.
    pub scope: String,
    /// Task identifier.
    pub task_id: Option<String>,
    /// Human-readable summary of the record or result.
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Structured payload for this record.
    pub payload: Option<Value>,
    /// Timestamp when this record was created.
    pub created_at: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub replay_order: i64,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct RuntimeTransitionRecord {
    /// Opaque cursor for event replay ordering.
    pub cursor: String,
    /// Runtime domain for this transition record.
    pub domain: String,
    /// Entity identifier.
    pub entity_id: String,
    /// Transition type name used for replay and audit.
    pub transition_type: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Structured payload for this record.
    pub payload: Option<Value>,
    /// Timestamp when this record was created.
    pub created_at: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub replay_order: i64,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct LeaseRecord {
    /// Stable local record identifier.
    pub id: String,
    /// Scope for this record or operation.
    pub scope: String,
    /// Target identifier for this record.
    pub target_id: String,
    /// Owner name or repository owner, depending on context.
    pub owner: String,
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
    /// Timestamp for acquired.
    pub acquired_at: String,
    /// Timestamp for renewed.
    pub renewed_at: Option<String>,
    /// Timestamp for released.
    pub released_at: Option<String>,
    /// Timestamp for expires.
    pub expires_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LeaseRecordData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Lease record returned or updated by this operation.
    pub lease: LeaseRecord,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LeaseListData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Lease records returned by this request.
    pub leases: Vec<LeaseRecord>,
    /// Number of records returned.
    pub returned: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EventsReplayData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Event records in this response.
    pub events: Vec<EventRecord>,
    /// Number of records returned.
    pub returned: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StorageCapabilityProbeData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// State backend name exercised by this probe.
    pub backend: String,
    /// Whether the operation succeeded.
    pub ok: bool,
    /// Diagnostic checks included in this result.
    pub checks: Vec<StorageCapabilityCheck>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StorageCapabilityCheck {
    /// Name of this check, profile, or record.
    pub name: String,
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Additional detail text.
    pub detail: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct EvidenceRecord {
    /// Stable local record identifier.
    pub id: String,
    /// Source backlog item identifier.
    pub source_item_id: Option<String>,
    /// Source task identifier.
    pub source_task_id: Option<String>,
    /// Kind or category for this record.
    pub kind: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    /// References such as commands, files, commits, URLs, or evidence IDs.
    pub refs: Vec<String>,
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
    /// Timestamp when this record was created.
    pub created_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EvidenceRecordData {
    /// Evidence records returned or created by this operation.
    pub evidence: EvidenceRecord,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EvidenceListData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Evidence records returned or created by this operation.
    pub evidence: Vec<EvidenceRecord>,
    /// Number of records returned.
    pub returned: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReconciliationData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Whether the operation succeeded.
    pub ok: bool,
    /// Backlog item IDs considered closed by Git trailers.
    pub closed_item_ids: Vec<String>,
    /// Number of completed tasks in the project state.
    pub completed_tasks: usize,
    /// Number of required findings without accepted disposition.
    pub unresolved_required_findings: usize,
    /// Reconciliation gaps that need follow-up.
    pub gaps: Vec<ReconciliationGap>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReconciliationGap {
    /// Kind or category for this record.
    pub kind: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    /// Suggested next action.
    pub next_action: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn action_result_separates_continuation_from_recovery() {
        let completed = ActionResult::completed(
            "ping",
            "ok",
            PingData {
                echo: "pong".to_string(),
            },
        );
        let failed = ActionResult::<PingData>::failed("ping", "failed", "boom");

        assert_eq!(completed.next_action, None);
        assert_eq!(completed.recovery_action, None);
        assert_eq!(
            serde_json::to_value(&failed).expect("failed json")["recovery_action"],
            json!("Inspect the request and project setup.")
        );
        assert_eq!(
            serde_json::to_value(&failed).expect("failed json")["next_action"],
            json!("Inspect the request and project setup.")
        );
    }
}
