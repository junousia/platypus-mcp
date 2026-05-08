use crate::{
    backlog,
    models::{
        ActionResult, ActionStatus, DraftExternalBacklogItemsParams, ExternalBacklogDraft,
        ExternalBacklogDraftData, ExternalRef, ExternalWorkRecord,
    },
};
use std::{collections::BTreeSet, path::Path};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

pub trait ExternalIntakeAdapter {
    fn provider(&self) -> &str;
    fn draft(
        &self,
        request: ExternalIntakeRequest,
    ) -> Result<Vec<ExternalBacklogDraft>, ExternalIntakeError>;
}

#[derive(Debug, Clone)]
pub struct ExternalIntakeRequest {
    pub provider: String,
    pub records: Vec<ExternalWorkRecord>,
    pub existing_refs: BTreeSet<ExternalRefKey>,
    pub suggested_worker: Option<String>,
    pub owned_surfaces: Vec<String>,
    pub verification_command: Vec<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExternalRefKey {
    pub provider: String,
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalIntakeError {
    message: String,
}

impl ExternalIntakeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ExternalIntakeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for ExternalIntakeError {}

pub struct HostProvidedIntakeAdapter;

impl ExternalIntakeAdapter for HostProvidedIntakeAdapter {
    fn provider(&self) -> &str {
        "host_provided"
    }

    fn draft(
        &self,
        request: ExternalIntakeRequest,
    ) -> Result<Vec<ExternalBacklogDraft>, ExternalIntakeError> {
        if request.provider.trim().is_empty() {
            return Err(ExternalIntakeError::new("provider is required"));
        }
        let provider = clean_token("provider", &request.provider)?;
        let worker = request
            .suggested_worker
            .filter(|value| !value.trim().is_empty())
            .or_else(|| Some("coder".to_string()));
        let verification = if request.verification_command.is_empty() {
            vec!["make".to_string(), "check".to_string()]
        } else {
            request.verification_command
        };
        let mut drafts = Vec::new();
        for record in request.records.into_iter().take(request.limit) {
            let kind = clean_token("kind", &record.kind)?;
            let id = clean_required("id", &record.id)?;
            let title = clean_required("title", &record.title)?;
            let external_ref = ExternalRef {
                provider: provider.clone(),
                kind,
                id,
                url: clean_optional(record.url),
                locator: clean_optional(record.locator),
                imported_at: None,
                source_hash: clean_optional(record.source_hash),
            };
            if external_ref.url.is_none() && external_ref.locator.is_none() {
                return Err(ExternalIntakeError::new("record requires url or locator"));
            }
            let key = ExternalRefKey::from_ref(&external_ref);
            let skipped = request.existing_refs.contains(&key);
            let objective = record
                .body
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(title.as_str())
                .to_string();
            drafts.push(ExternalBacklogDraft {
                candidate_id: format!("{}:{}:{}", key.provider, key.kind, key.id),
                title,
                objective,
                priority: priority_from_labels(&record.labels).to_string(),
                item_type: type_from_labels(&record.labels).to_string(),
                area: area_from_labels(&record.labels).to_string(),
                owned_surfaces: request.owned_surfaces.clone(),
                suggested_worker: worker.clone(),
                verification_command: verification.clone(),
                external_ref,
                labels: record.labels,
                mapping_reasons: vec![
                    "mapped from host-provided external work record".to_string(),
                    "local backlog item remains executable snapshot".to_string(),
                ],
                skipped,
                skip_reason: skipped.then(|| "external reference already imported".to_string()),
            });
        }
        Ok(drafts)
    }
}

impl ExternalRefKey {
    pub fn from_ref(reference: &ExternalRef) -> Self {
        Self {
            provider: reference.provider.trim().to_ascii_lowercase(),
            kind: reference.kind.trim().to_ascii_lowercase(),
            id: reference.id.trim().to_string(),
        }
    }
}

pub fn draft_external_backlog_items(
    default_root: &Path,
    params: DraftExternalBacklogItemsParams,
) -> ActionResult<ExternalBacklogDraftData> {
    let action = "draft_external_backlog_items";
    let root = match backlog::resolve_backlog_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(action, "Could not draft external backlog items.", error)
        }
    };
    let existing_refs = match backlog::external_ref_keys(&root) {
        Ok(keys) => keys,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not inspect existing external references.",
                error,
            )
        }
    };
    let adapter = HostProvidedIntakeAdapter;
    let request = ExternalIntakeRequest {
        provider: params.provider.clone(),
        records: params.records,
        existing_refs,
        suggested_worker: params.suggested_worker,
        owned_surfaces: params.owned_surfaces,
        verification_command: params.verification_command,
        limit: bounded_limit(params.limit),
    };
    let drafts = match adapter.draft(request) {
        Ok(drafts) => drafts,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not draft external backlog items.",
                error.to_string(),
            )
        }
    };
    let skipped = drafts.iter().filter(|draft| draft.skipped).count();
    let returned = drafts.len();
    let data = ExternalBacklogDraftData {
        root: root.display().to_string(),
        provider: params.provider,
        drafts,
        returned,
        skipped,
    };
    if returned == 0 || returned == skipped {
        ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No new external backlog candidates.".to_string(),
            next_action: Some(
                "Provide external records that have not already been imported.".to_string(),
            ),
            data: Some(data),
            error: None,
        }
    } else {
        ActionResult::completed(
            action,
            format!("Drafted {returned} external backlog candidate(s)."),
            data,
        )
    }
}

