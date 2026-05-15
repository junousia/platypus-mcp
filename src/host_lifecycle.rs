use crate::{
    assignments, backlog, dispatch, events, evidence, execution_mode, findings, guidance,
    models::{
        ActionResult, ActionStatus, CompleteBacklogItemCompact, CompleteBacklogItemData,
        CompleteBacklogItemParams, CompleteWorkerExecutionParams, CompletionClosureState,
        CompletionCommitOutcome, DispatchReadyWorkData, DispatchReadyWorkItem,
        DispatchReadyWorkParams, EvidenceRecord, FinishWorkData, FinishWorkFindingInput,
        FinishWorkParams, GeneratedEvidenceSummary, HostAction, IntegrateWorkerResultParams,
        PrepareWorkData, PrepareWorkParams, QueueStatusData, ReconcileParams, RecordEvidenceParams,
        RecordFindingParams, RecordVerificationEvidenceParams, WorkQueueData, WorkQueueItem,
        WorkerAssignment, WorktreeDiffData, WorktreeDiffParams,
    },
    reconcile, workspace,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Component, Path},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const GIT_TIMEOUT: Duration = Duration::from_secs(30);

pub fn prepare_work(
    default_root: &Path,
    params: PrepareWorkParams,
) -> ActionResult<PrepareWorkData> {
    let action = "prepare_work";
    let max_tasks = params.max_tasks.unwrap_or(1).clamp(1, 10);
    let requested_execution_mode = params
        .execution_mode
        .as_deref()
        .unwrap_or(execution_mode::MANUAL_HANDOFF)
        .to_string();
    let manual_handoff = execution_mode::is_manual_handoff(&requested_execution_mode);
    let include_queue_snapshot = params.include_queue_snapshot.unwrap_or(false);
    let queue_result = guidance::inspect_work_queue(
        default_root,
        crate::models::InspectWorkQueueParams {
            root: params.root.clone(),
            limit: Some(if params.item_id.is_some() {
                200
            } else {
                max_tasks
            }),
            require_task_plan: params.require_task_plan,
            require_planning_approval: params.require_planning_approval,
        },
    );
    let mut queue = match queue_result {
        ActionResult {
            status: ActionStatus::Completed | ActionStatus::Skipped,
            data: Some(data),
            ..
        } => data,
        ActionResult { summary, error, .. } => {
            return ActionResult::failed(
                action,
                "Could not prepare executable work.",
                error.unwrap_or(summary),
            )
        }
    };
    if params.auto_commit_artifacts.unwrap_or(false) {
        queue
            .preflight_warnings
            .retain(|warning| !warning.contains("auto_commit_artifacts=true"));
    }
    let selected =
        selected_prepare_items(&queue, params.item_id.as_deref(), max_tasks, manual_handoff);
    if selected.is_empty() {
        let blocked_action =
            blocked_prepare_action(&queue, params.item_id.as_deref(), params.worker.clone());
        let selected_item_ids = blocked_action
            .item_id
            .as_ref()
            .map(|item_id| vec![item_id.clone()])
            .unwrap_or_default();
        let next_action = blocked_action.next_action.clone();
        let data = prepare_queue_only_data(
            queue,
            vec![blocked_action.host_action],
            include_queue_snapshot,
            "not_prepared",
            false,
            "none",
            "No preparation state was persisted because no ready backlog item could be selected.",
            selected_item_ids,
        );
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No ready backlog item could be prepared.".to_string(),
            next_action: Some(next_action),
            recovery_action: None,
            data: Some(data),
            error: None,
        };
    }

    let mut host_actions = selected
        .iter()
        .filter(|item| item.execution_path == crate::execution_policy::DIRECT_EDIT)
        .map(|item| {
            direct_host_action(
                item,
                queue.root.as_str(),
                params.worker.clone(),
                params.auto_commit_artifacts.unwrap_or(false),
            )
        })
        .collect::<Vec<_>>();
    let non_direct_ids = selected
        .iter()
        .filter(|item| item.execution_path == crate::execution_policy::WORKER_HANDOFF)
        .map(|item| item.item_id.clone())
        .collect::<Vec<_>>();

    if non_direct_ids.is_empty() {
        let prepared = host_actions.len();
        let selected_item_ids = selected.iter().map(|item| item.item_id.clone()).collect();
        let data = prepare_queue_only_data(
            queue,
            host_actions,
            include_queue_snapshot,
            "direct_guidance",
            false,
            "response_only",
            "Direct prepare is response-local guidance only. No task, assignment, event, or worktree was created; complete_backlog_item is the next durable transition.",
            selected_item_ids,
        );
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Completed,
            summary: format!("Prepared {prepared} direct host action(s)."),
            next_action: Some("Use the host's native edit tools in the manager workspace, then close the loop with complete_backlog_item. No worker process, assignment, or worktree was launched.".to_string()),
            recovery_action: None,
            data: Some(data),
            error: None,
        };
    }

    match dispatch_selected_work(
        default_root,
        &params,
        &requested_execution_mode,
        &non_direct_ids,
    ) {
        Ok(dispatch) => {
            host_actions.extend(
                dispatch
                    .items
                    .iter()
                    .filter_map(host_action_from_dispatch_item),
            );
            let status = if host_actions.is_empty() {
                ActionStatus::Skipped
            } else {
                ActionStatus::Completed
            };
            let prepared = host_actions.len();
            let contains_direct = host_actions
                .iter()
                .any(|action| action.kind == "direct_edit");
            let (prepared_state, persistence, persistence_summary) = if contains_direct {
                (
                    "mixed_prepared",
                    "mixed_response_and_durable",
                    "Mixed preparation returned response-local direct guidance and persisted worker handoff task and assignment lifecycle state.",
                )
            } else {
                (
                    "worktree_prepared",
                    "durable_task_lifecycle",
                    "Worker handoff preparation persisted task and assignment lifecycle state.",
                )
            };
            ActionResult {
                action: action.to_string(),
                status,
                summary: format!("Prepared {prepared} host action(s)."),
                next_action: Some(
                    "Run returned worktree assignments in the host or selected worker; finish with finish_work.".to_string(),
                ),
                recovery_action: None,
                data: Some(PrepareWorkData {
                    root: dispatch.root.clone(),
                    queue_summary: format!(
                        "Prepared {} host action(s) from {} selected item(s).",
                        host_actions.len(),
                        selected.len()
                    ),
                    prepared_state: prepared_state.to_string(),
                    state_persisted: true,
                    persistence: persistence.to_string(),
                    persistence_summary: persistence_summary.to_string(),
                    durable_next_tool: durable_next_tool_for_host_actions(&host_actions),
                    selected_item_ids: selected.iter().map(|item| item.item_id.clone()).collect(),
                    ready_count: queue.ready_count,
                    blocked_count: queue.blocked_count,
                    host_actions,
                    dispatch: Some(dispatch),
                    queue: None,
                }),
                error: None,
            }
        }
        Err(ActionResult {
            summary,
            next_action,
            data,
            error,
            ..
        }) => ActionResult {
            action: action.to_string(),
            status: ActionStatus::Failed,
            summary,
            next_action: next_action.clone(),
            recovery_action: next_action,
            data: data.map(|dispatch| PrepareWorkData {
                root: dispatch.root.clone(),
                queue_summary: "Dispatch failed while preparing host-run work.".to_string(),
                prepared_state: "not_prepared".to_string(),
                state_persisted: false,
                persistence: "none".to_string(),
                persistence_summary:
                    "No preparation state was persisted because worker handoff preparation failed."
                        .to_string(),
                durable_next_tool: None,
                selected_item_ids: selected.iter().map(|item| item.item_id.clone()).collect(),
                ready_count: queue.ready_count,
                blocked_count: queue.blocked_count,
                host_actions: vec![HostAction {
                    kind: "inspect_or_recover".to_string(),
                    summary: "Preparation failed before a host-run handoff was available."
                        .to_string(),
                    instructions: vec![
                        "Inspect the dispatch error and project state.".to_string(),
                        "Retry prepare_work after resolving the blocker.".to_string(),
                    ],
                    task_id: None,
                    assignment_id: None,
                    worker: params.worker,
                    worktree_path: None,
                    bundle: None,
                    next_tools: vec![
                        "doctor_snapshot".to_string(),
                        "inspect_work_queue".to_string(),
                    ],
                }],
                dispatch: Some(dispatch),
                queue: None,
            }),
            error,
        },
    }
}

fn ready_for_prepare_work(item: &WorkQueueItem, manual_handoff: bool) -> bool {
    if item.ready_to_dispatch {
        return true;
    }
    manual_handoff
        && item.recommended_tool == "prepare_work"
        && matches!(item.plan.status.as_str(), "valid" | "not_required")
        && item
            .planning_approval
            .as_ref()
            .is_none_or(|approval| !approval.required || approval.approved)
}

struct BlockedPrepareAction {
    item_id: Option<String>,
    next_action: String,
    host_action: HostAction,
}

fn blocked_prepare_action(
    queue: &WorkQueueData,
    requested_item_id: Option<&str>,
    worker: Option<String>,
) -> BlockedPrepareAction {
    let requested = requested_item_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let item = requested.and_then(|item_id| {
        queue
            .items
            .iter()
            .find(|item| item.candidate.item_id == item_id)
    });
    if let Some(item) = item {
        let mut next_tools = vec![item.recommended_tool.clone(), "inspect_item".to_string()];
        if item.queue_state == "planning_blocked" {
            next_tools.push("write_task_plan".to_string());
            next_tools.push("validate_task_plan".to_string());
        }
        if item.queue_state == "approval_blocked" {
            next_tools.push("request_planning_approval".to_string());
            next_tools.push("approval_respond".to_string());
        }
        if matches!(
            item.queue_state.as_str(),
            "config_blocked" | "workspace_blocked"
        ) {
            next_tools.push("doctor_snapshot".to_string());
        }
        next_tools.push("inspect_work_queue".to_string());
        next_tools.sort();
        next_tools.dedup();
        let reason = format!(
            "`{}` is `{}` and cannot be prepared yet: {}",
            item.candidate.item_id, item.queue_state, item.reason
        );
        return BlockedPrepareAction {
            item_id: Some(item.candidate.item_id.clone()),
            next_action: reason.clone(),
            host_action: HostAction {
                kind: "inspect_or_recover".to_string(),
                summary: format!(
                    "Requested item `{}` is blocked: {}.",
                    item.candidate.item_id, item.queue_state
                ),
                instructions: vec![
                    reason,
                    format!(
                        "Recommended next tool for this item is `{}`.",
                        item.recommended_tool
                    ),
                    "Retry prepare_work after the item reaches direct_ready or ready.".to_string(),
                ],
                task_id: item.task_id.clone(),
                assignment_id: item.assignment_id.clone(),
                worker,
                worktree_path: None,
                bundle: None,
                next_tools,
            },
        };
    }

    let next_action = if let Some(item_id) = requested {
        format!(
            "`{item_id}` was not returned by inspect_work_queue; call inspect_item or inspect_work_queue to inspect whether it is closed, dependency-blocked, filtered, or missing."
        )
    } else {
        "Inspect the queue item reasons and resolve planning, approval, Git, or backlog blockers before retrying prepare_work.".to_string()
    };
    BlockedPrepareAction {
        item_id: None,
        next_action: next_action.clone(),
        host_action: HostAction {
            kind: "inspect_or_recover".to_string(),
            summary: "No work is ready to prepare.".to_string(),
            instructions: vec![
                next_action,
                "Call inspect_work_queue after resolving the blocker.".to_string(),
            ],
            task_id: None,
            assignment_id: None,
            worker,
            worktree_path: None,
            bundle: None,
            next_tools: vec![
                "inspect_work_queue".to_string(),
                "doctor_snapshot".to_string(),
            ],
        },
    }
}

