use crate::{
    models::{
        ActionResult, ActionStatus, FindingDispositionData, FindingListData, FindingRecord,
        FindingRecordData, FindingValidationData, ListFindingsParams, RecordFindingParams,
        UpdateFindingDispositionParams, ValidateFindingsParams,
    },
    storage,
};
use rusqlite::{params, OptionalExtension, Row};
use serde::de::DeserializeOwned;
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

    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open finding storage.",
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
        None => match next_finding_id(
            &storage.connection,
            params.source_item_id.as_deref(),
            params.source_task_id.as_deref(),
        ) {
            Ok(id) => id,
            Err(error) => {
                return ActionResult::failed(
                    action,
                    "Could not allocate finding id.",
                    error.to_string(),
                );
            }
        },
    };

    match finding_exists(&storage.connection, &id) {
        Ok(true) => {
            return ActionResult::failed(
                action,
                "Could not record finding.",
                format!("finding `{id}` already exists"),
            );
        }
        Ok(false) => {}
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not inspect finding storage.",
                error.to_string(),
            );
        }
    }

    let evidence_json = json_string(&params.evidence_refs);
    let metadata_json = json_string(&params.metadata);
    let required = params.required.unwrap_or(true);
    let inserted = storage.connection.execute(
        r#"
        INSERT INTO findings(
            id,
            source_item_id,
            source_task_id,
            source_finding_ref,
            title,
            status,
            severity,
            required,
            summary,
            evidence_json,
            metadata_json
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7, ?8, ?9, ?10)
        "#,
        params![
            id,
            clean_optional(params.source_item_id),
            clean_optional(params.source_task_id),
            clean_optional(params.source_finding_ref),
            title,
            clean_optional(params.severity),
            bool_to_i64(required),
            summary,
            evidence_json,
            metadata_json
        ],
    );

    if let Err(error) = inserted {
        return ActionResult::failed(action, "Could not record finding.", error.to_string());
    }

    match get_finding(&storage.connection, &id) {
        Ok(finding) => ActionResult::completed(
            action,
            format!("Recorded finding `{}`.", finding.id),
            FindingRecordData { finding },
        ),
        Err(error) => ActionResult::failed(
            action,
            "Could not load recorded finding.",
            error.to_string(),
        ),
    }
}

pub fn list_findings(
    default_root: &Path,
    params: ListFindingsParams,
) -> ActionResult<FindingListData> {
    let action = "list_findings";
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open finding storage.",
                error.to_string(),
            )
        }
    };
    let limit = bounded_limit(params.limit);

    match query_findings(
        &storage.connection,
        params.source_item_id.as_deref(),
        params.source_task_id.as_deref(),
        params.status.as_deref(),
        limit,
    ) {
        Ok(findings) => {
            let returned = findings.len();
            ActionResult::completed(
                action,
                format!("Returned {returned} finding(s)."),
                FindingListData {
                    root: storage.storage.root.display().to_string(),
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
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open finding storage.",
                error.to_string(),
            )
        }
    };

    match query_unresolved_required_findings(
        &storage.connection,
        params.source_item_id.as_deref(),
        params.source_task_id.as_deref(),
    ) {
        Ok(unresolved_required) => {
            let count = unresolved_required.len();
            let data = FindingValidationData {
                root: storage.storage.root.display().to_string(),
                ok: count == 0,
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
                        "Resolve, reject, or mark each required finding as duplicate.".to_string(),
                    ),
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

    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open finding storage.",
                error.to_string(),
            )
        }
    };

    match finding_exists(&storage.connection, &params.finding_id) {
        Ok(true) => {}
        Ok(false) => {
            return ActionResult::failed(
                action,
                "Could not update finding disposition.",
                format!("finding `{}` does not exist", params.finding_id),
            );
        }
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not inspect finding storage.",
                error.to_string(),
            );
        }
    }

    let evidence_json = json_string(&params.evidence_refs);
    let metadata_json = json_string(&params.metadata);
    if let Err(error) = storage.connection.execute(
        r#"
        UPDATE findings
        SET status = ?2,
            owner = ?3,
            disposition_reason = ?4,
            evidence_json = ?5,
            metadata_json = ?6,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
        params![
            params.finding_id,
            status,
            clean_optional(params.owner),
            clean_optional(params.disposition_reason),
            evidence_json,
            metadata_json
        ],
    ) {
        return ActionResult::failed(
            action,
            "Could not update finding disposition.",
            error.to_string(),
        );
    }

    match get_finding(&storage.connection, &params.finding_id) {
        Ok(finding) => ActionResult::completed(
            action,
            format!("Updated finding `{}` to `{}`.", finding.id, finding.status),
            FindingDispositionData { finding },
        ),
        Err(error) => {
            ActionResult::failed(action, "Could not load updated finding.", error.to_string())
        }
    }
}

