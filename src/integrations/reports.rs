use super::{bounded_limit, clean_optional, clean_required, clean_token};
use crate::{
    approvals::{self, NewApproval},
    backlog,
    events::{self, NewEvent},
    evidence,
    models::{
        ActionResult, ActionStatus, DraftExternalReportParams, EvidenceRecord, ExternalRef,
        ExternalReportApprovalData, ExternalReportDispatchData, ExternalReportDraft,
        ExternalReportDraftData, RecordEvidenceParams, RecordExternalReportDispatchParams,
        RequestExternalReportApprovalParams,
    },
    storage::{self, ApprovalStore, RepositoryError},
    tasks,
};
use std::{collections::BTreeMap, path::Path};

pub fn draft_external_report(
    default_root: &Path,
    params: DraftExternalReportParams,
) -> ActionResult<ExternalReportDraftData> {
    let action = "draft_external_report";
    let source_item_id = match clean_required("source_item_id", &params.source_item_id) {
        Ok(value) => value,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not draft external report.",
                error.to_string(),
            )
        }
    };
    let report_type = match clean_report_type(params.report_type.as_deref().unwrap_or("comment")) {
        Ok(value) => value,
        Err(error) => {
            return ActionResult::failed(action, "Could not draft external report.", error)
        }
    };
    let (root, item) =
        match backlog::backlog_item_snapshot(default_root, params.root.as_deref(), &source_item_id)
        {
            Ok(value) => value,
            Err(error) => {
                return ActionResult::failed(action, "Could not inspect backlog item.", error)
            }
        };
    let external_ref = match select_external_ref(
        &item.external_refs,
        params.provider.as_deref(),
        params.kind.as_deref(),
        params.external_id.as_deref(),
    ) {
        Ok(Some(reference)) => reference,
        Ok(None) => {
            return ActionResult::skipped(
                action,
                format!("Backlog item `{source_item_id}` has no matching external reference."),
                "Import external work first or pass a matching provider/kind/external_id.",
            )
        }
        Err(error) => return ActionResult::failed(action, "Could not select external ref.", error),
    };
    let task = match params.source_task_id.as_deref().map(str::trim) {
        Some(task_id) if !task_id.is_empty() => {
            match tasks::get_task_by_id(default_root, params.root.as_deref(), task_id) {
                Ok(task) => Some(task),
                Err(error) => {
                    return ActionResult::skipped(
                        action,
                        format!("Task `{task_id}` was not found."),
                        &error,
                    )
                }
            }
        }
        _ => None,
    };
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not inspect evidence storage.",
                error.to_string(),
            )
        }
    };
    let evidence_limit = bounded_limit(params.evidence_limit);
    let evidence = match evidence::query_evidence(
        &storage.connection,
        Some(&source_item_id),
        params.source_task_id.as_deref(),
        None,
        evidence_limit,
    ) {
        Ok(evidence) => evidence,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect evidence.", error.to_string())
        }
    };
    let evidence_refs = evidence
        .iter()
        .map(|record| format!("evidence:{}", record.id))
        .collect::<Vec<_>>();
    let title = params
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| default_report_title(&report_type, &item.title));
    let body = report_body(&item, task.as_ref(), &evidence, &report_type);
    let report_key = report_key(
        &external_ref.provider,
        &external_ref.kind,
        &external_ref.id,
        &report_type,
        &source_item_id,
    );
    let draft = ExternalReportDraft {
        report_key,
        provider: external_ref.provider.clone(),
        kind: external_ref.kind.clone(),
        external_id: external_ref.id.clone(),
        external_url: external_ref.url.clone().or(external_ref.locator.clone()),
        report_type,
        source_item_id: item.id,
        source_task_id: params
            .source_task_id
            .and_then(|value| clean_optional(Some(value))),
        title,
        body,
        local_refs: local_refs(&source_item_id, task.as_ref()),
        evidence_refs,
        safety_notes: vec![
            "Draft only; no external provider was called.".to_string(),
            "External mutation requires explicit approval.".to_string(),
            "Provider credentials must stay in the MCP host or approved plugin.".to_string(),
        ],
        requires_approval: true,
    };
    ActionResult::completed(
        action,
        format!("Drafted external {} report.", draft.report_type),
        ExternalReportDraftData {
            root: root.display().to_string(),
            draft,
            evidence,
            task,
        },
    )
}

