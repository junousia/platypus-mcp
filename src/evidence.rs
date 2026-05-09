use crate::{
    models::{
        ActionResult, ActionStatus, EvidenceListData, EvidenceRecord, EvidenceRecordData,
        ListEvidenceParams, RecordEvidenceParams, RecordVerificationEvidenceParams,
    },
    state::{sqlite::SqliteProjectState, EvidenceQuery, ProjectState, RecordEvidenceCommand},
};
use std::path::Path;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const VALID_KINDS: &[&str] = &[
    "commit",
    "verification",
    "file_summary",
    "worker_finding",
    "manager_disposition",
    "external_report",
    "note",
];

pub fn record_evidence(
    default_root: &Path,
    params: RecordEvidenceParams,
) -> ActionResult<EvidenceRecordData> {
    let action = "record_evidence";
    let kind = params.kind.trim();
    let summary = params.summary.trim();
    if !VALID_KINDS.contains(&kind) {
        return ActionResult::failed(
            action,
            "Could not record evidence.",
            "invalid evidence kind",
        );
    }
    if summary.is_empty() {
        return ActionResult::failed(action, "Could not record evidence.", "summary is required");
    }
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open evidence storage.",
                error.to_string(),
            )
        }
    };
    match state.record_evidence(RecordEvidenceCommand {
        id: params.id,
        source_item_id: params.source_item_id,
        source_task_id: params.source_task_id,
        kind: kind.to_string(),
        summary: summary.to_string(),
        refs: params.refs,
        metadata: params.metadata,
    }) {
        Ok(evidence) => {
            let evidence = evidence_record(evidence);
            ActionResult::completed(
                action,
                format!("Recorded evidence `{}`.", evidence.id),
                EvidenceRecordData { evidence },
            )
        }
        Err(error) => ActionResult::failed(action, "Could not record evidence.", error.to_string()),
    }
}

pub fn record_verification_evidence(
    default_root: &Path,
    params: RecordVerificationEvidenceParams,
) -> ActionResult<EvidenceRecordData> {
    let mut result = record_evidence(
        default_root,
        RecordEvidenceParams {
            root: params.root,
            id: params.id,
            source_item_id: params.source_item_id,
            source_task_id: Some(params.source_task_id),
            kind: "verification".to_string(),
            summary: params.summary,
            refs: params.refs,
            metadata: params.metadata,
        },
    );
    result.action = "record_verification_evidence".to_string();
    if matches!(result.status, ActionStatus::Completed) {
        if let Some(evidence) = result.data.as_ref() {
            result.summary = format!("Recorded verification evidence `{}`.", evidence.evidence.id);
        }
    }
    result
}

pub fn list_evidence(
    default_root: &Path,
    params: ListEvidenceParams,
) -> ActionResult<EvidenceListData> {
    let action = "list_evidence";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open evidence storage.",
                error.to_string(),
            )
        }
    };
    let limit = bounded_limit(params.limit);
    let evidence: Vec<EvidenceRecord> = match state.list_evidence(EvidenceQuery {
        source_item_id: params.source_item_id,
        source_task_id: params.source_task_id,
        kind: params.kind,
        limit: Some(limit),
    }) {
        Ok(evidence) => evidence.evidence.into_iter().map(evidence_record).collect(),
        Err(error) => {
            return ActionResult::failed(action, "Could not list evidence.", error.to_string())
        }
    };
    let returned = evidence.len();
    let data = EvidenceListData {
        root: state.root().display().to_string(),
        evidence,
        returned,
    };
    if returned == 0 {
        ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No evidence matched the filters.".to_string(),
            next_action: Some("Record evidence before listing it.".to_string()),
            data: Some(data),
            error: None,
        }
    } else {
        ActionResult::completed(
            action,
            format!("Returned {returned} evidence record(s)."),
            data,
        )
    }
}

fn evidence_record(evidence: crate::state::EvidenceSnapshot) -> EvidenceRecord {
    EvidenceRecord {
        id: evidence.id,
        source_item_id: evidence.source_item_id,
        source_task_id: evidence.source_task_id,
        kind: evidence.kind,
        summary: evidence.summary,
        refs: evidence.refs,
        metadata: evidence.metadata,
        created_at: evidence.created_at,
    }
}

fn bounded_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    #[test]
    fn records_and_lists_evidence() {
        let project = TempDir::new().expect("temp dir");
        let recorded = record_evidence(
            project.path(),
            RecordEvidenceParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: Some("task-1".to_string()),
                kind: "verification".to_string(),
                summary: "make check passed".to_string(),
                refs: vec!["log:1".to_string()],
                metadata: BTreeMap::new(),
            },
        );
        let evidence = recorded.data.expect("evidence").evidence;

        assert!(matches!(recorded.status, ActionStatus::Completed));
        assert_eq!(evidence.id, "EVD-001");
        assert_eq!(evidence.refs, vec!["log:1"]);

        let listed = list_evidence(
            project.path(),
            ListEvidenceParams {
                root: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: None,
                kind: Some("verification".to_string()),
                limit: None,
            },
        );
        let data = listed.data.expect("list data");
        assert_eq!(data.returned, 1);
    }

    #[test]
    fn rejects_unknown_evidence_kind() {
        let project = TempDir::new().expect("temp dir");
        let result = record_evidence(
            project.path(),
            RecordEvidenceParams {
                root: None,
                id: None,
                source_item_id: None,
                source_task_id: None,
                kind: "unknown".to_string(),
                summary: "Nope".to_string(),
                refs: Vec::new(),
                metadata: BTreeMap::new(),
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
    }
}
