use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

pub(super) const VALID_STATUSES: &[&str] = &[
    "todo",
    "ready",
    "in_progress",
    "in_review",
    "blocked",
    "done",
    "deferred",
];
pub(super) const VALID_PRIORITIES: &[&str] = &["P0", "P1", "P2"];
pub(super) const VALID_TYPES: &[&str] = &["foundation", "feature", "safety", "ux", "test", "docs"];
pub(super) const REQUIRED_SECTIONS: &[&str] = &["Goal", "Implementation Contract", "Acceptance"];

#[derive(Debug, Deserialize, Clone)]
pub(super) struct BacklogItemFrontmatter {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub area: String,
    pub epic: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
    pub suggested_worker: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EpicFrontmatter {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub area: String,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedBacklogItem {
    pub frontmatter: BacklogItemFrontmatter,
    pub path: PathBuf,
    pub sections: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct BacklogItemFrontmatterOut {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub area: String,
    pub epic: String,
    pub depends_on: Vec<String>,
    pub blocks: Vec<String>,
    pub owner_role: Option<String>,
    pub assigned_worker: Option<String>,
    pub assignment_reason: Option<String>,
    pub suggested_worker: Option<String>,
    pub required_capability_tags: Vec<String>,
    pub complexity_tier: Option<String>,
    pub owned_surfaces: Vec<String>,
    pub github_issue: Option<u64>,
    pub github_project_item: Option<String>,
    pub prs: Vec<u64>,
    pub source_item_id: Option<String>,
    pub source_task_id: Option<String>,
    pub source_finding_ref: Option<String>,
}

#[derive(Debug)]
pub(super) struct BacklogValidation {
    pub ok: bool,
    pub errors: Vec<String>,
    pub items: Vec<ParsedBacklogItem>,
    pub epic_ids: BTreeSet<String>,
}