pub fn request_external_report_approval(
    default_root: &Path,
    params: RequestExternalReportApprovalParams,
) -> ActionResult<ExternalReportApprovalData> {
    let action = "request_external_report_approval";
    if let Err(error) = validate_report_draft(&params.draft) {
        return ActionResult::failed(action, "Could not request report approval.", error);
    }
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "report_key".to_string(),
        serde_json::json!(params.draft.report_key),
    );
    metadata.insert(
        "provider".to_string(),
        serde_json::json!(params.draft.provider),
    );
    metadata.insert("kind".to_string(), serde_json::json!(params.draft.kind));
    metadata.insert(
        "external_id".to_string(),
        serde_json::json!(params.draft.external_id),
    );
    metadata.insert(
        "report_type".to_string(),
        serde_json::json!(params.draft.report_type),
    );
    metadata.insert(
        "source_item_id".to_string(),
        serde_json::json!(params.draft.source_item_id),
    );
    metadata.insert(
        "source_task_id".to_string(),
        serde_json::json!(params.draft.source_task_id),
    );
    let approval = match approvals::create_approval(
        default_root,
        params.root.as_deref(),
        NewApproval {
            scope: "external_report".to_string(),
            title: format!("Send external report: {}", params.draft.title),
            summary: format!(
                "{} {} report for {} `{}`.",
                params.draft.provider,
                params.draft.report_type,
                params.draft.kind,
                params.draft.external_id
            ),
            requested_by: params.requested_by,
            metadata,
        },
    ) {
        Ok(approval) => approval,
        Err(error) => return ActionResult::failed(action, "Could not create approval.", error),
    };
    let root = match backlog::resolve_backlog_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(action, "Could not resolve project root.", error)
        }
    };
    let report_key = approval
        .metadata
        .get("report_key")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    ActionResult::completed(
        action,
        format!("Approval `{}` requested for external report.", approval.id),
        ExternalReportApprovalData {
            root: root.display().to_string(),
            approval,
            report_key,
        },
    )
}

