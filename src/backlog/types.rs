use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

pub(super) const VALID_PRIORITIES: &[&str] = &["P0", "P1", "P2"];
pub(super) const VALID_TYPES: &[&str] = &["foundation", "feature", "safety", "ux", "test", "docs"];
pub(super) const REQUIRED_SECTIONS: &[&str] = &["Goal", "Implementation Contract", "Acceptance"];

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct BacklogItemFrontmatter {
    pub id: String,
    pub title: String,
    pub priority: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub area: String,
    pub epic: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub suggested_worker: Option<String>,
    #[serde(default)]
    pub owned_surfaces: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
    pub priority: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub area: String,
    pub epic: String,
    pub depends_on: Vec<String>,
    pub suggested_worker: Option<String>,
    pub owned_surfaces: Vec<String>,
}

#[derive(Debug)]
pub(super) struct BacklogValidation {
    pub ok: bool,
    pub errors: Vec<String>,
    pub items: Vec<ParsedBacklogItem>,
    pub epic_ids: BTreeSet<String>,
}
