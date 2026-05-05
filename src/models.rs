use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PingParams {
    pub message: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RootParams {
    pub root: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InitProjectParams {
    pub root: Option<String>,
    pub project_name: Option<String>,
    pub overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LimitParams {
    pub root: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ValidateBacklogParams {
    pub root: Option<String>,
    pub include_errors: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DraftBacklogItemsParams {
    pub goal: String,
    pub suggested_worker: Option<String>,
    #[serde(default)]
    pub owned_surfaces: Vec<String>,
    #[serde(default)]
    pub required_capability_tags: Vec<String>,
    pub complexity_tier: Option<String>,
    #[serde(default)]
    pub verification_command: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateBacklogItemParams {
    pub root: Option<String>,
    pub id: Option<String>,
    pub id_prefix: Option<String>,
    pub title: String,
    pub priority: Option<String>,
    #[serde(rename = "type")]
    pub item_type: Option<String>,
    pub area: Option<String>,
    pub epic: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
    pub owner_role: Option<String>,
    pub assigned_worker: Option<String>,
    pub assignment_reason: Option<String>,
    pub suggested_worker: Option<String>,
    #[serde(default)]
    pub required_capability_tags: Vec<String>,
    pub complexity_tier: Option<String>,
    #[serde(default)]
    pub owned_surfaces: Vec<String>,
    pub goal: String,
    pub implementation_contract: Option<String>,
    pub contract: Option<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    pub notes: Option<String>,
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
    pub source_finding_ref: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectTaskEventsParams {
    pub root: Option<String>,
    pub task_id: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendWorkerGuidanceParams {
    pub root: Option<String>,
    pub task_id: String,
    pub message: String,
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordFindingParams {
    pub root: Option<String>,
    pub id: Option<String>,
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
    pub source_finding_ref: Option<String>,
    pub title: String,
    pub summary: String,
    pub severity: Option<String>,
    pub required: Option<bool>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListFindingsParams {
    pub root: Option<String>,
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
    pub status: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ValidateFindingsParams {
    pub root: Option<String>,
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateFindingDispositionParams {
    pub root: Option<String>,
    pub finding_id: String,
    pub status: String,
    pub owner: Option<String>,
    pub disposition_reason: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Completed,
    Skipped,
    Failed,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ActionResult<T: Serialize + JsonSchema> {
    pub action: String,
    pub status: ActionStatus,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    pub echo: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectStatusData {
    pub root: String,
    pub platy_yaml: bool,
    pub backlog_dir: bool,
    pub git_metadata: bool,
    pub backlog_items: usize,
    pub runnable_backlog_items: usize,
    pub tasks_supported: bool,
    pub findings_supported: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DoctorSnapshotData {
    pub root: String,
    pub ok: bool,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorCheckStatus,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    pub root: String,
    pub created: usize,
    pub skipped: usize,
    pub entries: Vec<ScaffoldEntry>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ScaffoldEntry {
    pub path: String,
    pub kind: String,
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
    pub source: String,
    pub item_id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub area: String,
    pub suggested_worker: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BacklogListData {
    pub root: String,
    pub candidates: Vec<BacklogCandidate>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BacklogValidationData {
    pub root: String,
    pub ok: bool,
    pub item_count: usize,
    pub epic_count: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DraftBacklogData {
    pub drafts: Vec<DraftBacklogItem>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DraftBacklogItem {
    pub candidate_id: String,
    pub title: String,
    pub objective: String,
    pub owned_surfaces: Vec<String>,
    pub suggested_worker: Option<String>,
    pub required_capability_tags: Vec<String>,
    pub complexity_tier: Option<String>,
    pub verification_command: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CreatedBacklogItemData {
    pub item_id: String,
    pub path: String,
    pub created: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingRecordData {
    pub finding: FindingRecord,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingListData {
    pub root: String,
    pub findings: Vec<FindingRecord>,
    pub returned: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingValidationData {
    pub root: String,
    pub ok: bool,
    pub unresolved_required_count: usize,
    pub unresolved_required: Vec<FindingRecord>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingDispositionData {
    pub finding: FindingRecord,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct FindingRecord {
    pub id: String,
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
    pub source_finding_ref: Option<String>,
    pub title: String,
    pub status: String,
    pub severity: Option<String>,
    pub required: bool,
    pub summary: String,
    pub owner: Option<String>,
    pub disposition_reason: Option<String>,
    pub evidence_refs: Vec<String>,
    pub metadata: BTreeMap<String, Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskEventListData {
    pub root: String,
    pub task_id: String,
    pub events: Vec<TaskEventRecord>,
    pub returned: usize,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct TaskEventRecord {
    pub task_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    pub created_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct UnsupportedData {
    pub supported: bool,
    pub reason: String,
}