pub fn record_external_report_dispatch(
    default_root: &Path,
    params: RecordExternalReportDispatchParams,
) -> ActionResult<ExternalReportDispatchData> {
    let action = "record_external_report_dispatch";
    let approval_id = params.approval_id.trim();
    if approval_id.is_empty() {
        return ActionResult::failed(
            action,
            "Could not record external report dispatch.",
            "approval_id is required",
        );
    }
    let provider = match clean_token("provider", &params.provider) {
        Ok(value) => value,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not record external report dispatch.",
                error.to_string(),
            )
        }
    };
    let kind = match clean_token("kind", &params.kind) {
        Ok(value) => value,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not record external report dispatch.",
                error.to_string(),
            )
        }
    };
    let external_id = match clean_required("external_id", &params.external_id) {
        Ok(value) => value,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not record external report dispatch.",
                error.to_string(),
            )
        }
    };
    let report_type = match clean_report_type(&params.report_type) {
        Ok(value) => value,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not record external report dispatch.",
                error.to_string(),
            )
        }
    };
    let dispatch_status = match clean_dispatch_status(&params.status) {
        Ok(value) => value,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not record external report dispatch.",
                error,
            )
        }
    };
    let summary = match clean_required("summary", &params.summary) {
        Ok(value) => value,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not record external report dispatch.",
                error.to_string(),
            )
        }
    };
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open approval storage.",
                error.to_string(),
            )
        }
    };
    let approvals = storage.repository().approvals();
    let approval = match approvals.get(approval_id) {
        Ok(approval) => approval,
        Err(RepositoryError::NotFound) => {
            return ActionResult::skipped(
                action,
                format!("Approval `{approval_id}` was not found."),
                "Request and approve an external report approval before recording dispatch.",
            )
        }
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect approval.", error.to_string())
        }
    };
    if approval.scope != "external_report" {
        return ActionResult::failed(
            action,
            "Could not record external report dispatch.",
            "approval scope is not external_report",
        );
    }
    if approval.status == "pending" {
        return ActionResult::skipped(
            action,
            format!("Approval `{approval_id}` is still pending."),
            "Approve the external report approval before recording dispatch.",
        );
    }
    if approval.status == "denied" {
        return ActionResult::skipped(
            action,
            format!("Approval `{approval_id}` was denied."),
            "Create a new approval if the report should be sent later.",
        );
    }
    if approval.status != "approved" {
        return ActionResult::failed(
            action,
            "Could not record external report dispatch.",
            "approval status must be approved",
        );
    }
    if !approval_matches_report(&approval, &provider, &kind, &external_id, &report_type) {
        return ActionResult::failed(
            action,
            "Could not record external report dispatch.",
            "approval metadata does not match the dispatch report target",
        );
    }
    let report_key = metadata_str(&approval.metadata, "report_key")
        .map(ToString::to_string)
        .unwrap_or_else(|| report_key(&provider, &kind, &external_id, &report_type, ""));
    let redacted_metadata = redact_metadata(params.metadata);
    let event = match events::record_event(
        default_root,
        params.root.as_deref(),
        NewEvent {
            event_type: "external_report_dispatch".to_string(),
            scope: "external_report".to_string(),
            task_id: None,
            summary: format!(
                "External report `{approval_id}` dispatch recorded as {dispatch_status}."
            ),
            payload: Some(serde_json::json!({
                "approval_id": approval.id,
                "provider": provider,
                "kind": kind,
                "external_id": external_id,
                "report_type": report_type,
                "status": dispatch_status,
                "outbound_ref": params.outbound_ref,
                "error": params.error,
                "metadata": redacted_metadata
            })),
        },
    ) {
        Ok(event) => event,
        Err(error) => {
            return ActionResult::failed(action, "Could not record dispatch event.", error)
        }
    };
    let evidence = match evidence::record_evidence(
        default_root,
        RecordEvidenceParams {
            root: params.root.clone(),
            id: None,
            source_item_id: approval
                .metadata
                .get("source_item_id")
                .and_then(|value| value.as_str())
                .map(ToString::to_string),
            source_task_id: approval
                .metadata
                .get("source_task_id")
                .and_then(|value| value.as_str())
                .map(ToString::to_string),
            kind: "external_report".to_string(),
            summary: summary.clone(),
            refs: clean_optional(params.outbound_ref.clone())
                .map(|value| vec![value])
                .unwrap_or_default(),
            metadata: redacted_metadata.clone(),
        },
    ) {
        ActionResult {
            status: ActionStatus::Completed,
            data: Some(data),
            ..
        } => data.evidence,
        ActionResult { error, summary, .. } => {
            return ActionResult::failed(
                action,
                "Could not record dispatch evidence.",
                error.unwrap_or(summary),
            )
        }
    };
    ActionResult::completed(
        action,
        format!("External report dispatch recorded as {dispatch_status}."),
        ExternalReportDispatchData {
            root: storage.storage.root.display().to_string(),
            approval,
            report_key,
            status: dispatch_status,
            outbound_ref: params.outbound_ref,
            event,
            evidence,
        },
    )
}

fn select_external_ref(
    refs: &[ExternalRef],
    provider: Option<&str>,
    kind: Option<&str>,
    external_id: Option<&str>,
) -> Result<Option<ExternalRef>, String> {
    let provider = provider
        .map(|value| clean_token("provider", value))
        .transpose()
        .map_err(|error| error.to_string())?;
    let kind = kind
        .map(|value| clean_token("kind", value))
        .transpose()
        .map_err(|error| error.to_string())?;
    let external_id = external_id
        .map(|value| clean_required("external_id", value))
        .transpose()
        .map_err(|error| error.to_string())?;
    Ok(refs
        .iter()
        .find(|reference| {
            provider
                .as_deref()
                .is_none_or(|value| reference.provider.eq_ignore_ascii_case(value))
                && kind
                    .as_deref()
                    .is_none_or(|value| reference.kind.eq_ignore_ascii_case(value))
                && external_id
                    .as_deref()
                    .is_none_or(|value| reference.id == value)
        })
        .cloned())
}