pub fn complete_backlog_item(
    default_root: &Path,
    params: CompleteBacklogItemParams,
) -> ActionResult<CompleteBacklogItemData> {
    let action = "complete_backlog_item";
    let item_id = match clean_item_id(&params.item_id) {
        Ok(item_id) => item_id,
        Err(error) => {
            return ActionResult::failed(action, "Could not complete backlog item.", error)
        }
    };
    let summary = params.summary.trim();
    if summary.is_empty() {
        return ActionResult::failed(
            action,
            "Could not complete backlog item.",
            "summary is required",
        );
    }
    let detail = match completion_detail(params.detail.as_deref()) {
        Ok(detail) => detail,
        Err(error) => {
            return ActionResult::failed(action, "Could not complete backlog item.", error)
        }
    };
    let inventory = backlog::inspect_backlog_inventory(default_root, params.root.as_deref(), None);
    let (root, already_closed) = match inventory {
        ActionResult {
            status: ActionStatus::Completed,
            data: Some(data),
            ..
        } => {
            let Some(item) = data.items.iter().find(|item| item.item_id == item_id) else {
                return ActionResult::failed(
                    action,
                    "Could not complete backlog item.",
                    format!("unknown backlog item `{item_id}`"),
                );
            };
            (data.root, item.closed)
        }
        ActionResult { summary, error, .. } => {
            return ActionResult::failed(
                action,
                "Could not inspect backlog before completion.",
                error.unwrap_or(summary),
            )
        }
    };
    if already_closed {
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: format!("Backlog item `{item_id}` is already closed."),
            next_action: Some(
                "Select the next safe action or inspect the backlog queue.".to_string(),
            ),
            recovery_action: None,
            data: {
                let (queue_status, queue_status_error) =
                    completion_queue_status(default_root, &root);
                let closure = already_closed_closure(Path::new(&root), &item_id);
                let commit_outcome =
                    completion_commit_outcome(params.commit.unwrap_or(false), None, true);
                let generated_evidence = Vec::new();
                let compact = completion_compact(
                    &item_id,
                    true,
                    &closure,
                    &commit_outcome,
                    None,
                    params.record_auto_evidence.unwrap_or(true),
                    &generated_evidence,
                    &[],
                    queue_status.as_ref(),
                    "Backlog item was already closed.",
                );
                let (queue_status, queue_status_error) =
                    verbose_queue_status(detail, queue_status, queue_status_error);
                Some(CompleteBacklogItemData {
                    root,
                    item_id,
                    summary: summary.to_string(),
                    detail: detail.to_string(),
                    compact,
                    warnings: Vec::new(),
                    changed_files: params.changed_files,
                    evidence: Vec::new(),
                    auto_evidence_enabled: params.record_auto_evidence.unwrap_or(true),
                    generated_evidence,
                    event: None,
                    commit: None,
                    closed: true,
                    closure,
                    commit_outcome,
                    queue_status,
                    queue_status_error,
                    host_action: HostAction {
                        kind: "done".to_string(),
                        summary: "Backlog item was already closed.".to_string(),
                        instructions: vec![
                            "inspect_work_queue can choose the next item.".to_string()
                        ],
                        task_id: None,
                        assignment_id: None,
                        worker: None,
                        worktree_path: None,
                        bundle: None,
                        next_tools: vec!["inspect_work_queue".to_string()],
                    },
                })
            },
            error: None,
        };
    }

    let changed_files = match validate_changed_files(&params.changed_files) {
        Ok(files) => files,
        Err(error) => {
            return ActionResult::failed(action, "Could not complete backlog item.", error)
        }
    };
    let root_path = Path::new(&root);
    let record_auto_evidence = params.record_auto_evidence.unwrap_or(true);
    if !record_auto_evidence {
        if let Err(error) = validate_explicit_completion_evidence(
            default_root,
            &root,
            &item_id,
            &params.evidence_refs,
        ) {
            return ActionResult::failed(action, "Could not complete backlog item.", error);
        }
    }
    let warnings = changed_file_warnings(
        root_path,
        &changed_files,
        empty_changed_files_explained(&params),
    );
    let mut evidence_records = Vec::new();
    if record_auto_evidence {
        let completion_evidence = evidence::record_evidence(
            default_root,
            RecordEvidenceParams {
                root: Some(root.clone()),
                id: None,
                source_item_id: Some(item_id.clone()),
                source_task_id: None,
                kind: "note".to_string(),
                summary: summary.to_string(),
                refs: completion_refs(&changed_files, &params.evidence_refs, &params.finding_refs),
                metadata: BTreeMap::from([
                    (
                        "completion_kind".to_string(),
                        Value::String("direct".to_string()),
                    ),
                    (
                        "verification_status".to_string(),
                        Value::String(
                            params
                                .verification_status
                                .clone()
                                .unwrap_or_else(|| "not_run".to_string()),
                        ),
                    ),
                ]),
            },
        );
        match completion_evidence {
            ActionResult {
                status: ActionStatus::Completed,
                data: Some(data),
                ..
            } => evidence_records.push(data.evidence),
            ActionResult { summary, error, .. } => {
                return ActionResult::failed(
                    action,
                    "Could not record direct completion evidence.",
                    error.unwrap_or(summary),
                )
            }
        }
    }

    if record_auto_evidence
        && (params.verification_summary.is_some()
            || !params.verification_refs.is_empty()
            || matches!(
                params.verification_status.as_deref(),
                Some("passed" | "failed" | "skipped")
            ))
    {
        let verification = evidence::record_verification_evidence(
            default_root,
            RecordVerificationEvidenceParams {
                root: Some(root.clone()),
                id: None,
                source_item_id: Some(item_id.clone()),
                source_task_id: None,
                summary: params.verification_summary.clone().unwrap_or_else(|| {
                    format!(
                        "Direct work verification status: {}.",
                        params
                            .verification_status
                            .as_deref()
                            .unwrap_or("not_recorded")
                    )
                }),
                refs: params.verification_refs.clone(),
                metadata: BTreeMap::new(),
            },
        );
        match verification {
            ActionResult {
                status: ActionStatus::Completed,
                data: Some(data),
                ..
            } => evidence_records.push(data.evidence),
            ActionResult { summary, error, .. } => {
                return ActionResult::failed(
                    action,
                    "Could not record direct verification evidence.",
                    error.unwrap_or(summary),
                )
            }
        }
    }

    let commit_requested = params.commit.unwrap_or(false);
    let commit = if commit_requested {
        match commit_direct_completion(
            root_path,
            &item_id,
            params.commit_message.as_deref(),
            summary,
            params.verification_summary.as_deref(),
            params.verification_status.as_deref(),
            &changed_files,
        ) {
            Ok(commit) => {
                if record_auto_evidence {
                    let commit_evidence = evidence::record_evidence(
                        default_root,
                        RecordEvidenceParams {
                            root: Some(root.clone()),
                            id: None,
                            source_item_id: Some(item_id.clone()),
                            source_task_id: None,
                            kind: "commit".to_string(),
                            summary: format!("Direct backlog item `{item_id}` closure commit."),
                            refs: vec![format!("commit:{commit}")],
                            metadata: BTreeMap::from([(
                                "completion_kind".to_string(),
                                Value::String("direct".to_string()),
                            )]),
                        },
                    );
                    match commit_evidence {
                        ActionResult {
                            status: ActionStatus::Completed,
                            data: Some(data),
                            ..
                        } => evidence_records.push(data.evidence),
                        ActionResult { summary, error, .. } => return ActionResult::failed(
                            action,
                            "Direct work was committed but commit evidence could not be recorded.",
                            error.unwrap_or(summary),
                        ),
                    }
                }
                Some(commit)
            }
            Err(error) => {
                return ActionResult::failed(
                    action,
                    "Could not create direct completion commit.",
                    error,
                )
            }
        }
    } else {
        None
    };

    let generated_evidence = evidence_records
        .iter()
        .map(|evidence| GeneratedEvidenceSummary {
            id: evidence.id.clone(),
            kind: evidence.kind.clone(),
            summary: evidence.summary.clone(),
        })
        .collect::<Vec<_>>();
    let generated_evidence_refs = generated_evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<Vec<_>>();
    let mut completion_evidence_refs = params.evidence_refs.clone();
    completion_evidence_refs.extend(generated_evidence_refs.clone());
    let commit_outcome = completion_commit_outcome(commit_requested, commit.clone(), false);

    let event = match events::record_event(
        default_root,
        Some(root.as_str()),
        events::NewEvent {
            event_type: "backlog_item_completed".to_string(),
            scope: "backlog".to_string(),
            task_id: None,
            summary: format!("Completed direct backlog item `{item_id}`."),
            payload: Some(serde_json::json!({
                "item_id": item_id,
                "summary": summary,
                "changed_files": changed_files,
                "evidence_refs": completion_evidence_refs,
                "generated_evidence_refs": generated_evidence_refs,
                "auto_evidence_enabled": record_auto_evidence,
                "finding_refs": params.finding_refs,
                "commit": commit,
                "completion_kind": "direct"
            })),
        },
    ) {
        Ok(event) => Some(event),
        Err(error) => return ActionResult::failed(
            action,
            "Direct completion evidence was recorded but completion event could not be recorded.",
            error,
        ),
    };

    let host_action = HostAction {
        kind: "done".to_string(),
        summary: format!("Direct backlog item `{item_id}` is complete."),
        instructions: vec![
            "The item is now hidden from runnable backlog queues by recorded completion state."
                .to_string(),
            "Use a Git commit with Platypus-Closes for repository-portable closure when needed."
                .to_string(),
            "Call inspect_work_queue to choose the next item. Run reconcile_project only when state is unclear, recovery guidance is needed, or you want an explicit audit pass."
                .to_string(),
        ],
        task_id: None,
        assignment_id: None,
        worker: None,
        worktree_path: Some(root.clone()),
        bundle: None,
        next_tools: vec![
            "inspect_work_queue".to_string(),
            "reconcile_project".to_string(),
        ],
    };
    let (queue_status, queue_status_error) = completion_queue_status(default_root, &root);
    let closure = completion_closure(
        true,
        true,
        commit_outcome.commit.clone(),
        completion_evidence_refs,
    );
    let compact = completion_compact(
        &item_id,
        true,
        &closure,
        &commit_outcome,
        commit.clone(),
        record_auto_evidence,
        &generated_evidence,
        &warnings,
        queue_status.as_ref(),
        &format!("Completed direct backlog item `{item_id}`."),
    );
    let (queue_status, queue_status_error) =
        verbose_queue_status(detail, queue_status, queue_status_error);
    let data = CompleteBacklogItemData {
        root,
        item_id: item_id.clone(),
        summary: summary.to_string(),
        detail: detail.to_string(),
        compact,
        warnings,
        changed_files,
        evidence: evidence_records,
        auto_evidence_enabled: record_auto_evidence,
        generated_evidence,
        event,
        commit,
        closed: true,
        closure,
        commit_outcome,
        queue_status,
        queue_status_error,
        host_action,
    };
    let mut result = ActionResult::completed(
        action,
        format!("Completed direct backlog item `{item_id}`."),
        data,
    );
    result.next_action = Some("Run inspect_work_queue to continue with the next item.".to_string());
    result
}

fn completion_queue_status(
    default_root: &Path,
    root: &str,
) -> (Option<QueueStatusData>, Option<String>) {
    match guidance::inspect_queue_status(
        default_root,
        crate::models::InspectQueueStatusParams {
            root: Some(root.to_string()),
            limit: Some(5),
        },
    ) {
        ActionResult {
            data: Some(data), ..
        } => (Some(data), None),
        ActionResult { summary, error, .. } => (None, Some(error.unwrap_or(summary))),
    }
}

