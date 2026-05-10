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

fn example_id_prefix() -> String {
    "PROJ".to_string()
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
pub enum GoalWorkflowModeSchema {
    Auto,
    DirectScaffold,
    Hybrid,
    PlatypusWorkflow,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskPlanModeSchema {
    Direct,
    Standard,
    Full,
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
pub enum AgentRoleSchema {
    Manager,
    Worker,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentHarnessSchema {
    Codex,
    Claude,
    Fake,
    Custom,
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
    pub max_tasks: Option<usize>,
    /// Worker name associated with this item.
    pub worker: Option<String>,
    /// Name recorded as the task claimant.
    pub claimant: Option<String>,
    /// Whether to claim tasks, create worktrees, and persist worker handoff bundles during dispatch.
    pub prepare_handoffs: Option<bool>,
    /// Whether to immediately mark prepared assignments as running in the local
    /// lifecycle state machine. This does not launch an external worker process.
    pub auto_start: Option<bool>,
    /// Whether dispatch should auto-commit tracked planning artifacts when local
    /// dirt is limited to `backlog/items/*.md` and `backlog/plans/*.yaml`.
    pub auto_commit_artifacts: Option<bool>,
    /// Preview dispatchable work without mutating task, assignment, or Git
    /// lifecycle state.
    #[schemars(example = example_true())]
    pub dry_run: Option<bool>,
    #[serde(default)]
    /// Verification command to run or record for this item.
    pub verification_command: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NextSafeActionParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectWorkQueueParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
    pub limit: Option<usize>,
    /// Whether dispatch readiness should require task-plan artifacts for non-direct work.
    pub require_task_plan: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClassifyPlanningNeedsParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Backlog item identifier.
    #[schemars(example = example_item_id(), pattern(r"^[A-Z]+-[0-9]{3}$"))]
    pub item_id: Option<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClassifyWorkflowFitParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Goal text that drives this request or record.
    #[schemars(example = example_goal())]
    pub goal: String,
    #[serde(default)]
    /// Relative paths or top-level areas the work is expected to touch. Use broad
    /// directories for early scaffolding, for example `backend/` and `frontend/`.
    /// These values guide planning mode and later changed-file validation; use an
    /// empty list only when the surface is genuinely unknown.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlanGoalWorkParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Goal text to classify and turn into concrete next-tool guidance.
    #[schemars(example = example_goal())]
    pub goal: String,
    /// Workflow mode. Omit or pass null to let Platypus choose automatically.
    /// Supported values are auto, direct_scaffold, hybrid, and platypus_workflow.
    #[schemars(with = "Option<GoalWorkflowModeSchema>")]
    pub mode: Option<String>,
    #[serde(default)]
    /// Relative paths or top-level areas the work is expected to touch. Use broad
    /// directories for early scaffolding, for example `backend/` and `frontend/`.
    /// These values guide planning mode and later changed-file validation; use an
    /// empty list only when the surface is genuinely unknown.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartGoalWorkParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Goal text that drives this request or record.
    #[schemars(example = example_goal())]
    pub goal: String,
    /// Workflow mode. Omit or pass null to let Platypus choose automatically.
    /// Supported values are auto, direct_scaffold, hybrid, and platypus_workflow.
    #[schemars(with = "Option<GoalWorkflowModeSchema>")]
    pub mode: Option<String>,
    /// When true, create or reuse tracking and immediately route the item
    /// through dispatch. This can auto-commit tracking artifacts, prepare
    /// worker handoffs/worktrees, and mark assignments running depending on the
    /// other start_goal_work flags.
    #[schemars(example = example_true())]
    pub dispatch: Option<bool>,
    /// Whether dispatch should prepare worker handoffs and task worktrees when
    /// dispatch=true.
    #[schemars(example = example_true())]
    pub prepare_handoffs: Option<bool>,
    /// Whether dispatch should immediately mark prepared assignments as running
    /// in local lifecycle state when dispatch=true.
    #[schemars(example = example_true())]
    pub auto_start: Option<bool>,
    /// Whether dispatch should auto-commit tracked planning artifacts when
    /// local Git dirt is limited to backlog items/plans and dispatch=true.
    #[schemars(example = example_true())]
    pub auto_commit_artifacts: Option<bool>,
    /// Also create one lightweight backlog tracking item for this goal. This is
    /// useful when the recommendation is direct_scaffold but the baseline should
    /// still be visible in Platypus after the host agent edits files directly.
    pub also_track: Option<bool>,
    /// Legacy compatibility guard for the removed scaffold_in_place flag. This
    /// field is intentionally hidden from the MCP schema; callers that still
    /// send it receive a structured failure before any project mutation.
    #[schemars(skip)]
    pub scaffold_in_place: Option<bool>,
    /// Maximum number of tasks to include or process.
    #[schemars(range(min = 1, max = 10))]
    pub max_tasks: Option<usize>,
    /// Suggested worker name for this item.
    pub suggested_worker: Option<String>,
    #[serde(default)]
    /// Relative paths or top-level areas the work is expected to touch. Use broad
    /// directories for early scaffolding, for example `backend/` and `frontend/`.
    /// These values guide planning mode and later changed-file validation; use an
    /// empty list only when the surface is genuinely unknown.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Vec<String>,
    #[serde(default)]
    /// Verification command to run or record for this item.
    #[schemars(example = example_verification_command())]
    pub verification_command: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InitProjectParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Project name written into initialized Platypus configuration.
    pub project_name: Option<String>,
    /// Whether to overwrite an existing file or record.
    pub overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LimitParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
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
    pub ttl_seconds: Option<u64>,
    #[serde(default)]
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
    pub include_expired: Option<bool>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
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
    pub include_errors: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DraftBacklogItemsParams {
    /// Goal text that drives this request or record.
    #[schemars(example = example_goal())]
    pub goal: String,
    /// Suggested worker name for this item.
    #[schemars(example = example_worker())]
    pub suggested_worker: Option<String>,
    #[serde(default)]
    /// Owned code or documentation surfaces for this item.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Vec<String>,
    #[serde(default)]
    /// Verification command to run or record for this item.
    #[schemars(example = example_verification_command())]
    pub verification_command: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DraftExternalBacklogItemsParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// External provider name.
    pub provider: String,
    #[serde(default)]
    /// External work records supplied by the host client.
    pub records: Vec<ExternalWorkRecord>,
    /// Suggested worker name for this item.
    #[schemars(example = example_worker())]
    pub suggested_worker: Option<String>,
    #[serde(default)]
    /// Owned code or documentation surfaces for this item.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Vec<String>,
    #[serde(default)]
    /// Verification command to run or record for this item.
    #[schemars(example = example_verification_command())]
    pub verification_command: Vec<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
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
    #[serde(default)]
    /// Issue records supplied by the host client.
    pub issues: Vec<GitHubIssueRecord>,
    /// State or status value.
    pub state: Option<String>,
    /// Identifier prefix used when allocating new local backlog item IDs.
    #[schemars(example = example_id_prefix(), pattern(r"^[A-Z]+$"))]
    pub id_prefix: Option<String>,
    /// Suggested worker name for this item.
    #[schemars(example = example_worker())]
    pub suggested_worker: Option<String>,
    #[serde(default)]
    /// Owned code or documentation surfaces for this item.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Vec<String>,
    #[serde(default)]
    /// Verification command to run or record for this item.
    #[schemars(example = example_verification_command())]
    pub verification_command: Vec<String>,
    /// Maximum number of records to return.
    #[schemars(range(min = 1, max = 200))]
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
    #[serde(default)]
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
    #[serde(default)]
    /// Dependencies that must be satisfied first.
    #[schemars(inner(pattern(r"^[A-Z]+-[0-9]{3}$")))]
    pub depends_on: Vec<String>,
    /// Suggested worker name for this item.
    #[schemars(example = example_worker())]
    pub suggested_worker: Option<String>,
    #[serde(default)]
    /// Relative paths or top-level areas the work is expected to touch. Use broad
    /// directories for early scaffolding, for example `backend/` and `frontend/`.
    /// These values guide planning mode and later changed-file validation; use an
    /// empty list only when the surface is genuinely unknown.
    #[schemars(example = example_owned_surfaces())]
    pub owned_surfaces: Vec<String>,
    #[serde(default)]
    /// External references tied to this record.
    pub external_refs: Vec<ExternalRef>,
    #[serde(default)]
    /// Goal text that drives this request or record.
    #[schemars(example = example_goal())]
    pub goal: String,
    /// Implementation contract text for this backlog item.
    pub implementation_contract: Option<String>,
    /// Optional contract text for this backlog item.
    pub contract: Option<String>,
    #[serde(default)]
    /// Acceptance criteria for this item.
    pub acceptance: Vec<String>,
    /// Optional notes for this item.
    pub notes: Option<String>,
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
pub struct DraftTaskPlanParams {
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
pub struct WorktreeCleanupParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: String,
    /// Whether to force the operation.
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
    pub allow_unverified: Option<bool>,
    /// Remove the task worktree after successful integration when it is clean.
    pub cleanup_after: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateTaskBundleParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Task identifier.
    #[schemars(example = example_task_id())]
    pub task_id: String,
    #[serde(default)]
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
    /// Git base reference.
    pub base_ref: Option<String>,
    #[serde(default)]
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
    #[serde(default)]
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
    #[serde(default)]
    /// Files changed by the worker, relative to the task worktree.
    pub changed_files: Vec<String>,
    /// Verification status for the task or result.
    #[schemars(with = "Option<VerificationStatusSchema>")]
    pub verification_status: Option<String>,
    /// Whether completion may auto-start a prepared assignment before marking
    /// it terminal. Defaults to true so same-session MCP hosts can finish a
    /// prepared task without a separate start_worker_task call.
    pub auto_start_if_prepared: Option<bool>,
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
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunnerPrepareParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Worker name associated with this item.
    pub worker: Option<String>,
    /// Name recorded as the task claimant.
    pub claimant: Option<String>,
    /// Maximum number of tasks to include or process.
    #[schemars(range(min = 1, max = 10))]
    pub max_tasks: Option<usize>,
    /// Whether to inspect the operation without mutating state.
    pub dry_run: Option<bool>,
    #[serde(default)]
    /// Verification command to run or record for this item.
    pub verification_command: Vec<String>,
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
    #[serde(default)]
    /// References such as commands, files, commits, URLs, or evidence IDs.
    pub refs: Vec<String>,
    #[serde(default)]
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
    #[serde(default)]
    /// References such as commands, files, commits, URLs, or evidence IDs.
    pub refs: Vec<String>,
    #[serde(default)]
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
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReconcileParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AgentProfilesParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConfigureAgentProfileParams {
    /// Project root that bounds all file, Git, and state operations.
    pub root: Option<String>,
    /// Name of this check, profile, or record.
    pub name: String,
    /// Role name for this agent profile.
    #[schemars(with = "AgentRoleSchema")]
    pub role: String,
    /// Harness name for this agent profile.
    #[schemars(with = "AgentHarnessSchema")]
    pub harness: String,
    /// Executable path or command for this profile.
    pub executable: String,
    #[serde(default)]
    /// Capability names advertised by this profile.
    pub capabilities: Vec<String>,
    #[serde(default)]
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
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

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkflowConfigData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Effective workflow integration policy.
    pub integration: WorkflowIntegrationConfig,
    /// Effective workflow dispatch policy.
    pub dispatch: WorkflowDispatchConfig,
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
    /// Whether this finding or item must be resolved before completion.
    pub required: Option<bool>,
    #[serde(default)]
    /// References for evidence.
    pub evidence_refs: Vec<String>,
    #[serde(default)]
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
    #[serde(default)]
    /// References for evidence.
    pub evidence_refs: Vec<String>,
    #[serde(default)]
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
    /// Suggested next action.
    pub next_action: Option<String>,
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
            data: None,
            error: None,
        }
    }

    pub fn failed(action: &str, summary: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            action: action.to_string(),
            status: ActionStatus::Failed,
            summary: summary.into(),
            next_action: Some("Inspect the request and project setup.".to_string()),
            data: None,
            error: Some(error.into()),
        }
    }
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
pub struct NextSafeActionData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Recommended Platypus MCP tool to call next.
    pub recommended_tool: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    /// Human-readable reason for the decision or result.
    pub reason: String,
    /// Suggested parameters for the recommended tool call.
    pub params: BTreeMap<String, Value>,
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
    /// Number of backlog items omitted from ready dispatch because active tasks already exist.
    pub active_count: usize,
    /// Backlog item identifiers that already have active task lifecycle state.
    pub active_item_ids: Vec<String>,
    /// Warnings that should be resolved before dispatching work.
    pub preflight_warnings: Vec<String>,
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

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkQueueItem {
    /// One-based position in the returned queue.
    pub position: usize,
    /// Backlog candidate chosen or inspected by this operation.
    pub candidate: BacklogCandidate,
    /// Planning classification for this backlog candidate.
    pub planning: PlanningClassification,
    /// Task plan state or file for this item.
    pub plan: WorkQueuePlanState,
    /// Whether this item can be dispatched now.
    pub ready_to_dispatch: bool,
    /// Recommended Platypus MCP tool to call next.
    pub recommended_tool: String,
    /// Human-readable reason for the decision or result.
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema)]
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
    /// Planning mode required before this item can be dispatched.
    pub required_mode: String,
    /// Required planning artifact path, if one is needed.
    pub required_artifact: Option<String>,
    /// Human-readable reasons behind this decision.
    pub reasons: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PlanningClassificationData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Planning classifications returned by this request.
    pub classifications: Vec<PlanningClassification>,
    /// Number of records returned.
    pub returned: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkflowFitData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Recommended workflow mode for the current goal.
    pub recommended_mode: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    /// Human-readable reasons behind this decision.
    pub reasons: Vec<String>,
    /// Suggested next action.
    pub next_action: String,
    /// Number of local backlog items discovered.
    pub backlog_items: usize,
    /// Number of backlog items currently runnable.
    pub runnable_backlog_items: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PlanGoalWorkData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Recommended workflow mode for the current goal.
    pub recommended_mode: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    /// Human-readable reasons behind this decision.
    pub reasons: Vec<String>,
    /// Whether a lightweight Platypus tracking item is useful for this goal.
    pub tracking_recommended: bool,
    /// Recommended Platypus MCP tool to call next.
    pub recommended_tool: String,
    /// Concrete JSON arguments suitable for the recommended tool.
    pub recommended_arguments: BTreeMap<String, Value>,
    /// Suggested next action.
    pub next_action: String,
    /// Number of local backlog items discovered.
    pub backlog_items: usize,
    /// Number of backlog items currently runnable.
    pub runnable_backlog_items: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StartGoalWorkData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Recommended workflow mode for the current goal.
    pub recommended_mode: String,
    /// Human-readable summary of the record or result.
    pub summary: String,
    /// Backlog items created by this operation.
    pub created_items: Vec<CreatedBacklogItemData>,
    /// Primary backlog item created by this operation, if one was created.
    pub created_item_id: Option<String>,
    /// Task plans created by this operation.
    pub created_plans: Vec<TaskPlanWriteData>,
    /// Tasks dispatched by this operation.
    pub dispatched_tasks: Vec<DispatchReadyWorkItem>,
    /// Worker assignment identifiers created by dispatch in this operation.
    pub dispatched_assignment_ids: Vec<String>,
    /// Items that could not be advanced and why.
    pub blocked_items: Vec<GoalWorkBlockedItem>,
    /// Non-fatal warnings produced by this operation.
    pub warnings: Vec<String>,
    /// Suggested next action.
    pub next_action: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalWorkBlockedItem {
    /// Backlog item identifier.
    pub item_id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Recommended Platypus MCP tool to call next.
    pub recommended_tool: String,
    /// Human-readable reason for the decision or result.
    pub reason: String,
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
    /// Suggested worker name for this item.
    pub suggested_worker: Option<String>,
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

#[derive(Debug, Serialize, JsonSchema)]
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
    /// Suggested worker name for this item.
    pub suggested_worker: Option<String>,
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

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DraftBacklogData {
    /// Draft backlog items produced by this operation.
    pub drafts: Vec<DraftBacklogItem>,
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
    /// Suggested worker name for this item.
    pub suggested_worker: Option<String>,
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, JsonSchema)]
pub struct DraftBacklogItem {
    /// Candidate identifier.
    pub candidate_id: String,
    /// Human-readable title or short label.
    pub title: String,
    /// Objective for this candidate or record.
    pub objective: String,
    #[serde(rename = "type")]
    /// Backlog item type.
    pub item_type: String,
    /// Primary area or surface for this item.
    pub area: String,
    /// Owned code or documentation surfaces for this item.
    pub owned_surfaces: Vec<String>,
    /// Suggested worker name for this item.
    pub suggested_worker: Option<String>,
    /// Verification command to run or record for this item.
    pub verification_command: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CreatedBacklogItemData {
    /// Backlog item identifier.
    pub item_id: String,
    /// Filesystem path for the local file or workspace.
    pub path: String,
    /// Whether this tool call created the file, record, or workspace.
    pub created: bool,
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
    /// Suggested worker name for this item.
    pub suggested_worker: Option<String>,
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
    /// Number of required findings still unresolved.
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
    /// Whether this finding or item must be resolved before completion.
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
pub struct TaskBundleData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Generated task bundle for this assignment or task.
    pub bundle: TaskBundle,
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

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunnerReportData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Number of tasks requested for processing.
    pub requested: usize,
    /// Number of queued tasks claimed.
    pub claimed: usize,
    /// Number of worker handoffs prepared.
    pub prepared: usize,
    /// Reason the batch stopped.
    pub stopped_reason: String,
    /// Planned implementation tasks.
    pub tasks: Vec<RunnerTaskSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunnerTaskSummary {
    /// Worker assignment identifier.
    pub assignment_id: Option<String>,
    /// Task identifier.
    pub task_id: String,
    /// Backlog item identifier.
    pub item_id: String,
    /// Lifecycle or result status for this record.
    pub status: String,
    /// Path to the worker workspace.
    pub workspace_path: Option<String>,
    /// Whether a worker bundle was generated for this task.
    pub bundle_generated: bool,
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

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct AgentProfile {
    /// Name of this check, profile, or record.
    pub name: String,
    /// Role name for this agent profile.
    pub role: String,
    /// Harness name for this agent profile.
    pub harness: String,
    /// Executable path or command for this profile.
    pub executable: String,
    /// Capability names advertised by this profile.
    pub capabilities: Vec<String>,
    /// Free-form metadata attached to this record.
    pub metadata: BTreeMap<String, Value>,
    /// Whether this profile or workspace is ready for use.
    pub ready: bool,
    /// Issue records supplied by the host client.
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AgentProfilesData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Agent profiles returned by this request.
    pub profiles: Vec<AgentProfile>,
    /// Number of records returned.
    pub returned: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AgentProfileData {
    /// Project root that bounds all file, Git, and state operations.
    pub root: String,
    /// Agent profile returned or updated by this operation.
    pub profile: AgentProfile,
}
