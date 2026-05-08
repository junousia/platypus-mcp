use crate::{
    backlog::{self, create_backlog_item},
    models::{
        ActionResult, ActionStatus, CreateBacklogItemParams, DraftExternalBacklogItemsParams,
        ExternalBacklogDraft, ExternalBacklogDraftData, ExternalRef, ExternalWorkRecord,
        GitHubIssueImportData, GitHubIssueImportRecord, GitHubIssueRecord, GitHubIssueSkipRecord,
        ImportGitHubIssuesParams,
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
        let mut seen_refs = request.existing_refs;
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
            let skipped = !seen_refs.insert(key.clone());
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

pub fn import_github_issues(
    default_root: &Path,
    params: ImportGitHubIssuesParams,
) -> ActionResult<GitHubIssueImportData> {
    let action = "import_github_issues";
    let owner = match clean_token("owner", &params.owner) {
        Ok(owner) => owner,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not import GitHub issues.",
                error.to_string(),
            )
        }
    };
    let repo = match clean_token("repo", &params.repo) {
        Ok(repo) => repo,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not import GitHub issues.",
                error.to_string(),
            )
        }
    };
    let root = match backlog::resolve_backlog_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(action, "Could not import GitHub issues.", error)
        }
    };
    if params.issues.is_empty() {
        return ActionResult::skipped(
            action,
            "No GitHub issues were provided.",
            "Pass host-provided issue records or use a future live GitHub provider.",
        );
    }

    let state_filter = params
        .state
        .as_deref()
        .map(str::trim)
        .filter(|state| !state.is_empty())
        .unwrap_or("open")
        .to_ascii_lowercase();
    let limit = bounded_limit(params.limit);
    let mut skipped = Vec::new();
    let records = params
        .issues
        .into_iter()
        .filter_map(|issue| {
            let state = issue
                .state
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("open")
                .to_ascii_lowercase();
            let issue_id = issue_id(&owner, &repo, issue.number);
            if state != state_filter {
                skipped.push(GitHubIssueSkipRecord {
                    issue: issue_id,
                    reason: format!("state `{state}` did not match `{state_filter}`"),
                });
                None
            } else {
                Some(issue_to_external_record(&owner, &repo, issue))
            }
        })
        .take(limit)
        .collect::<Vec<_>>();

    let drafts = match draft_external_backlog_items(
        default_root,
        DraftExternalBacklogItemsParams {
            root: params.root.clone(),
            provider: "github".to_string(),
            records,
            suggested_worker: params.suggested_worker.clone(),
            owned_surfaces: params.owned_surfaces.clone(),
            verification_command: params.verification_command.clone(),
            limit: Some(limit),
        },
    ) {
        ActionResult {
            status: ActionStatus::Completed | ActionStatus::Skipped,
            data: Some(data),
            ..
        } => data.drafts,
        ActionResult { error, summary, .. } => {
            return ActionResult::failed(
                action,
                "Could not draft GitHub issue imports.",
                error.unwrap_or(summary),
            )
        }
    };

    let mut imported = Vec::new();
    for draft in drafts {
        if draft.skipped {
            skipped.push(GitHubIssueSkipRecord {
                issue: draft.external_ref.id,
                reason: draft.skip_reason.unwrap_or_else(|| "skipped".to_string()),
            });
            continue;
        }
        let created = create_backlog_item(
            default_root,
            CreateBacklogItemParams {
                root: params.root.clone(),
                id: None,
                id_prefix: params.id_prefix.clone().or_else(|| Some("GH".to_string())),
                title: draft.title.clone(),
                priority: Some(draft.priority.clone()),
                item_type: Some(draft.item_type.clone()),
                area: Some(draft.area.clone()),
                epic: Some("general".to_string()),
                depends_on: Vec::new(),
                suggested_worker: draft.suggested_worker.clone(),
                owned_surfaces: draft.owned_surfaces.clone(),
                external_refs: vec![draft.external_ref.clone()],
                goal: draft.objective.clone(),
                implementation_contract: Some(format!(
                    "Implement the local backlog snapshot imported from GitHub issue `{}`. Keep execution decisions in this repository.",
                    draft.external_ref.id
                )),
                contract: None,
                acceptance: vec![
                    "The imported issue is represented by a valid local backlog item.".to_string(),
                    "Implementation work remains scoped to the local backlog contract.".to_string(),
                ],
                notes: Some(
                    "Imported from GitHub issue data supplied by the MCP host. Live sync/reporting is not part of this snapshot."
                        .to_string(),
                ),
            },
        );
        match created {
            ActionResult {
                status: ActionStatus::Completed,
                data: Some(data),
                ..
            } => imported.push(GitHubIssueImportRecord {
                item_id: data.item_id,
                path: data.path,
                issue: draft.external_ref.id,
                url: draft
                    .external_ref
                    .url
                    .clone()
                    .or(draft.external_ref.locator.clone())
                    .unwrap_or_default(),
            }),
            ActionResult { error, summary, .. } => {
                return ActionResult::failed(
                    action,
                    "Could not create imported backlog item.",
                    error.unwrap_or(summary),
                )
            }
        }
    }

    let data = GitHubIssueImportData {
        root: root.display().to_string(),
        owner,
        repo,
        imported_count: imported.len(),
        skipped_count: skipped.len(),
        imported,
        skipped,
    };
    if data.imported_count == 0 {
        ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No GitHub issues were imported.".to_string(),
            next_action: Some("Inspect skipped issues or provide new issue records.".to_string()),
            data: Some(data),
            error: None,
        }
    } else {
        ActionResult::completed(
            action,
            format!("Imported {} GitHub issue(s).", data.imported_count),
            data,
        )
    }
}