fn completion_detail(detail: Option<&str>) -> Result<&'static str, String> {
    match detail.unwrap_or("compact").trim() {
        "" | "compact" => Ok("compact"),
        "verbose" => Ok("verbose"),
        value => Err(format!(
            "unsupported detail `{value}`; use compact or verbose"
        )),
    }
}

fn verbose_queue_status(
    detail: &str,
    queue_status: Option<QueueStatusData>,
    queue_status_error: Option<String>,
) -> (Option<QueueStatusData>, Option<String>) {
    if detail == "verbose" {
        (queue_status, queue_status_error)
    } else {
        (None, None)
    }
}

fn completion_compact(
    item_id: &str,
    closed: bool,
    closure: &CompletionClosureState,
    commit_outcome: &CompletionCommitOutcome,
    commit: Option<String>,
    auto_evidence_enabled: bool,
    generated_evidence: &[GeneratedEvidenceSummary],
    warnings: &[String],
    queue_status: Option<&QueueStatusData>,
    summary: &str,
) -> CompleteBacklogItemCompact {
    let next_ready = queue_status.and_then(|queue| queue.top_ready_items.first());
    CompleteBacklogItemCompact {
        item_id: item_id.to_string(),
        closed,
        closure_source: closure.source.clone(),
        commit_status: commit_outcome.status.clone(),
        commit,
        auto_evidence_enabled,
        generated_evidence: generated_evidence.to_vec(),
        warnings: warnings.to_vec(),
        queue_state: queue_status.map(|queue| queue.queue_state.clone()),
        next_ready_item_id: next_ready.map(|item| item.item_id.clone()),
        recommended_tool: queue_status.map(|queue| queue.recommended_tool.clone()),
        summary: summary.to_string(),
    }
}

fn completion_commit_outcome(
    requested: bool,
    commit: Option<String>,
    skipped_already_closed: bool,
) -> CompletionCommitOutcome {
    if let Some(commit) = commit {
        CompletionCommitOutcome {
            requested,
            status: "created".to_string(),
            commit: Some(commit),
            reason: "Created a closure commit with Platypus-Closes and verification trailers."
                .to_string(),
        }
    } else if requested && skipped_already_closed {
        CompletionCommitOutcome {
            requested,
            status: "skipped".to_string(),
            commit: None,
            reason: "Skipped commit because the backlog item was already closed before this call."
                .to_string(),
        }
    } else {
        CompletionCommitOutcome {
            requested,
            status: "not_requested".to_string(),
            commit: None,
            reason: "No Git closure commit was requested; closure is recorded in local runtime state unless an existing Git trailer also closes the item.".to_string(),
        }
    }
}

fn completion_closure(
    closed: bool,
    runtime_completion_recorded: bool,
    closure_commit: Option<String>,
    evidence_refs: Vec<String>,
) -> CompletionClosureState {
    let git_trailer_portable = closure_commit.is_some();
    let source = match (runtime_completion_recorded, git_trailer_portable) {
        (true, true) => "runtime_event_and_git_trailer",
        (true, false) => "runtime_event",
        (false, true) => "git_trailer",
        (false, false) => "already_closed",
    }
    .to_string();
    let summary = if git_trailer_portable {
        "Item is closed by the recorded direct completion and a portable Git Platypus-Closes trailer."
            .to_string()
    } else if runtime_completion_recorded {
        "Item is closed by local runtime completion state. Add or keep a Git Platypus-Closes trailer when closure must travel with repository history.".to_string()
    } else {
        "Item was already closed before this call.".to_string()
    };
    CompletionClosureState {
        closed,
        source,
        runtime_completion_recorded,
        git_trailer_portable,
        closure_commit,
        evidence_refs,
        summary,
    }
}

fn already_closed_closure(root: &Path, item_id: &str) -> CompletionClosureState {
    let sources = backlog::closure_sources(root, item_id);
    let source = match (sources.runtime_completion, sources.git_trailer) {
        (true, true) => "runtime_event_and_git_trailer",
        (true, false) => "runtime_event",
        (false, true) => "git_trailer",
        (false, false) => "already_closed",
    }
    .to_string();
    let summary = match (sources.runtime_completion, sources.git_trailer) {
        (true, true) => "Item was already closed by local runtime completion state and a reachable Git Platypus-Closes trailer.",
        (true, false) => "Item was already closed by local runtime completion state.",
        (false, true) => "Item was already closed by a reachable Git Platypus-Closes trailer.",
        (false, false) => "Item was already closed before this call; inspect backlog inventory, evidence, or Git trailers for the closure source.",
    }
    .to_string();
    CompletionClosureState {
        closed: true,
        source,
        runtime_completion_recorded: sources.runtime_completion,
        git_trailer_portable: sources.git_trailer,
        closure_commit: None,
        evidence_refs: Vec::new(),
        summary,
    }
}

pub fn finish_work(default_root: &Path, params: FinishWorkParams) -> ActionResult<FinishWorkData> {
    let action = "finish_work";
    if params.assignment_id.is_none() && params.task_id.is_none() {
        return direct_finish_guidance(default_root, params);
    }
    let root = params.root.clone();
    let status = params
        .status
        .clone()
        .unwrap_or_else(|| "completed".to_string());
    let mut task_id = params.task_id.clone();
    let inspected_assignment = params.assignment_id.as_ref().and_then(|assignment_id| {
        assignments::inspect_worker_assignment(
            default_root,
            crate::models::InspectWorkerAssignmentParams {
                root: root.clone(),
                assignment_id: assignment_id.clone(),
            },
        )
        .data
        .map(|data| data.assignment)
    });
    if task_id.is_none() {
        task_id = inspected_assignment
            .as_ref()
            .map(|assignment| assignment.task_id.clone());
    }

    let mut worktree_changes =
        task_id.as_ref().and_then(|task_id| {
            match workspace::worktree_diff(
                default_root,
                WorktreeDiffParams {
                    root: root.clone(),
                    task_id: task_id.clone(),
                },
            ) {
                ActionResult {
                    status: ActionStatus::Completed,
                    data: Some(data),
                    ..
                } => Some(data),
                _ => None,
            }
        });
    let mut changed_files = params.changed_files.clone();
    if changed_files.is_empty() {
        if let Some(diff) = worktree_changes.as_ref() {
            changed_files = diff.files.iter().map(|file| file.path.clone()).collect();
        }
    }

    let assignment = if let Some(assignment) = inspected_assignment
        .as_ref()
        .filter(|assignment| assignment.status == "completed")
        .cloned()
    {
        assignment
    } else {
        let completed = assignments::complete_worker_execution(
            default_root,
            CompleteWorkerExecutionParams {
                root: root.clone(),
                assignment_id: params.assignment_id.clone(),
                task_id: task_id.clone(),
                status: status.clone(),
                summary: params.summary.clone(),
                changed_files,
                verification_status: params.verification_status.clone(),
                auto_start_if_prepared: params.auto_start_if_prepared,
            },
        );
        match completed {
            ActionResult {
                status: ActionStatus::Completed,
                data: Some(data),
                ..
            } => data.assignment,
            ActionResult {
                summary,
                next_action,
                error,
                ..
            } => {
                return finish_failed(
                    action,
                    summary,
                    next_action,
                    error,
                    task_id,
                    params.assignment_id,
                    inspected_assignment,
                    worktree_changes,
                )
            }
        }
    };
    let refreshed_worktree_changes = workspace::worktree_diff(
        default_root,
        WorktreeDiffParams {
            root: root.clone(),
            task_id: assignment.task_id.clone(),
        },
    );
    if let ActionResult {
        status: ActionStatus::Completed,
        data: Some(data),
        ..
    } = refreshed_worktree_changes
    {
        worktree_changes = Some(data);
    }

    let mut evidence_records = Vec::new();
    if should_record_verification_evidence(params.verification_status.as_deref(), &params) {
        let verification = evidence::record_verification_evidence(
            default_root,
            RecordVerificationEvidenceParams {
                root: root.clone(),
                id: None,
                source_item_id: Some(assignment.bundle.item_id.clone()),
                source_task_id: Some(assignment.task_id.clone()),
                summary: params.verification_summary.clone().unwrap_or_else(|| {
                    format!(
                        "Worker reported verification `{}` for `{}`.",
                        params
                            .verification_status
                            .as_deref()
                            .unwrap_or("not_recorded"),
                        assignment.task_id
                    )
                }),
                refs: params.verification_refs.clone(),
                metadata: BTreeMap::new(),
            },
        );
        match verification {
            ActionResult {
                status: ActionStatus::Completed,
                data: Some(data),
                ..
            } => evidence_records.push(data.evidence),
            ActionResult {
                summary,
                next_action,
                error,
                ..
            } => {
                return finish_failed_with_assignment(
                    action,
                    summary,
                    next_action,
                    error,
                    assignment,
                    worktree_changes,
                    evidence_records,
                    Vec::new(),
                )
            }
        }
    }

    let mut finding_records = Vec::new();
    for finding in params.findings {
        let record = findings::record_finding(
            default_root,
            finish_finding_params(&root, &assignment, finding),
        );
        match record {
            ActionResult {
                status: ActionStatus::Completed,
                data: Some(data),
                ..
            } => finding_records.push(data.finding),
            ActionResult {
                summary,
                next_action,
                error,
                ..
            } => {
                return finish_failed_with_assignment(
                    action,
                    summary,
                    next_action,
                    error,
                    assignment,
                    worktree_changes,
                    evidence_records,
                    finding_records,
                )
            }
        }
    }

    let findings_valid = findings::validate_findings(
        default_root,
        crate::models::ValidateFindingsParams {
            root: root.clone(),
            source_item_id: Some(assignment.bundle.item_id.clone()),
            source_task_id: Some(assignment.task_id.clone()),
        },
    );
    let unresolved_required = findings_valid
        .data
        .as_ref()
        .map(|data| data.unresolved_required_count)
        .unwrap_or(0);
    let findings_reviewed =
        params.findings_reviewed.unwrap_or(false) || !finding_records.is_empty();

    let mut integration = None;
    let mut reconciliation = None;
    let meaningful_changes = worktree_changes
        .as_ref()
        .is_some_and(|changes| changes.meaningful_changes);
    if params.integrate_if_ready.unwrap_or(false)
        && status == "completed"
        && verification_ready(
            params.verification_status.as_deref(),
            params.allow_unverified,
        )
        && unresolved_required == 0
        && findings_reviewed
        && meaningful_changes
    {
        let integrated = workspace::integrate_worker_result(
            default_root,
            IntegrateWorkerResultParams {
                root: root.clone(),
                task_id: assignment.task_id.clone(),
                strategy: params.integration_strategy,
                allow_unverified: params.allow_unverified,
                cleanup_after: params.cleanup_after,
            },
        );
        if let ActionResult {
            status: ActionStatus::Completed,
            data: Some(data),
            ..
        } = integrated
        {
            integration = Some(data);
            if let ActionResult {
                status: ActionStatus::Completed,
                data: Some(data),
                ..
            } =
                reconcile::reconcile_project(default_root, ReconcileParams { root: root.clone() })
            {
                reconciliation = Some(data);
            }
        }
    }

    let host_action = finish_host_action(
        &assignment,
        status.as_str(),
        params.verification_status.as_deref(),
        params.allow_unverified.unwrap_or(false),
        findings_reviewed,
        unresolved_required,
        integration.is_some(),
        meaningful_changes,
    );
    let root = assignment_root(&root, &assignment);
    let mut result = ActionResult::completed(
        action,
        format!("Finished worker assignment `{}`.", assignment.id),
        FinishWorkData {
            root,
            assignment: Some(assignment),
            worktree_changes,
            evidence: evidence_records,
            findings: finding_records,
            integration,
            reconciliation,
            host_action,
        },
    );
    result.next_action = Some(
        result
            .data
            .as_ref()
            .map(|data| data.host_action.summary.clone())
            .unwrap_or_else(|| "Inspect the task lifecycle state.".to_string()),
    );
    result
}

