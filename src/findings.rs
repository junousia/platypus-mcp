use crate::{
    models::{
        ActionResult, ActionStatus, FindingDispositionData, FindingListData, FindingRecord,
        FindingRecordData, FindingValidationData, ListFindingsParams, RecordFindingParams,
        UpdateFindingDispositionParams, ValidateFindingsParams,
    },
    state::{
        sqlite::SqliteProjectState, FindingsQuery, ProjectState, RecordFindingCommand,
        UpdateFindingDispositionCommand, ValidateFindingsQuery,
    },
};
use std::path::Path;

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;
const VALID_STATUSES: &[&str] = &[
    "open",
    "accepted",
    "resolved",
    "rejected",
    "deferred",
    "duplicate",
];
pub fn record_finding(
    default_root: &Path,
    params: RecordFindingParams,
) -> ActionResult<FindingRecordData> {
    let action = "record_finding";
    let title = params.title.trim();
    let summary = params.summary.trim();
    if title.is_empty() {
        return ActionResult::failed(action, "Could not record finding.", "title is required");
    }
    if summary.is_empty() {
        return ActionResult::failed(action, "Could not record finding.", "summary is required");
    }

    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open finding storage.",
                error.to_string(),
            )
        }
    };

    match state.record_finding(RecordFindingCommand {
        id: params.id,
        source_item_id: params.source_item_id,
        source_task_id: params.source_task_id,
        source_finding_ref: params.source_finding_ref,
        title: title.to_string(),
        summary: summary.to_string(),
        severity: params.severity,
        required: params.required,
        evidence_refs: params.evidence_refs,
        metadata: params.metadata,
    }) {
        Ok(finding) => {
            let finding = finding_record(finding);
            ActionResult::completed(
                action,
                format!("Recorded finding `{}`.", finding.id),
                FindingRecordData { finding },
            )
        }
        Err(error) => ActionResult::failed(action, "Could not record finding.", error.to_string()),
    }
}

pub fn list_findings(
    default_root: &Path,
    params: ListFindingsParams,
) -> ActionResult<FindingListData> {
    let action = "list_findings";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open finding storage.",
                error.to_string(),
            )
        }
    };
    let limit = bounded_limit(params.limit);

    match state.list_findings(FindingsQuery {
        source_item_id: params.source_item_id,
        source_task_id: params.source_task_id,
        status: params.status,
        limit: Some(limit),
    }) {
        Ok(snapshot) => {
            let findings = snapshot
                .findings
                .into_iter()
                .map(finding_record)
                .collect::<Vec<_>>();
            let returned = findings.len();
            ActionResult::completed(
                action,
                format!("Returned {returned} finding(s)."),
                FindingListData {
                    root: state.root().display().to_string(),
                    findings,
                    returned,
                },
            )
        }
        Err(error) => ActionResult::failed(action, "Could not list findings.", error.to_string()),
    }
}

pub fn validate_findings(
    default_root: &Path,
    params: ValidateFindingsParams,
) -> ActionResult<FindingValidationData> {
    let action = "validate_findings";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open finding storage.",
                error.to_string(),
            )
        }
    };

    match state.validate_findings(ValidateFindingsQuery {
        source_item_id: params.source_item_id.clone(),
        source_task_id: params.source_task_id.clone(),
    }) {
        Ok(validation) => {
            let unresolved_required = validation
                .unresolved_required
                .into_iter()
                .map(finding_record)
                .collect::<Vec<_>>();
            let count = unresolved_required.len();
            let data = FindingValidationData {
                root: state.root().display().to_string(),
                ok: validation.ok,
                unresolved_required_count: count,
                unresolved_required,
            };
            if data.ok {
                ActionResult::completed(action, "No unresolved required findings.", data)
            } else {
                ActionResult {
                    action: action.to_string(),
                    status: ActionStatus::Failed,
                    summary: format!("{count} required finding(s) are unresolved."),
                    next_action: Some(
                        "Accept, defer, resolve, reject, or mark each required finding as duplicate."
                            .to_string(),
                    ),
                    recovery_action: None,
                    data: Some(data),
                    error: None,
                }
            }
        }
        Err(error) => {
            ActionResult::failed(action, "Could not validate findings.", error.to_string())
        }
    }
}

pub fn update_finding_disposition(
    default_root: &Path,
    params: UpdateFindingDispositionParams,
) -> ActionResult<FindingDispositionData> {
    let action = "update_finding_disposition";
    let status = params.status.trim().to_ascii_lowercase();
    if !VALID_STATUSES.contains(&status.as_str()) {
        return ActionResult::failed(
            action,
            "Could not update finding disposition.",
            format!("invalid status `{}`", params.status),
        );
    }

    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open finding storage.",
                error.to_string(),
            )
        }
    };

    match state.update_finding_disposition(UpdateFindingDispositionCommand {
        finding_id: params.finding_id,
        status,
        owner: params.owner,
        disposition_reason: params.disposition_reason,
        evidence_refs: params.evidence_refs,
        metadata: params.metadata,
    }) {
        Ok(finding) => {
            let finding = finding_record(finding);
            ActionResult::completed(
                action,
                format!("Updated finding `{}` to `{}`.", finding.id, finding.status),
                FindingDispositionData { finding },
            )
        }
        Err(error) => ActionResult::failed(
            action,
            "Could not update finding disposition.",
            error.to_string(),
        ),
    }
}