fn clean_report_type(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "comment" | "status" | "finding" | "closure" => Ok(value),
        _ => Err("report_type must be comment, status, finding, or closure".to_string()),
    }
}

fn clean_dispatch_status(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "sent" | "failed" | "skipped" | "retryable" => Ok(value),
        _ => Err("status must be sent, failed, skipped, or retryable".to_string()),
    }
}

fn default_report_title(report_type: &str, item_title: &str) -> String {
    match report_type {
        "status" => format!("Status update: {item_title}"),
        "finding" => format!("Follow-up finding: {item_title}"),
        "closure" => format!("Closure update: {item_title}"),
        _ => format!("Progress update: {item_title}"),
    }
}

fn report_body(
    item: &backlog::BacklogItemSnapshot,
    task: Option<&crate::models::TaskRecord>,
    evidence: &[EvidenceRecord],
    report_type: &str,
) -> String {
    let mut body = vec![
        format!("## {}", default_report_title(report_type, &item.title)),
        String::new(),
        format!("- Backlog item: `{}`", item.id),
    ];
    if let Some(task) = task {
        body.push(format!("- Task: `{}` ({})", task.id, task.status));
    }
    if evidence.is_empty() {
        body.push("- Evidence: none recorded yet".to_string());
    } else {
        body.push("- Evidence:".to_string());
        for record in evidence.iter().take(10) {
            body.push(format!(
                "  - `{}` {}: {}",
                record.id, record.kind, record.summary
            ));
        }
    }
    body.push(String::new());
    body.push(
        "This report was drafted from local Platypus state. Review before sending externally."
            .to_string(),
    );
    body.join("\n")
}

fn report_key(
    provider: &str,
    kind: &str,
    external_id: &str,
    report_type: &str,
    source_item_id: &str,
) -> String {
    if source_item_id.is_empty() {
        format!("{provider}:{kind}:{external_id}:{report_type}")
    } else {
        format!("{provider}:{kind}:{external_id}:{report_type}:{source_item_id}")
    }
}

fn local_refs(source_item_id: &str, task: Option<&crate::models::TaskRecord>) -> Vec<String> {
    let mut refs = vec![format!("backlog:{source_item_id}")];
    if let Some(task) = task {
        refs.push(format!("task:{}", task.id));
    }
    refs
}

fn validate_report_draft(draft: &ExternalReportDraft) -> Result<(), String> {
    clean_required("report_key", &draft.report_key).map_err(|error| error.to_string())?;
    clean_token("provider", &draft.provider).map_err(|error| error.to_string())?;
    clean_token("kind", &draft.kind).map_err(|error| error.to_string())?;
    clean_required("external_id", &draft.external_id).map_err(|error| error.to_string())?;
    clean_report_type(&draft.report_type)?;
    clean_required("source_item_id", &draft.source_item_id).map_err(|error| error.to_string())?;
    clean_required("title", &draft.title).map_err(|error| error.to_string())?;
    clean_required("body", &draft.body).map_err(|error| error.to_string())?;
    Ok(())
}

fn approval_matches_report(
    approval: &crate::models::ApprovalRecord,
    provider: &str,
    kind: &str,
    external_id: &str,
    report_type: &str,
) -> bool {
    metadata_str(&approval.metadata, "provider") == Some(provider)
        && metadata_str(&approval.metadata, "kind") == Some(kind)
        && metadata_str(&approval.metadata, "external_id") == Some(external_id)
        && metadata_str(&approval.metadata, "report_type") == Some(report_type)
}

fn metadata_str<'a>(
    metadata: &'a BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    metadata.get(key).and_then(|value| value.as_str())
}

fn redact_metadata(
    metadata: BTreeMap<String, serde_json::Value>,
) -> BTreeMap<String, serde_json::Value> {
    metadata
        .into_iter()
        .map(|(key, value)| {
            let lower = key.to_ascii_lowercase();
            if lower.contains("token")
                || lower.contains("secret")
                || lower.contains("password")
                || lower.contains("cookie")
                || lower.contains("authorization")
            {
                (key, serde_json::json!("[redacted]"))
            } else {
                (key, value)
            }
        })
        .collect()
}