fn prepare_queue_only_data(
    queue: WorkQueueData,
    host_actions: Vec<HostAction>,
    include_queue_snapshot: bool,
    prepared_state: &str,
    state_persisted: bool,
    persistence: &str,
    persistence_summary: &str,
    selected_item_ids: Vec<String>,
) -> PrepareWorkData {
    PrepareWorkData {
        root: queue.root.clone(),
        queue_summary: queue.summary.clone(),
        prepared_state: prepared_state.to_string(),
        state_persisted,
        persistence: persistence.to_string(),
        persistence_summary: persistence_summary.to_string(),
        durable_next_tool: durable_next_tool_for_prepared_state(prepared_state),
        selected_item_ids,
        ready_count: queue.ready_count,
        blocked_count: queue.blocked_count,
        host_actions,
        dispatch: None,
        queue: include_queue_snapshot.then_some(queue),
    }
}

#[derive(Debug)]
struct PrepareSelection {
    item_id: String,
    title: String,
    execution_path: String,
}

fn selected_prepare_items(
    queue: &WorkQueueData,
    requested_item_id: Option<&str>,
    max_tasks: usize,
    manual_handoff: bool,
) -> Vec<PrepareSelection> {
    queue
        .items
        .iter()
        .filter(|item| requested_item_id.is_none_or(|item_id| item.candidate.item_id == item_id))
        .filter(|item| ready_for_prepare_work(item, manual_handoff))
        .take(max_tasks)
        .map(|item| PrepareSelection {
            item_id: item.candidate.item_id.clone(),
            title: item.candidate.title.clone(),
            execution_path: item.effective_policy.execution_path.clone(),
        })
        .collect()
}

fn direct_host_action(
    item: &PrepareSelection,
    root: &str,
    worker: Option<String>,
    auto_commit_artifacts_requested: bool,
) -> HostAction {
    let mut instructions = vec![
        format!("Implement `{}` directly in the manager workspace when the user wants a lightweight scaffold or direct edit.", item.title),
        "This prepare_work result is response-local guidance; it does not persist a direct-prepared marker.".to_string(),
        "No worker process, task assignment, or Git worktree was created; the manager workspace is the expected target.".to_string(),
        "When the direct edit is done, call complete_backlog_item with item_id, summary, changed_files, verification status, and any evidence or finding references.".to_string(),
    ];
    if auto_commit_artifacts_requested {
        instructions.push(
            "auto_commit_artifacts is not applicable to direct work; use complete_backlog_item commit=true when you want Platypus to create the closure commit for explicit changed_files."
                .to_string(),
        );
    }
    HostAction {
        kind: "direct_edit".to_string(),
        summary: format!(
            "`{}` is direct work; implement it in the manager workspace and complete it with complete_backlog_item.",
            item.item_id
        ),
        instructions,
        task_id: None,
        assignment_id: None,
        worker,
        worktree_path: Some(root.to_string()),
        bundle: None,
        next_tools: vec![
            "complete_backlog_item".to_string(),
            "record_verification_evidence".to_string(),
            "record_finding".to_string(),
            "reconcile_project".to_string(),
        ],
    }
}

fn durable_next_tool_for_prepared_state(prepared_state: &str) -> Option<String> {
    match prepared_state {
        "direct_guidance" => Some("complete_backlog_item".to_string()),
        "worktree_prepared" => Some("finish_work".to_string()),
        _ => None,
    }
}

fn durable_next_tool_for_host_actions(host_actions: &[HostAction]) -> Option<String> {
    let mut tools = host_actions
        .iter()
        .filter_map(|action| match action.kind.as_str() {
            "direct_edit" => Some("complete_backlog_item"),
            "run_in_worktree" => Some("finish_work"),
            _ => None,
        });
    let first = tools.next()?;
    if tools.all(|tool| tool == first) {
        Some(first.to_string())
    } else {
        None
    }
}

fn dispatch_selected_work(
    default_root: &Path,
    params: &PrepareWorkParams,
    requested_execution_mode: &str,
    item_ids: &[String],
) -> Result<DispatchReadyWorkData, ActionResult<DispatchReadyWorkData>> {
    let mut combined: Option<DispatchReadyWorkData> = None;
    for item_id in item_ids {
        let result = dispatch::dispatch_ready_work(
            default_root,
            DispatchReadyWorkParams {
                root: params.root.clone(),
                item_id: Some(item_id.clone()),
                max_tasks: Some(1),
                worker: params.worker.clone(),
                claimant: params.claimant.clone(),
                execution_mode: Some(requested_execution_mode.to_string()),
                prepare_handoffs: Some(true),
                auto_start: Some(false),
                auto_commit_artifacts: params.auto_commit_artifacts,
                require_planning_approval: None,
                dry_run: Some(false),
                verification_command: params.verification_command.clone(),
            },
        );
        let dispatch = match result {
            ActionResult {
                status: ActionStatus::Completed | ActionStatus::Skipped,
                data: Some(data),
                ..
            } => data,
            failed => return Err(failed),
        };
        merge_dispatch_data(&mut combined, dispatch);
    }
    combined.ok_or_else(|| ActionResult {
        action: "dispatch_ready_work".to_string(),
        status: ActionStatus::Skipped,
        summary: "No non-direct work was selected for dispatch.".to_string(),
        next_action: Some(
            "Select direct work or runnable worker work, then retry prepare_work.".to_string(),
        ),
        recovery_action: None,
        data: None,
        error: None,
    })
}

fn merge_dispatch_data(
    target: &mut Option<DispatchReadyWorkData>,
    mut dispatch: DispatchReadyWorkData,
) {
    let Some(current) = target.as_mut() else {
        *target = Some(dispatch);
        return;
    };
    current.requested += dispatch.requested;
    current.available_before_dispatch = current
        .available_before_dispatch
        .max(dispatch.available_before_dispatch);
    current.selected += dispatch.selected;
    current.dispatched += dispatch.dispatched;
    current.prepared += dispatch.prepared;
    current.started += dispatch.started;
    current.failed += dispatch.failed;
    current
        .preflight_warnings
        .append(&mut dispatch.preflight_warnings);
    if dispatch.stopped_reason != "selection_exhausted"
        && dispatch.stopped_reason != "max_tasks_reached"
    {
        current.stopped_reason = dispatch.stopped_reason;
    }
    current.items.append(&mut dispatch.items);
}

fn host_action_from_dispatch_item(item: &DispatchReadyWorkItem) -> Option<HostAction> {
    let assignment = item.assignment.as_ref()?;
    Some(HostAction {
        kind: "run_in_worktree".to_string(),
        summary: format!(
            "Run `{}` in `{}`; MCP prepared the assignment but did not launch a worker.",
            assignment.id, assignment.worktree_path
        ),
        instructions: vec![
            "Give the bundle brief and worktree path to the selected worker harness or human implementer.".to_string(),
            "Run edits only inside the returned assignment worktree.".to_string(),
            "Use start_worker_task if you need an explicit running transition.".to_string(),
            "Finish through finish_work so changed files, verification, findings, integration guidance, and reconciliation stay connected.".to_string(),
        ],
        task_id: Some(assignment.task_id.clone()),
        assignment_id: Some(assignment.id.clone()),
        worker: assignment.worker.clone(),
        worktree_path: Some(assignment.worktree_path.clone()),
        bundle: Some(assignment.bundle.clone()),
        next_tools: vec![
            "start_worker_task".to_string(),
            "record_worker_progress".to_string(),
            "finish_work".to_string(),
            "inspect_task_events".to_string(),
        ],
    })
}

fn should_record_verification_evidence(status: Option<&str>, params: &FinishWorkParams) -> bool {
    matches!(status, Some("passed")) || params.verification_summary.is_some()
}

fn verification_ready(status: Option<&str>, allow_unverified: Option<bool>) -> bool {
    matches!(status, Some("passed")) || allow_unverified.unwrap_or(false)
}