fn query_findings(
    connection: &rusqlite::Connection,
    source_item_id: Option<&str>,
    source_task_id: Option<&str>,
    status: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<FindingRecord>> {
    let mut statement = connection.prepare(
        r#"
        SELECT *
        FROM findings
        WHERE (?1 IS NULL OR source_item_id = ?1)
          AND (?2 IS NULL OR source_task_id = ?2)
          AND (?3 IS NULL OR status = ?3)
        ORDER BY updated_at DESC, id DESC
        LIMIT ?4
        "#,
    )?;
    let rows = statement.query_map(
        params![source_item_id, source_task_id, status, limit],
        row_to_finding,
    )?;
    rows.collect()
}

fn query_unresolved_required_findings(
    connection: &rusqlite::Connection,
    source_item_id: Option<&str>,
    source_task_id: Option<&str>,
) -> rusqlite::Result<Vec<FindingRecord>> {
    let mut statement = connection.prepare(
        r#"
        SELECT *
        FROM findings
        WHERE required = 1
          AND status NOT IN ('resolved', 'rejected', 'duplicate')
          AND (?1 IS NULL OR source_item_id = ?1)
          AND (?2 IS NULL OR source_task_id = ?2)
        ORDER BY updated_at DESC, id DESC
        LIMIT 200
        "#,
    )?;
    let rows = statement.query_map(params![source_item_id, source_task_id], row_to_finding)?;
    rows.collect()
}

fn get_finding(connection: &rusqlite::Connection, id: &str) -> rusqlite::Result<FindingRecord> {
    connection.query_row("SELECT * FROM findings WHERE id = ?1", [id], row_to_finding)
}

fn finding_exists(connection: &rusqlite::Connection, id: &str) -> rusqlite::Result<bool> {
    connection
        .query_row("SELECT 1 FROM findings WHERE id = ?1", [id], |_| Ok(()))
        .optional()
        .map(|value| value.is_some())
}

fn next_finding_id(
    connection: &rusqlite::Connection,
    source_item_id: Option<&str>,
    source_task_id: Option<&str>,
) -> rusqlite::Result<String> {
    let scope_prefix = source_item_id.or(source_task_id).unwrap_or("FIND");
    let prefix = safe_id_prefix(scope_prefix);
    let existing: i64 = connection.query_row(
        r#"
        SELECT COUNT(*)
        FROM findings
        WHERE (?1 IS NULL OR source_item_id = ?1)
          AND (?2 IS NULL OR source_task_id = ?2)
        "#,
        params![source_item_id, source_task_id],
        |row| row.get(0),
    )?;
    Ok(format!("{prefix}-F{:03}", existing + 1))
}

fn row_to_finding(row: &Row<'_>) -> rusqlite::Result<FindingRecord> {
    let evidence_json: Option<String> = row.get("evidence_json")?;
    let metadata_json: Option<String> = row.get("metadata_json")?;
    let required: i64 = row.get("required")?;
    Ok(FindingRecord {
        id: row.get("id")?,
        source_item_id: row.get("source_item_id")?,
        source_task_id: row.get("source_task_id")?,
        source_finding_ref: row.get("source_finding_ref")?,
        title: row.get("title")?,
        status: row.get("status")?,
        severity: row.get("severity")?,
        required: required != 0,
        summary: row.get("summary")?,
        owner: row.get("owner")?,
        disposition_reason: row.get("disposition_reason")?,
        evidence_refs: parse_json(evidence_json),
        metadata: parse_json(metadata_json),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn bounded_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
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

fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn json_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn parse_json<T>(raw: Option<String>) -> T
where
    T: DeserializeOwned + Default,
{
    raw.and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn safe_id_prefix(value: &str) -> String {
    let prefix: String = value
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                Some(character.to_ascii_uppercase())
            } else if character == '-' || character == '_' {
                Some('-')
            } else {
                None
            }
        })
        .collect();
    let prefix = prefix.trim_matches('-');
    if prefix.is_empty() {
        "FIND".to_string()
    } else {
        prefix.to_string()
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
