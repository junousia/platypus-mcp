use crate::{
    models::{
        ActionResult, ActionStatus, EvidenceListData, EvidenceRecord, EvidenceRecordData,
        ListEvidenceParams, RecordEvidenceParams, RecordVerificationEvidenceParams,
    },
    storage,
};
use rusqlite::{params, Row};
use serde::de::DeserializeOwned;
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
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open evidence storage.",
                error.to_string(),
            )
        }
    };
    let id = match params
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => id.to_string(),
        None => match next_evidence_id(&storage.connection) {
            Ok(id) => id,
            Err(error) => {
                return ActionResult::failed(
                    action,
                    "Could not allocate evidence id.",
                    error.to_string(),
                )
            }
        },
    };
    let refs = clean_vec(params.refs);
    let refs_json = json_string(&refs);
    let metadata_json = json_string(&params.metadata);
    let inserted = storage.connection.execute(
        r#"
        INSERT INTO evidence(
            id, source_item_id, source_task_id, kind, summary, refs_json, metadata_json
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            id,
            clean_optional(params.source_item_id),
            clean_optional(params.source_task_id),
            kind,
            summary,
            refs_json,
            metadata_json
        ],
    );
    if let Err(error) = inserted {
        return ActionResult::failed(action, "Could not record evidence.", error.to_string());
    }
    match get_evidence(&storage.connection, &id) {
        Ok(evidence) => ActionResult::completed(
            action,
            format!("Recorded evidence `{}`.", evidence.id),
            EvidenceRecordData { evidence },
        ),
        Err(error) => ActionResult::failed(
            action,
            "Could not load recorded evidence.",
            error.to_string(),
        ),
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
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open evidence storage.",
                error.to_string(),
            )
        }
    };
    let limit = bounded_limit(params.limit);
    let evidence = match query_evidence(
        &storage.connection,
        params.source_item_id.as_deref(),
        params.source_task_id.as_deref(),
        params.kind.as_deref(),
        limit,
    ) {
        Ok(evidence) => evidence,
        Err(error) => {
            return ActionResult::failed(action, "Could not list evidence.", error.to_string())
        }
    };
    let returned = evidence.len();
    let data = EvidenceListData {
        root: storage.storage.root.display().to_string(),
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

pub(crate) fn query_evidence(
    connection: &rusqlite::Connection,
    source_item_id: Option<&str>,
    source_task_id: Option<&str>,
    kind: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<EvidenceRecord>> {
    let mut statement = connection.prepare(
        r#"
        SELECT id, source_item_id, source_task_id, kind, summary, refs_json, metadata_json, created_at
        FROM evidence
        WHERE (?1 IS NULL OR source_item_id = ?1)
          AND (?2 IS NULL OR source_task_id = ?2)
          AND (?3 IS NULL OR kind = ?3)
        ORDER BY created_at ASC, id ASC
        LIMIT ?4
        "#,
    )?;
    let rows = statement.query_map(
        params![source_item_id, source_task_id, kind, limit],
        row_to_evidence,
    )?;
    rows.collect()
}

fn get_evidence(connection: &rusqlite::Connection, id: &str) -> rusqlite::Result<EvidenceRecord> {
    connection.query_row(
        r#"
        SELECT id, source_item_id, source_task_id, kind, summary, refs_json, metadata_json, created_at
        FROM evidence
        WHERE id = ?1
        "#,
        [id],
        row_to_evidence,
    )
}

fn next_evidence_id(connection: &rusqlite::Connection) -> rusqlite::Result<String> {
    let existing: i64 =
        connection.query_row("SELECT COUNT(*) FROM evidence", [], |row| row.get(0))?;
    Ok(format!("EVD-{:03}", existing + 1))
}

fn row_to_evidence(row: &Row<'_>) -> rusqlite::Result<EvidenceRecord> {
    let refs_json: Option<String> = row.get("refs_json")?;
    let metadata_json: Option<String> = row.get("metadata_json")?;
    Ok(EvidenceRecord {
        id: row.get("id")?,
        source_item_id: row.get("source_item_id")?,
        source_task_id: row.get("source_task_id")?,
        kind: row.get("kind")?,
        summary: row.get("summary")?,
        refs: decode_json(refs_json).unwrap_or_default(),
        metadata: decode_json(metadata_json).unwrap_or_default(),
        created_at: row.get("created_at")?,
    })
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

fn clean_vec(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn json_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn decode_json<T: DeserializeOwned>(raw: Option<String>) -> Option<T> {
    raw.and_then(|raw| serde_json::from_str(&raw).ok())
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