fn finish_finding_params(
    root: &Option<String>,
    assignment: &WorkerAssignment,
    finding: FinishWorkFindingInput,
) -> RecordFindingParams {
    let mut metadata = BTreeMap::new();
    if let Some(owner) = finding.owner.as_ref() {
        metadata.insert("owner".to_string(), Value::String(owner.clone()));
    }
    RecordFindingParams {
        root: root.clone(),
        id: None,
        source_item_id: Some(assignment.bundle.item_id.clone()),
        source_task_id: Some(assignment.task_id.clone()),
        source_finding_ref: None,
        title: finding.title,
        summary: finding.summary,
        severity: finding.severity,
        required: finding.required,
        evidence_refs: finding.evidence_refs,
        metadata,
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_host_action(
    assignment: &WorkerAssignment,
    status: &str,
    verification_status: Option<&str>,
    allow_unverified: bool,
    findings_reviewed: bool,
    unresolved_required: usize,
    integrated: bool,
    meaningful_changes: bool,
) -> HostAction {
    if status != "completed" {
        return HostAction {
            kind: "inspect_or_recover".to_string(),
            summary: format!("Assignment `{}` ended with `{status}`.", assignment.id),
            instructions: vec![
                "Inspect task events and worktree changes before deciding whether to retry, clean up, or create follow-up work.".to_string(),
            ],
            task_id: Some(assignment.task_id.clone()),
            assignment_id: Some(assignment.id.clone()),
            worker: assignment.worker.clone(),
            worktree_path: Some(assignment.worktree_path.clone()),
            bundle: Some(assignment.bundle.clone()),
            next_tools: vec![
                "inspect_task_events".to_string(),
                "inspect_worktree_changes".to_string(),
                "record_finding".to_string(),
            ],
        };
    }
    if !verification_ready(verification_status, Some(allow_unverified)) {
        return HostAction {
            kind: "verify_or_record_risk".to_string(),
            summary: "Verification is not passed or explicitly waived; verify before integration.".to_string(),
            instructions: vec![
                "Run run_task_verification for the assignment or task when a command is configured.".to_string(),
                "Record verification evidence for passed checks or record a finding when verification cannot pass.".to_string(),
                "Use allow_unverified only for an explicit low-risk decision.".to_string(),
            ],
            task_id: Some(assignment.task_id.clone()),
            assignment_id: Some(assignment.id.clone()),
            worker: assignment.worker.clone(),
            worktree_path: Some(assignment.worktree_path.clone()),
            bundle: Some(assignment.bundle.clone()),
            next_tools: vec![
                "run_task_verification".to_string(),
                "record_verification_evidence".to_string(),
                "record_finding".to_string(),
            ],
        };
    }
    if unresolved_required > 0 || !findings_reviewed {
        return HostAction {
            kind: "resolve_findings".to_string(),
            summary: if unresolved_required > 0 {
                format!("{unresolved_required} required finding(s) must be explicitly dispositioned before final integration.")
            } else {
                "Confirm findings were reviewed or record follow-up findings before integration.".to_string()
            },
            instructions: vec![
                "Record limitations, impediments, and follow-up work as findings.".to_string(),
                "Use findings_reviewed=true only after explicitly checking that no findings are needed.".to_string(),
                "Accept, defer, resolve, reject, or mark required findings as duplicate before integrating.".to_string(),
            ],
            task_id: Some(assignment.task_id.clone()),
            assignment_id: Some(assignment.id.clone()),
            worker: assignment.worker.clone(),
            worktree_path: Some(assignment.worktree_path.clone()),
            bundle: Some(assignment.bundle.clone()),
            next_tools: vec![
                "record_finding".to_string(),
                "validate_findings".to_string(),
                "update_finding_disposition".to_string(),
            ],
        };
    }
    if integrated {
        return HostAction {
            kind: "done".to_string(),
            summary: format!("Task `{}` is finished and integrated.", assignment.task_id),
            instructions: vec![
                "Review reconciliation output for any remaining gaps.".to_string(),
                "Select the next safe action or prepare the next item.".to_string(),
            ],
            task_id: Some(assignment.task_id.clone()),
            assignment_id: Some(assignment.id.clone()),
            worker: assignment.worker.clone(),
            worktree_path: Some(assignment.worktree_path.clone()),
            bundle: Some(assignment.bundle.clone()),
            next_tools: vec!["reconcile_project".to_string(), "prepare_work".to_string()],
        };
    }
    if !meaningful_changes {
        return HostAction {
            kind: "done".to_string(),
            summary: format!(
                "Assignment `{}` is complete and has no meaningful worktree changes to integrate.",
                assignment.id
            ),
            instructions: vec![
                "No integration merge is required because the worker produced no file changes."
                    .to_string(),
                "Use worktree_cleanup when the clean task worktree is no longer needed."
                    .to_string(),
                "Inspect the queue or run reconcile_project if you need an audit pass.".to_string(),
            ],
            task_id: Some(assignment.task_id.clone()),
            assignment_id: Some(assignment.id.clone()),
            worker: assignment.worker.clone(),
            worktree_path: Some(assignment.worktree_path.clone()),
            bundle: Some(assignment.bundle.clone()),
            next_tools: vec![
                "worktree_cleanup".to_string(),
                "inspect_work_queue".to_string(),
                "reconcile_project".to_string(),
            ],
        };
    }
    HostAction {
        kind: "integrate_result".to_string(),
        summary: format!(
            "Assignment `{}` is complete; review and integrate the worker result.",
            assignment.id
        ),
        instructions: vec![
            "Inspect the worktree changes and evidence.".to_string(),
            "Run inspect_integration_gates when the host needs a read-only checklist before integrating.".to_string(),
            "Integrate the worker result when the manager workspace is clean.".to_string(),
            "Run reconcile_project after integration to surface closure or finding gaps."
                .to_string(),
        ],
        task_id: Some(assignment.task_id.clone()),
        assignment_id: Some(assignment.id.clone()),
        worker: assignment.worker.clone(),
        worktree_path: Some(assignment.worktree_path.clone()),
        bundle: Some(assignment.bundle.clone()),
        next_tools: vec![
            "inspect_worktree_changes".to_string(),
            "inspect_integration_gates".to_string(),
            "integrate_worker_result".to_string(),
            "reconcile_project".to_string(),
        ],
    }
}

fn finish_failed(
    action: &str,
    summary: String,
    next_action: Option<String>,
    error: Option<String>,
    task_id: Option<String>,
    assignment_id: Option<String>,
    assignment: Option<WorkerAssignment>,
    worktree_changes: Option<WorktreeDiffData>,
) -> ActionResult<FinishWorkData> {
    let host_action = HostAction {
        kind: "inspect_or_recover".to_string(),
        summary: "Could not finish worker result.".to_string(),
        instructions: vec![
            "Inspect the assignment and task events to recover the lifecycle.".to_string(),
            "Retry finish_work after the assignment is prepared or running.".to_string(),
        ],
        task_id,
        assignment_id,
        worker: assignment.as_ref().and_then(|value| value.worker.clone()),
        worktree_path: assignment.as_ref().map(|value| value.worktree_path.clone()),
        bundle: assignment.as_ref().map(|value| value.bundle.clone()),
        next_tools: vec![
            "inspect_worker_assignment".to_string(),
            "inspect_task_events".to_string(),
            "inspect_work_queue".to_string(),
        ],
    };
    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Failed,
        summary,
        next_action: next_action.clone(),
        recovery_action: next_action,
        data: Some(FinishWorkData {
            root: String::new(),
            assignment,
            worktree_changes,
            evidence: Vec::new(),
            findings: Vec::new(),
            integration: None,
            reconciliation: None,
            host_action,
        }),
        error,
    }
}

fn finish_failed_with_assignment(
    action: &str,
    summary: String,
    next_action: Option<String>,
    error: Option<String>,
    assignment: WorkerAssignment,
    worktree_changes: Option<WorktreeDiffData>,
    evidence: Vec<EvidenceRecord>,
    findings: Vec<crate::models::FindingRecord>,
) -> ActionResult<FinishWorkData> {
    let root = assignment_root(&None, &assignment);
    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Failed,
        summary,
        next_action: next_action.clone(),
        recovery_action: next_action,
        data: Some(FinishWorkData {
            root,
            assignment: Some(assignment.clone()),
            worktree_changes,
            evidence,
            findings,
            integration: None,
            reconciliation: None,
            host_action: HostAction {
                kind: "inspect_or_recover".to_string(),
                summary: "Finish work stopped after a partial lifecycle update.".to_string(),
                instructions: vec![
                    "Inspect the assignment, evidence, findings, and task events before retrying."
                        .to_string(),
                ],
                task_id: Some(assignment.task_id.clone()),
                assignment_id: Some(assignment.id.clone()),
                worker: assignment.worker.clone(),
                worktree_path: Some(assignment.worktree_path.clone()),
                bundle: Some(assignment.bundle.clone()),
                next_tools: vec![
                    "inspect_worker_assignment".to_string(),
                    "list_evidence".to_string(),
                    "list_findings".to_string(),
                    "inspect_task_events".to_string(),
                ],
            },
        }),
        error,
    }
}

fn direct_finish_guidance(
    default_root: &Path,
    params: FinishWorkParams,
) -> ActionResult<FinishWorkData> {
    let root = params.root.clone().unwrap_or_else(|| {
        default_root
            .canonicalize()
            .unwrap_or_else(|_| default_root.to_path_buf())
            .display()
            .to_string()
    });
    let item_id = params.item_id.clone();
    let host_action = HostAction {
        kind: "inspect_or_recover".to_string(),
        summary: "finish_work is for worker assignments; direct host work finishes with complete_backlog_item.".to_string(),
        instructions: vec![
            "Call complete_backlog_item for direct work returned by prepare_work with host action direct_edit.".to_string(),
            "Pass item_id, summary, changed_files, verification status, and any evidence or finding references.".to_string(),
            "Use finish_work only when prepare_work returned a task_id or assignment_id for a worker handoff.".to_string(),
        ],
        task_id: None,
        assignment_id: None,
        worker: None,
        worktree_path: Some(root.clone()),
        bundle: None,
        next_tools: vec!["complete_backlog_item".to_string(), "inspect_work_queue".to_string()],
    };
    ActionResult {
        action: "finish_work".to_string(),
        status: ActionStatus::Skipped,
        summary: "Direct work should be completed with complete_backlog_item.".to_string(),
        next_action: Some(if let Some(item_id) = item_id {
            format!("Call complete_backlog_item for `{item_id}` with summary, changed_files, and verification details.")
        } else {
            "Call complete_backlog_item with the direct backlog item id, summary, changed_files, and verification details.".to_string()
        }),
        recovery_action: None,
        data: Some(FinishWorkData {
            root,
            assignment: None,
            worktree_changes: None,
            evidence: Vec::new(),
            findings: Vec::new(),
            integration: None,
            reconciliation: None,
            host_action,
        }),
        error: None,
    }
}

fn clean_item_id(value: &str) -> Result<String, String> {
    let item_id = value.trim().to_ascii_uppercase();
    let Some((prefix, number)) = item_id.split_once('-') else {
        return Err("item_id must look like PROJ-001".to_string());
    };
    if prefix.is_empty()
        || !prefix
            .chars()
            .all(|character| character.is_ascii_uppercase())
        || number.len() != 3
        || !number.chars().all(|character| character.is_ascii_digit())
    {
        return Err("item_id must look like PROJ-001".to_string());
    }
    Ok(item_id)
}

fn validate_changed_files(paths: &[String]) -> Result<Vec<String>, String> {
    let mut validated = Vec::new();
    for path in paths {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            continue;
        }
        let candidate = Path::new(trimmed);
        if candidate.is_absolute() {
            return Err(format!("changed file `{trimmed}` must be relative"));
        }
        if trimmed.starts_with(".platy/") || trimmed == ".platy" {
            return Err("changed_files must not point at Platypus runtime state".to_string());
        }
        if candidate.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(format!(
                "changed file `{trimmed}` must stay inside the project root"
            ));
        }
        validated.push(trimmed.to_string());
    }
    Ok(validated)
}

fn validate_explicit_completion_evidence(
    default_root: &Path,
    root: &str,
    item_id: &str,
    evidence_refs: &[String],
) -> Result<(), String> {
    let requested = evidence_refs
        .iter()
        .map(|reference| reference.trim())
        .filter(|reference| !reference.is_empty())
        .collect::<Vec<_>>();
    if requested.is_empty() {
        return Err(
            "record_auto_evidence=false requires evidence_refs for existing evidence records"
                .to_string(),
        );
    }

    let mut unknown = Vec::new();
    let mut wrong_item = Vec::new();
    for reference in &requested {
        match evidence::inspect_evidence(default_root, Some(root), reference) {
            ActionResult {
                status: ActionStatus::Completed,
                data: Some(data),
                ..
            } if data.evidence.source_item_id.as_deref() == Some(item_id) => {}
            ActionResult {
                status: ActionStatus::Completed,
                ..
            } => wrong_item.push(*reference),
            ActionResult { .. } => unknown.push(*reference),
        }
    }
    if !unknown.is_empty() {
        return Err(format!(
            "record_auto_evidence=false requires evidence_refs to reference existing evidence for `{item_id}`; unknown: {}",
            unknown.join(", ")
        ));
    }
    if !wrong_item.is_empty() {
        return Err(format!(
            "record_auto_evidence=false requires evidence_refs to belong to `{item_id}`; wrong item: {}",
            wrong_item.join(", ")
        ));
    }
    Ok(())
}