fn issue_to_external_record(
    owner: &str,
    repo: &str,
    issue: GitHubIssueRecord,
) -> ExternalWorkRecord {
    let url = issue.url.clone().or_else(|| {
        Some(format!(
            "https://github.com/{owner}/{repo}/issues/{}",
            issue.number
        ))
    });
    let source_hash = issue
        .source_hash
        .clone()
        .or_else(|| Some(source_hash(&owner, &repo, &issue)));
    ExternalWorkRecord {
        kind: "issue".to_string(),
        id: issue_id(owner, repo, issue.number),
        title: issue.title,
        body: issue.body,
        url,
        locator: None,
        labels: issue.labels,
        metadata: issue.metadata,
        source_hash,
    }
}

fn issue_id(owner: &str, repo: &str, number: u64) -> String {
    format!("{owner}/{repo}#{number}")
}

fn source_hash(owner: &str, repo: &str, issue: &GitHubIssueRecord) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in format!(
        "{owner}\0{repo}\0{}\0{}\0{}\0{}\0{}",
        issue.number,
        issue.title,
        issue.body.as_deref().unwrap_or(""),
        issue.url.as_deref().unwrap_or(""),
        issue.labels.join(",")
    )
    .as_bytes()
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}

fn bounded_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn clean_token(field: &str, value: &str) -> Result<String, ExternalIntakeError> {
    let token = clean_required(field, value)?;
    if token.chars().all(|character| {
        character.is_ascii_alphanumeric()
            || character == '-'
            || character == '_'
            || character == '.'
    }) {
        Ok(token.to_ascii_lowercase())
    } else {
        Err(ExternalIntakeError::new(format!(
            "{field} must contain ASCII letters, digits, '.', '-' or '_'"
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
                    ExternalWorkRecord {
                        kind: "issue".to_string(),
                        id: "owner/repo#2".to_string(),
                        title: "Duplicate new issue".to_string(),
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

        assert_eq!(drafts.len(), 3);
        assert!(drafts[0].skipped);
        assert!(!drafts[1].skipped);
        assert_eq!(drafts[1].item_type, "docs");
        assert!(drafts[2].skipped);
    }
}