fn bounded_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn clean_token(field: &str, value: &str) -> Result<String, ExternalIntakeError> {
    let token = clean_required(field, value)?;
    if token
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        Ok(token.to_ascii_lowercase())
    } else {
        Err(ExternalIntakeError::new(format!(
            "{field} must contain ASCII letters, digits, '-' or '_'"
        )))
    }
}

fn clean_required(field: &str, value: &str) -> Result<String, ExternalIntakeError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(ExternalIntakeError::new(format!("{field} is required")))
    } else {
        Ok(trimmed.to_string())
    }
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

fn priority_from_labels(labels: &[String]) -> &'static str {
    if labels.iter().any(|label| label.eq_ignore_ascii_case("p0")) {
        "P0"
    } else if labels.iter().any(|label| label.eq_ignore_ascii_case("p2")) {
        "P2"
    } else {
        "P1"
    }
}

fn type_from_labels(labels: &[String]) -> &'static str {
    for (label, item_type) in [
        ("foundation", "foundation"),
        ("safety", "safety"),
        ("ux", "ux"),
        ("test", "test"),
        ("docs", "docs"),
    ] {
        if labels
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(label))
        {
            return item_type;
        }
    }
    "feature"
}

fn area_from_labels(labels: &[String]) -> String {
    labels
        .iter()
        .find_map(|label| label.strip_prefix("area:").map(str::trim))
        .filter(|area| !area.is_empty())
        .unwrap_or("integrations")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_adapter_maps_and_dedupes_records() {
        let mut existing = BTreeSet::new();
        existing.insert(ExternalRefKey {
            provider: "github".to_string(),
            kind: "issue".to_string(),
            id: "owner/repo#1".to_string(),
        });
        let adapter = HostProvidedIntakeAdapter;

        let drafts = adapter
            .draft(ExternalIntakeRequest {
                provider: "github".to_string(),
                records: vec![
                    ExternalWorkRecord {
                        kind: "issue".to_string(),
                        id: "owner/repo#1".to_string(),
                        title: "Existing issue".to_string(),
                        body: Some("Existing body".to_string()),
                        url: Some("https://github.com/owner/repo/issues/1".to_string()),
                        locator: None,
                        labels: vec!["p0".to_string(), "area:storage".to_string()],
                        metadata: Default::default(),
                        source_hash: Some("sha256:1".to_string()),
                    },
                    ExternalWorkRecord {
                        kind: "issue".to_string(),
                        id: "owner/repo#2".to_string(),
                        title: "New issue".to_string(),
                        body: None,
                        url: Some("https://github.com/owner/repo/issues/2".to_string()),
                        locator: None,
                        labels: vec!["docs".to_string()],
                        metadata: Default::default(),
                        source_hash: Some("sha256:2".to_string()),
                    },
                ],
                existing_refs: existing,
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: vec!["docs".to_string()],
                verification_command: vec!["make".to_string(), "check".to_string()],
                limit: 10,
            })
            .expect("drafts");

        assert_eq!(drafts.len(), 2);
        assert!(drafts[0].skipped);
        assert!(!drafts[1].skipped);
        assert_eq!(drafts[1].item_type, "docs");
    }
}