fn changed_file_warnings(
    root: &Path,
    paths: &[String],
    empty_files_explained: bool,
) -> Vec<String> {
    if paths.is_empty() {
        if empty_files_explained {
            return Vec::new();
        }
        return vec![
            "changed_files is empty; include touched paths when files changed or provide evidence_refs for non-file work."
                .to_string(),
        ];
    }
    let mut warnings = Vec::new();
    for path in paths {
        if !root.join(path).exists() {
            warnings.push(format!(
                "changed_files includes `{path}`, but that path does not exist in the manager workspace."
            ));
            continue;
        }
        let output = Command::new("git")
            .args([
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--",
                path,
            ])
            .current_dir(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        if let Ok(output) = output {
            if output.status.success() && String::from_utf8_lossy(&output.stdout).trim().is_empty()
            {
                if is_platypus_owned_changed_file(path) {
                    continue;
                }
                warnings.push(format!(
                    "changed_files includes `{path}`, but Git does not report local changes for it."
                ));
            }
        }
    }
    warnings
}

fn empty_changed_files_explained(params: &CompleteBacklogItemParams) -> bool {
    params.verification_status.is_some()
        || params.verification_summary.is_some()
        || !params.verification_refs.is_empty()
        || !params.evidence_refs.is_empty()
}

fn is_platypus_owned_changed_file(path: &str) -> bool {
    path == "platy.yaml"
        || path == "WORKFLOW.md"
        || path == "AGENTS.md"
        || path == "CLAUDE.md"
        || path.starts_with("backlog/")
}

fn completion_refs(
    changed_files: &[String],
    evidence_refs: &[String],
    finding_refs: &[String],
) -> Vec<String> {
    let mut refs = changed_files
        .iter()
        .map(|path| format!("file:{path}"))
        .collect::<Vec<_>>();
    refs.extend(evidence_refs.iter().cloned());
    refs.extend(
        finding_refs
            .iter()
            .map(|finding| format!("finding:{finding}")),
    );
    refs
}

fn commit_direct_completion(
    root: &Path,
    item_id: &str,
    commit_message: Option<&str>,
    summary: &str,
    verification_summary: Option<&str>,
    verification_status: Option<&str>,
    changed_files: &[String],
) -> Result<String, String> {
    if changed_files.is_empty() {
        return Err(
            "changed_files is required when commit=true so Platypus does not stage unrelated work"
                .to_string(),
        );
    }
    run_git(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let mut add_args = vec!["add".to_string(), "--".to_string()];
    add_args.extend(changed_files.iter().cloned());
    let add_refs = add_args.iter().map(String::as_str).collect::<Vec<_>>();
    run_git(root, &add_refs)?;
    let subject = commit_message
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("Complete {item_id} direct work"));
    let verification = verification_summary
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(verification_status)
        .unwrap_or("direct completion recorded");
    commit_with_message(
        root,
        &subject,
        &[
            format!("Platypus-Closes: {item_id}"),
            format!("Platypus-Verification: {}", one_line(verification)),
            format!("Platypus-Direct-Summary: {}", one_line(summary)),
        ],
    )?;
    head_commit(root)
}

fn commit_with_message(root: &Path, subject: &str, body_lines: &[String]) -> Result<(), String> {
    let mut args = vec!["commit".to_string(), "-m".to_string(), one_line(subject)];
    if !body_lines.is_empty() {
        args.push("-m".to_string());
        args.push(
            body_lines
                .iter()
                .map(|line| one_line(line))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_git(root, &arg_refs).map(|_| ())
}

fn head_commit(root: &Path) -> Result<String, String> {
    let output = run_git(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let commit = output.lines().next().unwrap_or_default().trim();
    if commit.is_empty() {
        Err("HEAD did not resolve to a commit".to_string())
    } else {
        Ok(commit.to_string())
    }
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start git: {error}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("failed to collect git output: {error}"))?;
                if output.status.success() {
                    return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
                }
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return Err(if stderr.is_empty() {
                    format!("git {:?} failed with {}", args, output.status)
                } else {
                    stderr
                });
            }
            Ok(None) => {
                if started.elapsed() > GIT_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("git {:?} timed out", args));
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(format!("failed to poll git: {error}")),
        }
    }
}