fn bounded_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn finding_record(finding: crate::state::FindingSnapshot) -> FindingRecord {
    FindingRecord {
        id: finding.id,
        source_item_id: finding.source_item_id,
        source_task_id: finding.source_task_id,
        source_finding_ref: finding.source_finding_ref,
        title: finding.title,
        status: finding.status,
        severity: Some(finding.severity),
        required: finding.required,
        summary: finding.summary,
        owner: finding.owner,
        disposition_reason: finding.disposition_reason,
        evidence_refs: finding.evidence_refs,
        metadata: finding.metadata,
        created_at: finding.created_at,
        updated_at: finding.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    #[test]
    fn records_lists_validates_and_dispositions_findings() {
        let project = TempDir::new().expect("temp dir");
        let recorded = record_finding(
            project.path(),
            RecordFindingParams {
                root: None,
                id: None,
                source_item_id: Some("MCP-002".to_string()),
                source_task_id: Some("task-1".to_string()),
                source_finding_ref: None,
                title: "Missing feedback loop".to_string(),
                summary: "Worker findings are not persisted.".to_string(),
                severity: Some("high".to_string()),
                required: Some(true),
                evidence_refs: vec!["tests".to_string()],
                metadata: BTreeMap::from([("kind".to_string(), Value::String("test".to_string()))]),
            },
        );
        let finding = recorded.data.expect("record data").finding;
        assert_eq!(finding.id, "MCP-002-F001");

        let listed = list_findings(
            project.path(),
            ListFindingsParams {
                root: None,
                source_item_id: Some("MCP-002".to_string()),
                source_task_id: None,
                status: Some("open".to_string()),
                limit: None,
            },
        );
        assert_eq!(listed.data.expect("list data").returned, 1);

        let invalid = validate_findings(
            project.path(),
            ValidateFindingsParams {
                root: None,
                source_item_id: Some("MCP-002".to_string()),
                source_task_id: None,
            },
        );
        assert!(matches!(invalid.status, ActionStatus::Failed));

        let updated = update_finding_disposition(
            project.path(),
            UpdateFindingDispositionParams {
                root: None,
                finding_id: finding.id.clone(),
                status: "resolved".to_string(),
                owner: Some("manager".to_string()),
                disposition_reason: Some("covered by MCP-002".to_string()),
                evidence_refs: vec!["commit".to_string()],
                metadata: BTreeMap::new(),
            },
        );
        assert_eq!(
            updated.data.expect("updated data").finding.status,
            "resolved"
        );

        let valid = validate_findings(
            project.path(),
            ValidateFindingsParams {
                root: None,
                source_item_id: Some("MCP-002".to_string()),
                source_task_id: None,
            },
        );
        assert!(matches!(valid.status, ActionStatus::Completed));
        assert!(valid.data.expect("valid data").ok);
    }

    #[test]
    fn rejects_duplicate_explicit_finding_id() {
        let project = TempDir::new().expect("temp dir");
        let params = || RecordFindingParams {
            root: None,
            id: Some("FIND-1".to_string()),
            source_item_id: None,
            source_task_id: None,
            source_finding_ref: None,
            title: "Duplicate".to_string(),
            summary: "Duplicate ids should fail.".to_string(),
            severity: None,
            required: None,
            evidence_refs: Vec::new(),
            metadata: BTreeMap::new(),
        };

        let first = record_finding(project.path(), params());
        let second = record_finding(project.path(), params());

        assert!(matches!(first.status, ActionStatus::Completed));
        assert!(matches!(second.status, ActionStatus::Failed));
    }

    #[test]
    fn validation_returns_unresolved_required_findings_not_first_page() {
        let project = TempDir::new().expect("temp dir");
        let open = record_finding(
            project.path(),
            RecordFindingParams {
                root: None,
                id: Some("AAA-OPEN".to_string()),
                source_item_id: Some("MCP-002".to_string()),
                source_task_id: None,
                source_finding_ref: None,
                title: "Required".to_string(),
                summary: "This must be returned.".to_string(),
                severity: None,
                required: Some(true),
                evidence_refs: Vec::new(),
                metadata: BTreeMap::new(),
            },
        );
        assert!(matches!(open.status, ActionStatus::Completed));

        for index in 0..205 {
            let finding = record_finding(
                project.path(),
                RecordFindingParams {
                    root: None,
                    id: Some(format!("ZZZ-RESOLVED-{index:03}")),
                    source_item_id: Some("MCP-002".to_string()),
                    source_task_id: None,
                    source_finding_ref: None,
                    title: format!("Resolved {index}"),
                    summary: "Already handled.".to_string(),
                    severity: None,
                    required: Some(false),
                    evidence_refs: Vec::new(),
                    metadata: BTreeMap::new(),
                },
            )
            .data
            .expect("finding")
            .finding;
            update_finding_disposition(
                project.path(),
                UpdateFindingDispositionParams {
                    root: None,
                    finding_id: finding.id,
                    status: "resolved".to_string(),
                    owner: None,
                    disposition_reason: Some("done".to_string()),
                    evidence_refs: Vec::new(),
                    metadata: BTreeMap::new(),
                },
            );
        }

        let validation = validate_findings(
            project.path(),
            ValidateFindingsParams {
                root: None,
                source_item_id: Some("MCP-002".to_string()),
                source_task_id: None,
            },
        );
        let data = validation.data.expect("validation");

        assert!(matches!(validation.status, ActionStatus::Failed));
        assert_eq!(data.unresolved_required_count, 1);
        assert_eq!(data.unresolved_required[0].id, "AAA-OPEN");
    }

    #[test]
    fn rejects_invalid_disposition_status() {
        let project = TempDir::new().expect("temp dir");
        let result = update_finding_disposition(
            project.path(),
            UpdateFindingDispositionParams {
                root: None,
                finding_id: "missing".to_string(),
                status: "unknown".to_string(),
                owner: None,
                disposition_reason: None,
                evidence_refs: Vec::new(),
                metadata: BTreeMap::new(),
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
    }
}