fn assignment_root(root: &Option<String>, assignment: &WorkerAssignment) -> String {
    root.clone().unwrap_or_else(|| {
        let worktree = std::path::Path::new(&assignment.worktree_path);
        worktree
            .ancestors()
            .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(".platy"))
            .and_then(|path| path.parent())
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn prepare_work_returns_direct_action_for_direct_items() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Readme docs",
            "docs",
            "docs",
            &["README.md"],
        );
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add direct backlog"]);

        let result = prepare_work(
            project.path(),
            PrepareWorkParams {
                root: None,
                item_id: None,
                max_tasks: None,
                worker: None,
                claimant: None,
                execution_mode: None,
                require_task_plan: Some(false),
                require_planning_approval: None,
                auto_commit_artifacts: None,
                verification_command: Vec::new(),
                include_queue_snapshot: None,
            },
        );
        let data = result.data.expect("prepare data");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.host_actions[0].kind, "direct_edit");
        assert!(data.host_actions[0]
            .next_tools
            .contains(&"complete_backlog_item".to_string()));
        assert!(data.host_actions[0]
            .instructions
            .iter()
            .any(|instruction| instruction
                .contains("No worker process, task assignment, or Git worktree was created")));
        assert!(data.host_actions[0]
            .instructions
            .iter()
            .any(|instruction| instruction.contains("response-local guidance")));
        assert!(data.dispatch.is_none());
        assert_eq!(data.prepared_state, "direct_guidance");
        assert_eq!(data.state_persisted, false);
        assert_eq!(data.persistence, "response_only");
        assert_eq!(
            data.durable_next_tool.as_deref(),
            Some("complete_backlog_item")
        );
        assert!(data.persistence_summary.contains("No task"));
        assert_eq!(data.selected_item_ids, vec!["PROJ-001".to_string()]);
        assert!(data.queue.is_none());

        let inspected = crate::guidance::inspect_work_queue(
            project.path(),
            crate::models::InspectWorkQueueParams {
                root: None,
                limit: Some(5),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let inspected = inspected.data.expect("queue");
        assert_eq!(inspected.items[0].queue_state, "direct_ready");
        assert!(inspected.items[0]
            .execution_guidance
            .contains("response-local"));
        assert!(inspected.items[0]
            .execution_guidance
            .contains("complete_backlog_item"));

        let verbose = prepare_work(
            project.path(),
            PrepareWorkParams {
                root: None,
                item_id: None,
                max_tasks: None,
                worker: None,
                claimant: None,
                execution_mode: None,
                require_task_plan: Some(false),
                require_planning_approval: None,
                auto_commit_artifacts: None,
                verification_command: Vec::new(),
                include_queue_snapshot: Some(true),
            },
        )
        .data
        .expect("verbose prepare data");
        assert!(verbose.queue.is_some());
    }

    #[test]
    fn prepare_work_reports_requested_item_blocker() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Feature workflow",
            "feature",
            "general",
            &["src/lib.rs"],
        );
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add blocked backlog"]);

        let result = prepare_work(
            project.path(),
            PrepareWorkParams {
                root: None,
                item_id: Some("PROJ-001".to_string()),
                max_tasks: None,
                worker: Some("coder".to_string()),
                claimant: Some("host".to_string()),
                execution_mode: None,
                require_task_plan: Some(true),
                require_planning_approval: None,
                auto_commit_artifacts: None,
                verification_command: Vec::new(),
                include_queue_snapshot: None,
            },
        );
        let data = result.data.expect("prepare data");
        let action = data.host_actions.first().expect("host action");

        assert_eq!(result.status, ActionStatus::Skipped);
        assert_eq!(data.prepared_state, "not_prepared");
        assert_eq!(data.selected_item_ids, vec!["PROJ-001".to_string()]);
        assert!(result
            .next_action
            .as_deref()
            .expect("next action")
            .contains("planning_blocked"));
        assert!(action.summary.contains("PROJ-001"));
        assert!(action.summary.contains("planning_blocked"));
        assert!(action.next_tools.contains(&"write_task_plan".to_string()));
    }

    #[test]
    fn complete_backlog_item_closes_direct_item_without_worker_task() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Readme docs",
            "docs",
            "docs",
            &["README.md"],
        );
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add direct backlog"]);
        fs::write(project.path().join("README.md"), "# Done\n").expect("readme");

        let completed = complete_backlog_item(
            project.path(),
            CompleteBacklogItemParams {
                root: None,
                item_id: "PROJ-001".to_string(),
                summary: "Updated the readme.".to_string(),
                changed_files: vec!["README.md".to_string()],
                verification_status: Some("skipped".to_string()),
                verification_summary: Some("Reviewed README.md manually.".to_string()),
                verification_refs: vec!["manual:readme".to_string()],
                evidence_refs: Vec::new(),
                finding_refs: Vec::new(),
                record_auto_evidence: None,
                commit: Some(false),
                commit_message: None,
                detail: Some("verbose".to_string()),
            },
        );
        let data = completed.data.expect("complete data");

        assert_eq!(completed.status, ActionStatus::Completed);
        assert!(data.closed);
        assert_eq!(data.closure.source, "runtime_event");
        assert!(data.closure.runtime_completion_recorded);
        assert!(!data.closure.git_trailer_portable);
        assert_eq!(data.commit_outcome.status, "not_requested");
        assert!(!data.commit_outcome.requested);
        assert!(data.queue_status.is_some());
        assert!(data.queue_status_error.is_none());
        assert_eq!(
            data.queue_status
                .as_ref()
                .expect("queue status")
                .counts
                .closed_count,
            1
        );
        assert_eq!(data.host_action.kind, "done");
        assert!(data.event.is_some());
        assert_eq!(data.evidence.len(), 2);
        assert!(data.auto_evidence_enabled);
        assert_eq!(data.generated_evidence.len(), 2);
        assert_eq!(data.generated_evidence[0].kind, "note");
        assert_eq!(data.generated_evidence[1].kind, "verification");
        let direct_done_guidance = data.host_action.instructions.join("\n");
        assert!(direct_done_guidance.contains("inspect_work_queue"));
        assert!(direct_done_guidance.contains("Run reconcile_project only when state is unclear"));

        let queue = guidance::inspect_work_queue(
            project.path(),
            crate::models::InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let queue_data = queue.data.expect("queue data");
        assert!(queue_data.items.is_empty());
    }

    #[test]
    fn complete_backlog_item_reports_commit_created_outcome() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Readme docs",
            "docs",
            "docs",
            &["README.md"],
        );
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add direct backlog"]);
        fs::write(project.path().join("README.md"), "# Done\n").expect("readme");

        let completed = complete_backlog_item(
            project.path(),
            CompleteBacklogItemParams {
                root: None,
                item_id: "PROJ-001".to_string(),
                summary: "Updated the readme.".to_string(),
                changed_files: vec!["README.md".to_string()],
                verification_status: Some("passed".to_string()),
                verification_summary: Some("Reviewed README.md manually.".to_string()),
                verification_refs: vec!["manual:readme".to_string()],
                evidence_refs: Vec::new(),
                finding_refs: Vec::new(),
                record_auto_evidence: None,
                commit: Some(true),
                commit_message: Some("Finish readme direct work".to_string()),
                detail: None,
            },
        );
        let data = completed.data.expect("complete data");

        assert_eq!(completed.status, ActionStatus::Completed);
        assert_eq!(data.commit_outcome.status, "created");
        assert!(data.commit_outcome.requested);
        assert!(data.commit_outcome.commit.is_some());
        assert_eq!(
            data.commit.as_deref(),
            data.commit_outcome.commit.as_deref()
        );
        assert_eq!(data.closure.source, "runtime_event_and_git_trailer");
        assert!(data.closure.git_trailer_portable);
        assert_eq!(
            data.closure.closure_commit.as_deref(),
            data.commit.as_deref()
        );

        let output = Command::new("git")
            .args(["log", "-1", "--format=%B"])
            .current_dir(project.path())
            .output()
            .expect("git log");
        assert!(output.status.success());
        let message = String::from_utf8_lossy(&output.stdout);
        assert!(message.contains("Platypus-Closes: PROJ-001"));
        assert!(message.contains("Platypus-Verification: Reviewed README.md manually."));
    }

    #[test]
    fn complete_backlog_item_reports_already_closed_skip_outcome() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Readme docs",
            "docs",
            "docs",
            &["README.md"],
        );
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add direct backlog"]);
        fs::write(project.path().join("README.md"), "# Done\n").expect("readme");

        let first = complete_backlog_item(
            project.path(),
            CompleteBacklogItemParams {
                root: None,
                item_id: "PROJ-001".to_string(),
                summary: "Updated the readme.".to_string(),
                changed_files: vec!["README.md".to_string()],
                verification_status: Some("skipped".to_string()),
                verification_summary: None,
                verification_refs: Vec::new(),
                evidence_refs: Vec::new(),
                finding_refs: Vec::new(),
                record_auto_evidence: None,
                commit: Some(false),
                commit_message: None,
                detail: None,
            },
        );
        assert_eq!(first.status, ActionStatus::Completed);

        let skipped = complete_backlog_item(
            project.path(),
            CompleteBacklogItemParams {
                root: None,
                item_id: "PROJ-001".to_string(),
                summary: "Already done.".to_string(),
                changed_files: vec!["README.md".to_string()],
                verification_status: Some("skipped".to_string()),
                verification_summary: None,
                verification_refs: Vec::new(),
                evidence_refs: Vec::new(),
                finding_refs: Vec::new(),
                record_auto_evidence: None,
                commit: Some(true),
                commit_message: None,
                detail: Some("verbose".to_string()),
            },
        );
        let data = skipped.data.expect("skip data");

        assert_eq!(skipped.status, ActionStatus::Skipped);
        assert_eq!(data.closure.source, "runtime_event");
        assert!(data.closure.runtime_completion_recorded);
        assert!(!data.closure.git_trailer_portable);
        assert_eq!(data.commit_outcome.status, "skipped");
        assert!(data.commit_outcome.requested);
        assert!(data.queue_status.is_some());
    }

    #[test]
    fn complete_backlog_item_preserves_git_trailer_closure_on_skip() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Readme docs",
            "docs",
            "docs",
            &["README.md"],
        );
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add direct backlog"]);
        git(
            project.path(),
            &[
                "commit",
                "--allow-empty",
                "-m",
                "Close direct item",
                "-m",
                "Platypus-Closes: PROJ-001\nPlatypus-Verification: checked",
            ],
        );

        let skipped = complete_backlog_item(
            project.path(),
            CompleteBacklogItemParams {
                root: None,
                item_id: "PROJ-001".to_string(),
                summary: "Already done.".to_string(),
                changed_files: vec!["README.md".to_string()],
                verification_status: Some("skipped".to_string()),
                verification_summary: None,
                verification_refs: Vec::new(),
                evidence_refs: Vec::new(),
                finding_refs: Vec::new(),
                record_auto_evidence: None,
                commit: Some(false),
                commit_message: None,
                detail: None,
            },
        );
        let data = skipped.data.expect("skip data");

        assert_eq!(skipped.status, ActionStatus::Skipped);
        assert_eq!(data.closure.source, "git_trailer");
        assert!(!data.closure.runtime_completion_recorded);
        assert!(data.closure.git_trailer_portable);
        assert_eq!(data.commit_outcome.status, "not_requested");
    }

    #[test]
    fn complete_backlog_item_can_disable_auto_evidence_with_explicit_refs() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Readme docs",
            "docs",
            "docs",
            &["README.md"],
        );
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add direct backlog"]);
        fs::write(project.path().join("README.md"), "# Done\n").expect("readme");

        let mut explicit = None;
        for index in 0..205 {
            explicit = Some(
                evidence::record_evidence(
                    project.path(),
                    RecordEvidenceParams {
                        root: None,
                        id: None,
                        source_item_id: Some("PROJ-001".to_string()),
                        source_task_id: None,
                        kind: "note".to_string(),
                        summary: format!("Explicit evidence #{index} covers the direct edit."),
                        refs: vec!["file:README.md".to_string()],
                        metadata: BTreeMap::new(),
                    },
                )
                .data
                .expect("explicit evidence")
                .evidence,
            );
        }
        let explicit = explicit.expect("explicit evidence");

        let completed = complete_backlog_item(
            project.path(),
            CompleteBacklogItemParams {
                root: None,
                item_id: "PROJ-001".to_string(),
                summary: "Updated the readme.".to_string(),
                changed_files: vec!["README.md".to_string()],
                verification_status: Some("skipped".to_string()),
                verification_summary: Some("Explicit evidence covers verification.".to_string()),
                verification_refs: vec!["manual:readme".to_string()],
                evidence_refs: vec![explicit.id.clone()],
                finding_refs: Vec::new(),
                record_auto_evidence: Some(false),
                commit: Some(false),
                commit_message: None,
                detail: None,
            },
        );
        let data = completed.data.expect("complete data");

        assert_eq!(completed.status, ActionStatus::Completed);
        assert!(data.closed);
        assert!(!data.auto_evidence_enabled);
        assert!(data.evidence.is_empty());
        assert!(data.generated_evidence.is_empty());
        let payload = data.event.expect("event").payload.expect("payload");
        assert_eq!(payload["auto_evidence_enabled"], false);
        assert_eq!(
            payload["generated_evidence_refs"].as_array().unwrap().len(),
            0
        );
        assert!(payload["evidence_refs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == &explicit.id));
    }

    #[test]
    fn complete_backlog_item_rejects_auto_evidence_opt_out_without_existing_evidence() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Readme docs",
            "docs",
            "docs",
            &["README.md"],
        );
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add direct backlog"]);
        fs::write(project.path().join("README.md"), "# Done\n").expect("readme");

        let completed = complete_backlog_item(
            project.path(),
            CompleteBacklogItemParams {
                root: None,
                item_id: "PROJ-001".to_string(),
                summary: "Updated the readme.".to_string(),
                changed_files: vec!["README.md".to_string()],
                verification_status: Some("skipped".to_string()),
                verification_summary: None,
                verification_refs: Vec::new(),
                evidence_refs: vec!["EVD-999".to_string()],
                finding_refs: Vec::new(),
                record_auto_evidence: Some(false),
                commit: Some(false),
                commit_message: None,
                detail: None,
            },
        );

        assert_eq!(completed.status, ActionStatus::Failed);
        assert!(completed
            .error
            .as_deref()
            .unwrap_or("")
            .contains("unknown: EVD-999"));
    }

    #[test]
    fn complete_backlog_item_warns_about_missing_or_unchanged_files() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Readme docs",
            "docs",
            "docs",
            &["README.md"],
        );
        fs::write(project.path().join("README.md"), "# Existing\n").expect("readme");
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add direct backlog"]);

        let completed = complete_backlog_item(
            project.path(),
            CompleteBacklogItemParams {
                root: None,
                item_id: "PROJ-001".to_string(),
                summary: "Reviewed the readme.".to_string(),
                changed_files: vec!["README.md".to_string(), "MISSING.md".to_string()],
                verification_status: Some("skipped".to_string()),
                verification_summary: Some("Manual review only.".to_string()),
                verification_refs: vec!["manual:review".to_string()],
                evidence_refs: Vec::new(),
                finding_refs: Vec::new(),
                record_auto_evidence: None,
                commit: Some(false),
                commit_message: None,
                detail: None,
            },
        );
        let data = completed.data.expect("complete data");

        assert_eq!(completed.status, ActionStatus::Completed);
        assert!(data
            .warnings
            .iter()
            .any(|warning| warning.contains("Git does not report local changes")));
        assert!(data
            .warnings
            .iter()
            .any(|warning| warning.contains("does not exist")));
        assert_eq!(data.compact.warnings, data.warnings);
    }

    #[test]
    fn complete_backlog_item_allows_verified_non_file_work_without_empty_warning() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Review workflow",
            "docs",
            "docs",
            &["README.md"],
        );
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add direct backlog"]);

        let completed = complete_backlog_item(
            project.path(),
            CompleteBacklogItemParams {
                root: None,
                item_id: "PROJ-001".to_string(),
                summary: "Reviewed the workflow without file changes.".to_string(),
                changed_files: Vec::new(),
                verification_status: Some("passed".to_string()),
                verification_summary: Some("Manual workflow review passed.".to_string()),
                verification_refs: vec!["manual:review".to_string()],
                evidence_refs: Vec::new(),
                finding_refs: Vec::new(),
                record_auto_evidence: None,
                commit: Some(false),
                commit_message: None,
                detail: None,
            },
        );
        let data = completed.data.expect("complete data");

        assert_eq!(completed.status, ActionStatus::Completed);
        assert!(!data
            .warnings
            .iter()
            .any(|warning| warning.contains("changed_files is empty")));
        assert!(!data
            .compact
            .warnings
            .iter()
            .any(|warning| warning.contains("changed_files is empty")));
    }

    #[test]
    fn complete_backlog_item_recognizes_untracked_changed_files() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Feedback docs",
            "docs",
            "docs",
            &["FEEDBACK.md"],
        );
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add direct backlog"]);
        fs::write(project.path().join("FEEDBACK.md"), "Useful feedback.\n").expect("feedback");

        let completed = complete_backlog_item(
            project.path(),
            CompleteBacklogItemParams {
                root: None,
                item_id: "PROJ-001".to_string(),
                summary: "Captured feedback.".to_string(),
                changed_files: vec!["FEEDBACK.md".to_string()],
                verification_status: Some("skipped".to_string()),
                verification_summary: Some("Manual review only.".to_string()),
                verification_refs: vec!["manual:review".to_string()],
                evidence_refs: Vec::new(),
                finding_refs: Vec::new(),
                record_auto_evidence: None,
                commit: Some(false),
                commit_message: None,
                detail: None,
            },
        );
        let data = completed.data.expect("complete data");

        assert_eq!(completed.status, ActionStatus::Completed);
        assert!(!data
            .warnings
            .iter()
            .any(|warning| warning.contains("Git does not report local changes")));
    }

    #[test]
    fn complete_backlog_item_does_not_warn_for_unchanged_platypus_files() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Backlog docs",
            "docs",
            "docs",
            &["backlog/items/PROJ-001.md"],
        );
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add direct backlog"]);

        let completed = complete_backlog_item(
            project.path(),
            CompleteBacklogItemParams {
                root: None,
                item_id: "PROJ-001".to_string(),
                summary: "Reviewed backlog metadata.".to_string(),
                changed_files: vec!["backlog/items/PROJ-001.md".to_string()],
                verification_status: Some("skipped".to_string()),
                verification_summary: Some("Manual review only.".to_string()),
                verification_refs: vec!["manual:review".to_string()],
                evidence_refs: Vec::new(),
                finding_refs: Vec::new(),
                record_auto_evidence: None,
                commit: Some(false),
                commit_message: None,
                detail: None,
            },
        );
        let data = completed.data.expect("complete data");

        assert_eq!(completed.status, ActionStatus::Completed);
        assert!(!data
            .warnings
            .iter()
            .any(|warning| warning.contains("Git does not report local changes")));
    }

    #[test]
    fn finish_work_guides_direct_callers_to_complete_backlog_item() {
        let project = backlog_project(false);
        let result = finish_work(
            project.path(),
            FinishWorkParams {
                root: None,
                item_id: Some("PROJ-001".to_string()),
                assignment_id: None,
                task_id: None,
                status: None,
                summary: "Finished direct docs.".to_string(),
                changed_files: vec!["README.md".to_string()],
                verification_status: Some("skipped".to_string()),
                verification_summary: None,
                verification_refs: Vec::new(),
                findings: Vec::new(),
                findings_reviewed: Some(true),
                auto_start_if_prepared: None,
                integrate_if_ready: None,
                allow_unverified: None,
                integration_strategy: None,
                cleanup_after: None,
            },
        );
        let data = result.data.expect("finish guidance");

        assert_eq!(result.status, ActionStatus::Skipped);
        assert!(result
            .next_action
            .as_deref()
            .expect("next action")
            .contains("complete_backlog_item"));
        assert!(data
            .host_action
            .next_tools
            .contains(&"complete_backlog_item".to_string()));
    }

    #[test]
    fn prepare_work_prepares_manual_handoff_without_local_worker_config() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Feature workflow",
            "feature",
            "general",
            &["src/lib.rs"],
        );
        write_plan(project.path(), "PROJ-001", "src/lib.rs");
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add standard backlog"]);

        let result = prepare_work(
            project.path(),
            PrepareWorkParams {
                root: None,
                item_id: None,
                max_tasks: None,
                worker: Some("coder".to_string()),
                claimant: Some("host".to_string()),
                execution_mode: None,
                require_task_plan: Some(true),
                require_planning_approval: None,
                auto_commit_artifacts: None,
                verification_command: Vec::new(),
                include_queue_snapshot: None,
            },
        );
        let data = result.data.expect("prepare data");
        let action = &data.host_actions[0];

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(action.kind, "run_in_worktree");
        assert!(action.assignment_id.is_some());
        assert!(action.worktree_path.is_some());
        assert_eq!(data.dispatch.as_ref().expect("dispatch").prepared, 1);
        let bundle = action.bundle.as_ref().expect("bundle");
        assert_eq!(bundle.completion_contract.required_fields[0], "status");
    }

    #[test]
    fn prepare_work_honors_requested_item_id() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "First feature workflow",
            "feature",
            "general",
            &["src/first.rs"],
        );
        write_item(
            project.path(),
            "PROJ-002",
            "Second feature workflow",
            "feature",
            "general",
            &["src/second.rs"],
        );
        write_plan(project.path(), "PROJ-002", "src/second.rs");
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add standard backlog"]);

        let result = prepare_work(
            project.path(),
            PrepareWorkParams {
                root: None,
                item_id: Some("PROJ-002".to_string()),
                max_tasks: None,
                worker: Some("coder".to_string()),
                claimant: Some("host".to_string()),
                execution_mode: None,
                require_task_plan: Some(true),
                require_planning_approval: None,
                auto_commit_artifacts: None,
                verification_command: Vec::new(),
                include_queue_snapshot: None,
            },
        );
        let action = result
            .data
            .expect("prepare data")
            .host_actions
            .into_iter()
            .next()
            .expect("host action");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(action.bundle.as_ref().expect("bundle").item_id, "PROJ-002");
    }

    #[test]
    fn prepare_work_honors_max_tasks_for_multiple_handoffs() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "First feature workflow",
            "feature",
            "general",
            &["src/first.rs"],
        );
        write_item(
            project.path(),
            "PROJ-002",
            "Second feature workflow",
            "feature",
            "general",
            &["src/second.rs"],
        );
        write_plan(project.path(), "PROJ-001", "src/first.rs");
        write_plan(project.path(), "PROJ-002", "src/second.rs");
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add standard backlog"]);

        let result = prepare_work(
            project.path(),
            PrepareWorkParams {
                root: None,
                item_id: None,
                max_tasks: Some(2),
                worker: Some("coder".to_string()),
                claimant: Some("host".to_string()),
                execution_mode: None,
                require_task_plan: Some(true),
                require_planning_approval: None,
                auto_commit_artifacts: None,
                verification_command: Vec::new(),
                include_queue_snapshot: None,
            },
        );
        let data = result.data.expect("prepare data");
        let item_ids = data
            .host_actions
            .iter()
            .filter_map(|action| action.bundle.as_ref().map(|bundle| bundle.item_id.clone()))
            .collect::<Vec<_>>();

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.host_actions.len(), 2);
        assert_eq!(data.dispatch.as_ref().expect("dispatch").prepared, 2);
        assert_eq!(
            item_ids,
            vec!["PROJ-001".to_string(), "PROJ-002".to_string()]
        );
    }

    #[test]
    fn prepare_work_reports_mixed_direct_and_worker_batches_without_single_next_tool() {
        let project = backlog_project(false);
        init_git(project.path());
        write_item(
            project.path(),
            "PROJ-001",
            "Document direct path",
            "docs",
            "docs",
            &["README.md"],
        );
        write_item(
            project.path(),
            "PROJ-002",
            "Build worker path",
            "feature",
            "general",
            &["src/worker.rs"],
        );
        write_plan(project.path(), "PROJ-002", "src/worker.rs");
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Add mixed backlog"]);

        let result = prepare_work(
            project.path(),
            PrepareWorkParams {
                root: None,
                item_id: None,
                max_tasks: Some(2),
                worker: Some("coder".to_string()),
                claimant: Some("host".to_string()),
                execution_mode: None,
                require_task_plan: Some(true),
                require_planning_approval: None,
                auto_commit_artifacts: None,
                verification_command: Vec::new(),
                include_queue_snapshot: None,
            },
        );
        let data = result.data.expect("prepare data");
        let kinds = data
            .host_actions
            .iter()
            .map(|action| action.kind.as_str())
            .collect::<Vec<_>>();

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.prepared_state, "mixed_prepared");
        assert_eq!(data.state_persisted, true);
        assert_eq!(data.persistence, "mixed_response_and_durable");
        assert_eq!(data.durable_next_tool, None);
        assert_eq!(
            data.selected_item_ids,
            vec!["PROJ-001".to_string(), "PROJ-002".to_string()]
        );
        assert_eq!(kinds, vec!["direct_edit", "run_in_worktree"]);
        assert!(data.host_actions[0]
            .next_tools
            .contains(&"complete_backlog_item".to_string()));
        assert!(data.host_actions[1]
            .next_tools
            .contains(&"finish_work".to_string()));
    }

    #[test]
    fn finish_work_records_completion_and_returns_verification_action() {
        let project = prepared_assignment_project();
        let prepared = prepare_work(
            project.path(),
            PrepareWorkParams {
                root: None,
                item_id: Some("PROJ-001".to_string()),
                max_tasks: None,
                worker: Some("coder".to_string()),
                claimant: Some("host".to_string()),
                execution_mode: None,
                require_task_plan: Some(true),
                require_planning_approval: None,
                auto_commit_artifacts: None,
                verification_command: Vec::new(),
                include_queue_snapshot: None,
            },
        );
        let action = prepared
            .data
            .expect("prepare data")
            .host_actions
            .into_iter()
            .next()
            .expect("host action");
        let worktree = action.worktree_path.expect("worktree");
        fs::write(
            format!("{worktree}/src/lib.rs"),
            "pub fn answer() -> u8 { 42 }\n",
        )
        .expect("worker edit");

        let result = finish_work(
            project.path(),
            FinishWorkParams {
                root: None,
                item_id: None,
                assignment_id: action.assignment_id,
                task_id: action.task_id,
                status: None,
                summary: "Implemented the feature slice.".to_string(),
                changed_files: Vec::new(),
                verification_status: Some("skipped".to_string()),
                verification_summary: None,
                verification_refs: Vec::new(),
                findings: Vec::new(),
                findings_reviewed: Some(true),
                auto_start_if_prepared: None,
                integrate_if_ready: Some(false),
                allow_unverified: None,
                integration_strategy: None,
                cleanup_after: None,
            },
        );
        let data = result.data.expect("finish data");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.host_action.kind, "verify_or_record_risk");
        assert_eq!(
            data.assignment.as_ref().expect("assignment").changed_files,
            vec!["src/lib.rs".to_string()]
        );
    }

    #[test]
    fn finish_work_does_not_route_zero_change_worker_result_to_integration() {
        let project = prepared_assignment_project();
        let prepared = prepare_work(
            project.path(),
            PrepareWorkParams {
                root: None,
                item_id: Some("PROJ-001".to_string()),
                max_tasks: None,
                worker: Some("coder".to_string()),
                claimant: Some("host".to_string()),
                execution_mode: None,
                require_task_plan: Some(true),
                require_planning_approval: None,
                auto_commit_artifacts: None,
                verification_command: Vec::new(),
                include_queue_snapshot: None,
            },
        );
        let action = prepared
            .data
            .expect("prepare data")
            .host_actions
            .into_iter()
            .next()
            .expect("host action");

        let result = finish_work(
            project.path(),
            FinishWorkParams {
                root: None,
                item_id: None,
                assignment_id: action.assignment_id,
                task_id: action.task_id,
                status: None,
                summary: "Completed a probe without file changes.".to_string(),
                changed_files: Vec::new(),
                verification_status: Some("passed".to_string()),
                verification_summary: Some("Probe completed successfully.".to_string()),
                verification_refs: vec!["manual:probe".to_string()],
                findings: Vec::new(),
                findings_reviewed: Some(true),
                auto_start_if_prepared: None,
                integrate_if_ready: Some(true),
                allow_unverified: None,
                integration_strategy: None,
                cleanup_after: None,
            },
        );
        let data = result.data.expect("finish data");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.host_action.kind, "done");
        assert!(data
            .host_action
            .summary
            .contains("no meaningful worktree changes"));
        assert!(!data
            .host_action
            .next_tools
            .contains(&"integrate_worker_result".to_string()));
        assert!(data.integration.is_none());
        assert!(
            !data
                .worktree_changes
                .as_ref()
                .expect("worktree changes")
                .meaningful_changes
        );
    }

    fn prepared_assignment_project() -> TempDir {
        let project = backlog_project(false);
        init_git(project.path());
        fs::create_dir_all(project.path().join("src")).expect("src");
        fs::write(
            project.path().join("src/lib.rs"),
            "pub fn answer() -> u8 { 0 }\n",
        )
        .expect("lib");
        write_item(
            project.path(),
            "PROJ-001",
            "Feature workflow",
            "feature",
            "general",
            &["src/lib.rs"],
        );
        write_plan(project.path(), "PROJ-001", "src/lib.rs");
        git(project.path(), &["add", "--all"]);
        git(project.path(), &["commit", "-m", "Initial project"]);
        project
    }

    fn backlog_project(with_config: bool) -> TempDir {
        let project = TempDir::new().expect("temp dir");
        if with_config {
            fs::write(project.path().join("platy.yaml"), "project: test\n").expect("config");
        }
        fs::create_dir_all(project.path().join("backlog/items")).expect("items");
        fs::create_dir_all(project.path().join("backlog/epics")).expect("epics");
        fs::write(
            project.path().join("backlog/epics/general.md"),
            r#"---
id: general
title: General
status: active
priority: P1
area: general
---

# General
"#,
        )
        .expect("epic");
        project
    }

    fn write_item(
        root: &Path,
        id: &str,
        title: &str,
        item_type: &str,
        area: &str,
        owned_surfaces: &[&str],
    ) {
        let surfaces = owned_surfaces
            .iter()
            .map(|surface| format!("- {surface}"))
            .collect::<Vec<_>>()
            .join("\n");
        let execution_policy = if item_type == "docs" {
            String::new()
        } else {
            "execution_path: worker_handoff\nplanning_gate: task_plan\n".to_string()
        };
        fs::write(
            root.join("backlog/items").join(format!("{id}.md")),
            format!(
                r#"---
id: {id}
title: {title}
priority: P1
type: {item_type}
area: {area}
epic: general
depends_on: []
owned_surfaces:
{surfaces}
{execution_policy}
---

# {id} {title}

## Goal

Deliver the requested work.

## Implementation Contract

Keep the change scoped to owned surfaces.

## Acceptance

- The work is complete.
"#,
            ),
        )
        .expect("item");
    }

    fn write_plan(root: &Path, item_id: &str, surface: &str) {
        fs::create_dir_all(root.join("backlog/plans")).expect("plans");
        fs::write(
            root.join("backlog/plans").join(format!("{item_id}.yaml")),
            format!(
                r#"item_id: {item_id}
version: 1
mode: standard
requirements:
  - id: R1
    text: Deliver the requested work.
design:
  summary: Focused implementation.
  owned_surfaces:
    - {surface}
  notes: null
tasks:
  - id: {item_id}-T001
    title: Implement {item_id}
    goal: Complete the requested work.
    requirement_refs:
      - R1
    depends_on: []
    owned_surfaces:
      - {surface}
    verification:
      - make check
    acceptance:
      - The work is implemented and verified.
    notes: null
"#,
            ),
        )
        .expect("plan");
    }

    fn init_git(root: &Path) {
        git(root, &["init"]);
        git(root, &["config", "user.name", "Platypus Test"]);
        git(root, &["config", "user.email", "platypus@example.invalid"]);
        fs::write(root.join("README.md"), "# Test\n").expect("readme");
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git command");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
