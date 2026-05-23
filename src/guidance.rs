use crate::{
    approvals, backlog, config, execution_policy,
    git_readiness::{inspect_git_readiness, GitReadinessStatus},
    models::{
        ActionResult, ActionStatus, BacklogCandidate, BacklogInventoryData, BacklogInventoryItem,
        BacklogItemMarkdownState, BacklogListData, DirectWorkLoop, DirectWorkStep,
        DoctorCheckStatus, DoctorSnapshotData, EffectiveExecutionPolicy, EvidenceRecord,
        FindingRecord, GetBacklogItemData, GetBacklogItemParams, InspectItemData,
        InspectItemParams, InspectQueueStatusParams, InspectSessionCompact, InspectSessionData,
        InspectSessionParams, InspectWorkQueueParams, LeaseRecord, PlanningApprovalState,
        PlanningClassification, QueueLeaseSummary, QueueStateDescription, QueueStatusCounts,
        QueueStatusData, QueueStatusItem, QueueTaskSummary, SchemaDiscoveryHint,
        SessionHealthSummary, TaskPlanQueryParams, WorkQueueData, WorkQueueInventorySummary,
        WorkQueueItem, WorkQueuePlanState, WorkflowConfigData, WorkflowConfigParams,
        WorkflowExecutionConfig,
    },
    project,
    storage::LeaseStore,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

const DIRECTION_SOURCE_GUIDANCE: &str = "Review durable direction in docs/product.md, docs/architecture.md, docs/testing.md, docs/roadmap.md, and docs/workflow.md when present; if direction is missing or stale, ask the user for product direction, review key workflow config defaults, summarize assumptions, and get approval before creating backlog items.";

pub fn inspect_session(
    default_root: &Path,
    params: InspectSessionParams,
) -> ActionResult<InspectSessionData> {
    let action = "inspect_session";
    let root_arg = params.root.clone();
    let detail = match normalize_session_detail(params.detail.as_deref()) {
        Ok(detail) => detail,
        Err(error) => return ActionResult::failed(action, "Could not inspect session.", error),
    };
    let mut root = root_arg.clone().unwrap_or_else(|| {
        default_root
            .canonicalize()
            .unwrap_or_else(|_| default_root.to_path_buf())
            .display()
            .to_string()
    });
    let mut errors = Vec::new();

    let doctor_result = project::doctor_snapshot(default_root, root_arg.as_deref());
    let doctor = match doctor_result {
        ActionResult {
            data: Some(data), ..
        } => {
            root = data.root.clone();
            Some(data)
        }
        ActionResult { summary, error, .. } => {
            errors.push(error.unwrap_or(summary));
            None
        }
    };

    let status_result = backlog::inspect_status(default_root, root_arg.as_deref(), params.limit);
    let status = match status_result {
        ActionResult {
            data: Some(data), ..
        } => Some(data),
        ActionResult { summary, error, .. } => {
            errors.push(error.unwrap_or(summary));
            None
        }
    };

    let workflow_result = config::inspect_workflow_config(
        default_root,
        WorkflowConfigParams {
            root: root_arg.clone(),
        },
    );
    let workflow = match workflow_result {
        ActionResult {
            data: Some(data), ..
        } => Some(data),
        ActionResult { summary, error, .. } => {
            errors.push(error.unwrap_or(summary));
            None
        }
    };

    let queue_result = inspect_work_queue(
        default_root,
        InspectWorkQueueParams {
            root: root_arg,
            limit: params.limit,
            require_task_plan: params.require_task_plan,
            require_planning_approval: params.require_planning_approval,
        },
    );
    let queue = match queue_result {
        ActionResult {
            data: Some(data), ..
        } => Some(data),
        ActionResult { summary, error, .. } => {
            errors.push(error.unwrap_or(summary));
            None
        }
    };

    let failed_doctor_check = doctor.as_ref().and_then(|snapshot| {
        snapshot
            .checks
            .iter()
            .find(|check| matches!(check.status, crate::models::DoctorCheckStatus::Fail))
    });
    let ok = errors.is_empty() && doctor.as_ref().is_none_or(|snapshot| snapshot.ok);
    let (recommended_tool, reason, params_map) = if let Some(check) = failed_doctor_check {
        let tool = if matches!(
            check.name.as_str(),
            "project_config" | "backlog_items" | "backlog_epics"
        ) {
            "init_project"
        } else {
            "doctor_snapshot"
        };
        (
            tool.to_string(),
            check
                .next_action
                .clone()
                .unwrap_or_else(|| check.summary.clone()),
            map_params([("root", root.as_str())]),
        )
    } else if let Some(queue) = queue.as_ref() {
        (
            queue.recommended_tool.clone(),
            queue.reason.clone(),
            queue.params.clone(),
        )
    } else if errors.is_empty() {
        (
            "inspect_work_queue".to_string(),
            "Inspect the work queue to choose the next deterministic workflow step.".to_string(),
            map_params([("root", root.as_str())]),
        )
    } else {
        (
            "doctor_snapshot".to_string(),
            "Resolve session inspection errors, then inspect the session again.".to_string(),
            map_params([("root", root.as_str())]),
        )
    };
    let summary = if ok {
        "Session inspected; use the recommended tool to continue.".to_string()
    } else if errors.is_empty() {
        "Session inspected with setup blockers.".to_string()
    } else {
        format!(
            "Session inspection collected {} non-fatal error(s).",
            errors.len()
        )
    };
    let recovery_action = (!ok).then(|| reason.clone());
    let schemas_likely_needed_next = queue
        .as_ref()
        .map(|queue| queue.schemas_likely_needed_next.clone())
        .unwrap_or_else(|| schema_hints_for_tools("session", [recommended_tool.as_str()]));
    let claude_toolsearch_batch_selector =
        claude_toolsearch_batch_selector(&schemas_likely_needed_next);
    let host_neutral_tool_search_query =
        host_neutral_tool_search_query(&schemas_likely_needed_next);
    let minimal_direct_loop = queue
        .as_ref()
        .and_then(|queue| queue.minimal_direct_loop.clone());
    let health = session_health(doctor.as_ref(), workflow.as_ref());
    let compact = InspectSessionCompact {
        root: root.clone(),
        ok,
        queue_state: queue.as_ref().map(|queue| queue.queue_state.clone()),
        total_items: queue.as_ref().map(|queue| queue.inventory.total_count),
        runnable_items: queue.as_ref().map(|queue| queue.inventory.runnable_count),
        health,
        recommended_tool: recommended_tool.clone(),
        reason: reason.clone(),
        minimal_direct_loop: minimal_direct_loop.clone(),
        errors: errors.clone(),
    };
    let (doctor, status, workflow, queue) = if detail == "verbose" {
        (doctor, status, workflow, queue)
    } else {
        (None, None, None, None)
    };
    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Completed,
        summary: summary.clone(),
        next_action: Some(reason.clone()),
        recovery_action,
        data: Some(InspectSessionData {
            root,
            ok,
            detail,
            compact,
            doctor,
            status,
            workflow,
            queue,
            errors,
            recommended_tool,
            summary,
            reason,
            params: params_map,
            minimal_direct_loop,
            claude_toolsearch_batch_selector,
            host_neutral_tool_search_query,
            schemas_likely_needed_next,
        }),
        error: None,
    }
}

fn normalize_session_detail(value: Option<&str>) -> Result<String, String> {
    match value
        .unwrap_or("compact")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "compact" => Ok("compact".to_string()),
        "verbose" => Ok("verbose".to_string()),
        value => Err(format!(
            "unsupported detail `{value}`; use compact or verbose"
        )),
    }
}

fn session_health(
    doctor: Option<&DoctorSnapshotData>,
    workflow: Option<&WorkflowConfigData>,
) -> SessionHealthSummary {
    let scaffold_ok = doctor
        .map(|doctor| {
            check_passed(doctor, "project_config")
                && check_passed(doctor, "backlog_items")
                && check_passed(doctor, "backlog_epics")
        })
        .unwrap_or(false);
    let git_ok = doctor
        .map(|doctor| check_passed(doctor, "git_readiness"))
        .unwrap_or(false);
    let backlog_ok = doctor
        .map(|doctor| check_passed(doctor, "backlog_items_count"))
        .unwrap_or(false);
    let workflow_ok = workflow.is_some();

    let scaffold = if scaffold_ok {
        "ready"
    } else {
        "needs_scaffold"
    };
    let git = if git_ok { "ready" } else { "needs_git" };
    let backlog = if backlog_ok {
        "ready"
    } else {
        "empty_or_missing"
    };
    let workflow_status = if workflow_ok {
        "ready"
    } else {
        "missing_or_invalid"
    };
    let summary = format!(
        "scaffold: {scaffold}; git: {git}; backlog: {backlog}; workflow: {workflow_status}"
    );

    SessionHealthSummary {
        scaffold_ok,
        git_ok,
        backlog_ok,
        workflow_ok,
        scaffold: scaffold.to_string(),
        git: git.to_string(),
        backlog: backlog.to_string(),
        workflow: workflow_status.to_string(),
        summary,
    }
}

fn check_passed(doctor: &DoctorSnapshotData, name: &str) -> bool {
    doctor
        .checks
        .iter()
        .any(|check| check.name == name && matches!(check.status, DoctorCheckStatus::Pass))
}

pub fn inspect_work_queue(
    default_root: &Path,
    params: InspectWorkQueueParams,
) -> ActionResult<WorkQueueData> {
    let action = "inspect_work_queue";
    let legacy_policy_warnings =
        legacy_policy_warnings(params.require_task_plan, params.require_planning_approval);
    let requested_limit = params.limit.unwrap_or(10).clamp(1, 200);
    let listed = backlog::list_backlog(default_root, params.root.as_deref(), Some(200));
    let (root, candidates) = match listed {
        ActionResult {
            status: ActionStatus::Completed | ActionStatus::Skipped,
            data: Some(BacklogListData { root, candidates }),
            ..
        } => (root, candidates),
        ActionResult { summary, error, .. } => {
            return ActionResult::failed(
                action,
                "Could not inspect executable work queue.",
                error.unwrap_or(summary),
            )
        }
    };

    let execution_config = match config::effective_execution_config(Path::new(&root)) {
        Ok(config) => config,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect executable work queue.", error)
        }
    };
    let dispatch_blockers = source_item_dispatch_blockers(Path::new(&root));
    let direct_claims = active_direct_claims(default_root, &root);
    let inventory = queue_inventory(default_root, &root, requested_limit, &dispatch_blockers);
    let mut active_item_ids = Vec::new();
    let mut active_task_ids = Vec::new();
    let mut active_lease_ids = Vec::new();
    let mut active_filtered = 0usize;
    let mut active_lease_filtered = 0usize;
    let mut items: Vec<WorkQueueItem> = candidates
        .into_iter()
        .take(requested_limit)
        .enumerate()
        .map(|(index, candidate)| {
            let mut item =
                work_queue_item(default_root, &root, index + 1, candidate, &execution_config);
            if let Some(blocker) = dispatch_blockers.get(&item.candidate.item_id) {
                active_filtered += 1;
                active_item_ids.push(item.candidate.item_id.clone());
                active_task_ids.push(blocker.task_id.clone());
                apply_dispatch_blocker(&mut item, blocker);
            } else if let Some(claim) = direct_claims.get(&item.candidate.item_id) {
                active_lease_filtered += 1;
                active_item_ids.push(item.candidate.item_id.clone());
                active_lease_ids.push(claim.id.clone());
                apply_direct_claim(&mut item, claim);
            }
            item
        })
        .collect();
    let mut preflight_warnings = Vec::new();
    let readiness = inspect_git_readiness(Path::new(&root), true);
    let mut dispatch_blocker: Option<(String, String)> = None;
    let mut dispatch_blocker_applies = false;
    if !readiness.ready() {
        if matches!(readiness.status, GitReadinessStatus::Dirty) {
            let artifact_only_dirty = manager_dirty_paths(Path::new(&root))
                .is_some_and(|paths| paths_are_backlog_artifacts(&paths));
            if artifact_only_dirty {
                if items.iter().any(item_requires_clean_workspace) {
                    preflight_warnings.push(
                        "Info: manager workspace has only backlog artifacts pending. Enable auto_commit_artifacts=true if you want dispatch_ready_work to auto-commit these artifacts."
                            .to_string(),
                    );
                }
            } else {
                let warning = readiness.next_action.clone().unwrap_or_else(|| {
                    "Commit, stash, or discard local changes before dispatch.".to_string()
                });
                let reason = format!(
                    "{warning} Dispatch will be blocked until the manager workspace is clean."
                );
                if items.iter().any(item_requires_clean_workspace) {
                    preflight_warnings.push(reason.clone());
                }
                dispatch_blocker = Some(("doctor_snapshot".to_string(), reason.clone()));
                for item in &mut items {
                    if item.ready_to_dispatch && item_requires_clean_workspace(item) {
                        dispatch_blocker_applies = true;
                        item.ready_to_dispatch = false;
                        item.queue_state = "workspace_blocked".to_string();
                        item.recommended_tool = "doctor_snapshot".to_string();
                        item.reason = "Planning is ready, but dispatch is currently blocked by local manager workspace changes.".to_string();
                        apply_execution_metadata(item);
                    }
                }
            }
        } else {
            let warning = readiness
                .next_action
                .clone()
                .unwrap_or_else(|| readiness.summary.clone());
            let reason = format!("{warning} Dispatch will be blocked until Git is ready.");
            preflight_warnings.push(reason.clone());
            dispatch_blocker = Some(("doctor_snapshot".to_string(), reason));
            for item in &mut items {
                if item.ready_to_dispatch && item_requires_clean_workspace(item) {
                    dispatch_blocker_applies = true;
                    item.ready_to_dispatch = false;
                    item.queue_state = "config_blocked".to_string();
                    item.recommended_tool = "doctor_snapshot".to_string();
                    item.reason = "Planning is ready, but dispatch is currently blocked because Git is not ready.".to_string();
                    apply_execution_metadata(item);
                }
            }
        }
    }
    let require_task_plan = items
        .iter()
        .any(|item| execution_policy::plan_required(&item.effective_policy.planning_gate));
    let ready_count = items.iter().filter(|item| item.ready_to_dispatch).count();
    let blocked_count = items.len().saturating_sub(ready_count);
    if active_filtered > 0 {
        preflight_warnings.push(format!(
            "{active_filtered} backlog item(s) already have active tasks or completed tasks awaiting integration."
        ));
    }
    if active_lease_filtered > 0 {
        preflight_warnings.push(format!(
            "{active_lease_filtered} direct backlog item(s) already have active task-scope leases."
        ));
    }
    let (mut recommended_tool, mut reason, mut params) = recommended_queue_action(&root, &items);
    if let Some((blocked_tool, blocked_reason)) =
        dispatch_blocker.filter(|_| dispatch_blocker_applies)
    {
        recommended_tool = blocked_tool;
        reason = blocked_reason;
        params = map_params([("root", root.as_str())]);
    } else if items.is_empty() && inventory.pending_integration_count > 0 {
        recommended_tool = "inspect_integration_gates".to_string();
        let task_id = inventory
            .pending_integration_task_ids
            .first()
            .cloned()
            .unwrap_or_default();
        reason = format!(
            "{} task(s) have completed worker output awaiting integration. Inspect integration gates before creating more backlog work.",
            inventory.pending_integration_count
        );
        params = map_params([("root", root.as_str())]);
        if !task_id.is_empty() {
            params.insert("task_id".to_string(), Value::String(task_id));
        }
    } else if items.is_empty()
        && (active_filtered > 0
            || active_lease_filtered > 0
            || inventory.active_lifecycle_count > 0)
    {
        recommended_tool = active_task_ids
            .first()
            .or_else(|| {
                dispatch_blockers
                    .values()
                    .find(|blocker| blocker.queue_state == "active")
                    .map(|blocker| &blocker.task_id)
            })
            .map(|_| "inspect_task")
            .unwrap_or_else(|| {
                if active_lease_ids.is_empty() {
                    "inspect_work_queue"
                } else {
                    "list_leases"
                }
            })
            .to_string();
        reason = if active_lease_filtered > 0 && active_task_ids.is_empty() {
            format!(
                "{active_lease_filtered} direct backlog item(s) already have active leases. Use list_leases, renew_lease, or release_lease before starting duplicate direct work."
            )
        } else {
            format!(
                "{} backlog item(s) already have active tasks. Use inspect_task to continue active work instead of creating new backlog items.",
                active_filtered.max(inventory.active_lifecycle_count)
            )
        };
        params = map_params([("root", root.as_str())]);
        if let Some(task_id) = active_task_ids.first().or_else(|| {
            dispatch_blockers
                .values()
                .find(|blocker| blocker.queue_state == "active")
                .map(|blocker| &blocker.task_id)
        }) {
            params.insert("task_id".to_string(), Value::String(task_id.clone()));
        } else if let Some(lease_id) = active_lease_ids.first() {
            params.insert("scope".to_string(), Value::String("task".to_string()));
            params.insert("status".to_string(), Value::String("active".to_string()));
            params.insert("lease_id".to_string(), Value::String(lease_id.clone()));
        }
    } else if items.is_empty() && inventory.dependency_blocked_count > 0 {
        recommended_tool = "inspect_item".to_string();
        let first_blocked = inventory
            .dependency_blocked_items
            .first()
            .map(|item| item.item_id.clone())
            .unwrap_or_default();
        reason = format!(
            "No runnable backlog items; {} item(s) are dependency-blocked. Inspect `{}` or close its dependencies before creating more work.",
            inventory.dependency_blocked_count, first_blocked
        );
        params = map_params([("root", root.as_str())]);
        if !first_blocked.is_empty() {
            params.insert("item_id".to_string(), Value::String(first_blocked));
        }
    } else if items.is_empty() && inventory.total_count > 0 && inventory.runnable_count == 0 {
        recommended_tool = "create_backlog_items".to_string();
        reason = format!(
            "All {} backlog item(s) are closed. {} Run project intake first: inspect docs, Git/scaffold state, and workflow config defaults; summarize known facts, assumptions, open questions, proposed first milestone, and things not to do yet; get user approval before calling create_backlog_items.",
            inventory.total_count, DIRECTION_SOURCE_GUIDANCE
        );
        params = map_params([("root", root.as_str())]);
    }
    let summary = if items.is_empty() {
        if active_filtered > 0 {
            format!(
                "No dispatchable backlog items; {active_filtered} backlog item(s) already have active tasks or completed tasks awaiting integration."
            )
        } else if inventory.dependency_blocked_count > 0 || inventory.closed_count > 0 {
            format!(
                "No runnable backlog items. Inventory: {} total, {} dependency-blocked, {} closed.",
                inventory.total_count, inventory.dependency_blocked_count, inventory.closed_count
            )
        } else {
            "No runnable backlog items.".to_string()
        }
    } else {
        let blocked_by_planning = items
            .iter()
            .filter(|item| {
                !item.ready_to_dispatch
                    && matches!(
                        item.queue_state.as_str(),
                        "planning_blocked"
                            | "approval_blocked"
                            | "config_blocked"
                            | "workspace_blocked"
                    )
            })
            .count();
        let blocked_by_lifecycle = blocked_count.saturating_sub(blocked_by_planning);
        let blocked_label = if blocked_by_lifecycle > 0 {
            format!(
                "{blocked_by_planning} blocked by planning or setup, {blocked_by_lifecycle} blocked by active lifecycle"
            )
        } else {
            format!("{blocked_count} blocked by planning or setup")
        };
        format!(
            "{} runnable backlog item(s) inspected: {} ready, {}.",
            items.len(),
            ready_count,
            blocked_label
        )
    };
    let status = if items.is_empty()
        && active_filtered == 0
        && inventory.total_count == 0
        && inventory.active_lifecycle_count == 0
        && inventory.pending_integration_count == 0
    {
        ActionStatus::Skipped
    } else {
        ActionStatus::Completed
    };
    let active_count = active_filtered + active_lease_filtered;
    let queue_state = overall_queue_state(&items, &inventory, active_count);
    let next_ready_item_id = next_ready_item_id(&items);
    let lifecycle_mode = queue_lifecycle_mode(&queue_state, &items, &inventory);
    let minimal_direct_loop = minimal_direct_loop_for_queue(&queue_state, &items);
    let schemas_likely_needed_next =
        schema_hints_for_queue(&queue_state, &recommended_tool, &items, &inventory);
    let claude_toolsearch_batch_selector =
        claude_toolsearch_batch_selector(&schemas_likely_needed_next);
    let host_neutral_tool_search_query =
        host_neutral_tool_search_query(&schemas_likely_needed_next);
    ActionResult {
        action: action.to_string(),
        status,
        summary: summary.clone(),
        next_action: Some(reason.clone()),
        recovery_action: None,
        data: Some(WorkQueueData {
            root,
            queue_state,
            require_task_plan,
            ready_count,
            next_ready_item_id,
            lifecycle_mode,
            blocked_count,
            active_count,
            active_item_ids,
            active_task_ids,
            active_lease_ids,
            inventory,
            preflight_warnings,
            policy_warnings: legacy_policy_warnings,
            recommended_tool,
            summary,
            reason,
            params,
            minimal_direct_loop,
            claude_toolsearch_batch_selector,
            host_neutral_tool_search_query,
            schemas_likely_needed_next,
            items,
        }),
        error: None,
    }
}

pub fn inspect_queue_status(
    default_root: &Path,
    params: InspectQueueStatusParams,
) -> ActionResult<QueueStatusData> {
    let action = "inspect_queue_status";
    let requested_limit = params.limit.unwrap_or(5).clamp(1, 50);
    let queue_result = inspect_work_queue(
        default_root,
        InspectWorkQueueParams {
            root: params.root,
            limit: Some(requested_limit),
            require_task_plan: None,
            require_planning_approval: None,
        },
    );
    let ActionResult {
        status,
        summary,
        next_action,
        data,
        error,
        ..
    } = queue_result;
    let queue = match data {
        Some(data) => data,
        None => {
            return ActionResult {
                action: action.to_string(),
                status,
                summary,
                next_action,
                recovery_action: None,
                data: None,
                error,
            };
        }
    };

    let dispatch_blockers = source_item_dispatch_blockers(Path::new(&queue.root));
    let data = compact_queue_status(&queue, requested_limit, &dispatch_blockers);
    ActionResult {
        action: action.to_string(),
        status,
        summary: data_summary(&data),
        next_action: Some(data.reason.clone()),
        recovery_action: None,
        data: Some(data),
        error,
    }
}

fn compact_queue_status(
    queue: &WorkQueueData,
    limit: usize,
    dispatch_blockers: &BTreeMap<String, QueueDispatchBlocker>,
) -> QueueStatusData {
    let mut blocked_item_ids = BTreeSet::new();
    for item in queue.items.iter().filter(|item| !item.ready_to_dispatch) {
        blocked_item_ids.insert(item.candidate.item_id.clone());
    }
    for item in &queue.inventory.dependency_blocked_items {
        blocked_item_ids.insert(item.item_id.clone());
    }
    for item_id in &queue.inventory.active_lifecycle_item_ids {
        blocked_item_ids.insert(item_id.clone());
    }
    for (item_id, blocker) in dispatch_blockers {
        if blocker.queue_state == "completed_pending_integration" {
            blocked_item_ids.insert(item_id.clone());
        }
    }
    let counts = QueueStatusCounts {
        total_count: queue.inventory.total_count,
        runnable_count: queue.inventory.runnable_count,
        ready_count: queue.ready_count,
        blocked_count: blocked_item_ids.len(),
        dependency_blocked_count: queue.inventory.dependency_blocked_count,
        active_count: queue.active_count,
        pending_integration_count: queue.inventory.pending_integration_count,
        closed_count: queue.inventory.closed_count,
        closed_with_evidence_count: queue.inventory.closed_with_evidence_count,
        closed_without_evidence_count: queue.inventory.closed_without_evidence_count,
    };
    let top_ready_items = queue
        .items
        .iter()
        .filter(|item| item.ready_to_dispatch)
        .take(limit)
        .map(queue_status_item_from_work_item)
        .collect::<Vec<_>>();

    let mut seen_blocked = BTreeSet::new();
    let mut top_blocked_items = Vec::new();
    for item in queue.items.iter().filter(|item| !item.ready_to_dispatch) {
        seen_blocked.insert(item.candidate.item_id.clone());
        top_blocked_items.push(queue_status_item_from_work_item(item));
        if top_blocked_items.len() >= limit {
            break;
        }
    }
    if top_blocked_items.len() < limit {
        for item in &queue.inventory.dependency_blocked_items {
            if seen_blocked.insert(item.item_id.clone()) {
                top_blocked_items.push(queue_status_item_from_inventory_item(item));
            }
            if top_blocked_items.len() >= limit {
                break;
            }
        }
    }

    let active_tasks = compact_active_tasks(queue, dispatch_blockers, limit);
    let active_leases = compact_active_leases(queue, limit);
    let truncated = queue.items.len() > limit
        || queue.inventory.truncated
        || queue.ready_count > top_ready_items.len()
        || counts.blocked_count > top_blocked_items.len()
        || queue.active_task_ids.len() > active_tasks.len()
        || queue.active_lease_ids.len() > active_leases.len()
        || queue.inventory.pending_integration_task_ids.len()
            > active_tasks
                .iter()
                .filter(|task| task.queue_state == "completed_pending_integration")
                .count();
    QueueStatusData {
        root: queue.root.clone(),
        queue_state: queue.queue_state.clone(),
        counts,
        next_ready_item_id: queue.next_ready_item_id.clone(),
        lifecycle_mode: queue.lifecycle_mode.clone(),
        top_ready_items,
        top_blocked_items,
        active_tasks,
        active_leases,
        state_descriptions: queue_state_descriptions(),
        preflight_warnings: queue.preflight_warnings.clone(),
        recommended_tool: queue.recommended_tool.clone(),
        reason: queue.reason.clone(),
        minimal_direct_loop: queue.minimal_direct_loop.clone(),
        claude_toolsearch_batch_selector: queue.claude_toolsearch_batch_selector.clone(),
        host_neutral_tool_search_query: queue.host_neutral_tool_search_query.clone(),
        truncated,
    }
}

fn data_summary(data: &QueueStatusData) -> String {
    if data.counts.total_count == 0 {
        "No backlog items.".to_string()
    } else {
        format!(
            "{} total backlog item(s): {} runnable, {} ready, {} blocked, {} active, {} pending integration, {} closed.",
            data.counts.total_count,
            data.counts.runnable_count,
            data.counts.ready_count,
            data.counts.blocked_count,
            data.counts.active_count,
            data.counts.pending_integration_count,
            data.counts.closed_count
        )
    }
}

fn schema_hints_for_queue(
    queue_state: &str,
    recommended_tool: &str,
    items: &[WorkQueueItem],
    inventory: &WorkQueueInventorySummary,
) -> Vec<SchemaDiscoveryHint> {
    let phase = queue_state.to_string();
    let mut tools = match queue_state {
        "empty_backlog" => vec!["create_backlog_items", "inspect_work_queue"],
        "direct_ready" => vec![
            "complete_backlog_item",
            "record_verification_evidence",
            "prepare_work",
        ],
        "ready" => vec!["prepare_work", "finish_work", "inspect_task_events"],
        "planning_blocked" => vec!["write_task_plan", "validate_task_plan", "inspect_item"],
        "approval_blocked" => vec!["request_planning_approval", "approval_respond"],
        "config_blocked" => vec!["doctor_snapshot", "init_project"],
        "workspace_blocked" => vec!["doctor_snapshot", "inspect_work_queue"],
        "active" => vec!["inspect_task", "inspect_task_events", "finish_work"],
        "completed_pending_integration" => vec![
            "inspect_integration_gates",
            "integrate_worker_result",
            "reconcile_project",
        ],
        _ if inventory.pending_integration_count > 0 => vec![
            "inspect_integration_gates",
            "integrate_worker_result",
            "reconcile_project",
        ],
        _ if inventory.total_count > 0 && items.is_empty() => {
            vec!["inspect_item", "get_backlog_item", "create_backlog_items"]
        }
        _ => vec![recommended_tool],
    };
    if let Some(index) = tools.iter().position(|tool| *tool == recommended_tool) {
        let tool = tools.remove(index);
        tools.insert(0, tool);
    } else {
        tools.insert(0, recommended_tool);
    }
    schema_hints_for_tools(&phase, tools)
}

fn schema_hints_for_tools<'a>(
    phase: impl Into<String>,
    tools: impl IntoIterator<Item = &'a str>,
) -> Vec<SchemaDiscoveryHint> {
    let phase = phase.into();
    let mut seen = BTreeSet::new();
    tools
        .into_iter()
        .filter(|tool| !tool.trim().is_empty())
        .filter(|tool| seen.insert((*tool).to_string()))
        .take(4)
        .map(|tool| SchemaDiscoveryHint {
            tool_name: tool.to_string(),
            phase: phase.clone(),
            usage: schema_hint_usage(tool, &phase).to_string(),
            reason: schema_hint_reason(tool, &phase),
            host_neutral_query: format!("platypus tool {tool}"),
            codex_tool_search_query: format!("mcp__platypus__{tool} platypus {tool}"),
            claude_toolsearch_selector: Some(format!("select:mcp__platypus__{tool}")),
        })
        .collect()
}

fn schema_hint_usage(tool: &str, phase: &str) -> &'static str {
    match (phase, tool) {
        ("direct_ready", "complete_backlog_item") => "required",
        ("direct_ready", "record_verification_evidence" | "prepare_work") => "optional",
        ("empty_backlog", "create_backlog_items") => "post_intake",
        ("empty_backlog", "inspect_work_queue") => "optional",
        ("planning_blocked", "write_task_plan" | "validate_task_plan") => "required",
        ("planning_blocked", "inspect_item") => "optional",
        ("approval_blocked", "request_planning_approval" | "approval_respond") => "required",
        ("config_blocked" | "workspace_blocked", _) => "recovery",
        ("completed_pending_integration", _) => "required",
        (_, "doctor_snapshot" | "reconcile_project") => "recovery",
        (_, "inspect_work_queue" | "inspect_item" | "inspect_task" | "inspect_task_events") => {
            "optional"
        }
        _ => "required",
    }
}

fn host_neutral_tool_search_query(hints: &[SchemaDiscoveryHint]) -> Option<String> {
    let mut tools = hints
        .iter()
        .map(|hint| hint.tool_name.as_str())
        .filter(|tool| !tool.trim().is_empty())
        .collect::<Vec<_>>();
    if tools.is_empty() {
        return None;
    }
    tools.dedup();
    Some(format!("platypus tools {}", tools.join(" ")))
}

fn claude_toolsearch_batch_selector(hints: &[SchemaDiscoveryHint]) -> Option<String> {
    let mut selectors = hints
        .iter()
        .filter_map(|hint| hint.claude_toolsearch_selector.as_deref())
        .map(|selector| selector.strip_prefix("select:").unwrap_or(selector))
        .filter(|selector| !selector.trim().is_empty())
        .collect::<Vec<_>>();
    if selectors.is_empty() {
        return None;
    }
    selectors.dedup();
    Some(format!("select:{}", selectors.join(",")))
}

fn schema_hint_reason(tool: &str, phase: &str) -> String {
    match tool {
        "create_backlog_items" => {
            "Create concrete backlog items only after project intake summarizes known facts, assumptions, workflow config defaults, open questions, and the user approves durable artifact creation."
                .to_string()
        }
        "validate_backlog" => {
            "Validate backlog markdown after manual edits or when an explicit audit result is needed."
                .to_string()
        }
        "complete_backlog_item" => {
            "Close direct manager-workspace work with summary, changed files, and verification evidence."
                .to_string()
        }
        "record_verification_evidence" => {
            "Optional for direct completion: record reusable verification evidence separately when the evidence should outlive the completion summary.".to_string()
        }
        "prepare_work" => {
            "Optional for direct_ready items; required only when preparing a worker handoff."
                .to_string()
        }
        "write_task_plan" => {
            "Write the required task plan before worker handoff or gated execution.".to_string()
        }
        "validate_task_plan" => "Validate a task plan before execution.".to_string(),
        "inspect_item" => "Inspect one backlog item and its blockers.".to_string(),
        "request_planning_approval" => {
            "Create an explicit approval request for gated planning.".to_string()
        }
        "approval_respond" => "Approve or deny a pending planning request.".to_string(),
        "doctor_snapshot" => "Diagnose setup, Git, and workflow blockers.".to_string(),
        "init_project" => "Create missing Platypus project scaffold files only after explicit user confirmation because this mutates the repository.".to_string(),
        "inspect_work_queue" => "Refresh queue state after resolving a blocker.".to_string(),
        "inspect_task" => "Inspect an active worker task lifecycle.".to_string(),
        "inspect_task_events" => "Replay activity for an active or completed task.".to_string(),
        "finish_work" => "Finish a host-managed worker assignment and collect result state.".to_string(),
        "inspect_integration_gates" => {
            "Inspect completed worker task gates before integration.".to_string()
        }
        "integrate_worker_result" => "Integrate a completed worker result into the manager workspace.".to_string(),
        "reconcile_project" => "Audit project state after integration or unclear workflow state.".to_string(),
        other => format!("Likely next tool for queue phase `{phase}`: `{other}`."),
    }
}

fn minimal_direct_loop_for_queue(
    queue_state: &str,
    items: &[WorkQueueItem],
) -> Option<DirectWorkLoop> {
    if queue_state != "direct_ready" {
        return None;
    }
    items
        .iter()
        .find(|item| item.ready_to_dispatch && item.queue_state == "direct_ready")
        .map(|item| direct_work_loop(Some(item.candidate.item_id.as_str())))
}

fn next_ready_item_id(items: &[WorkQueueItem]) -> Option<String> {
    items
        .iter()
        .find(|item| item.ready_to_dispatch)
        .map(|item| item.candidate.item_id.clone())
}

fn queue_lifecycle_mode(
    queue_state: &str,
    items: &[WorkQueueItem],
    inventory: &WorkQueueInventorySummary,
) -> String {
    if inventory.total_count == 0 {
        return "empty".to_string();
    }
    if queue_state == "closed" {
        return "closed".to_string();
    }
    if let Some(item) = items.iter().find(|item| item.ready_to_dispatch) {
        return item.lifecycle_mode.clone();
    }
    lifecycle_mode_for_state(queue_state, None)
}

fn item_lifecycle_mode(queue_state: &str, policy: &EffectiveExecutionPolicy) -> String {
    lifecycle_mode_for_state(queue_state, Some(policy))
}

fn lifecycle_mode_for_state(
    queue_state: &str,
    policy: Option<&EffectiveExecutionPolicy>,
) -> String {
    match queue_state {
        "empty_backlog" => "empty",
        "closed" => "closed",
        "direct_ready" => "simple_direct",
        "ready" => "worker_handoff",
        "active" => "active",
        "completed_pending_integration" => "integration",
        _ => {
            if matches!(
                policy.map(|policy| policy.execution_path.as_str()),
                Some("worker_handoff")
            ) {
                "worker_handoff"
            } else {
                "blocked"
            }
        }
    }
    .to_string()
}

fn direct_work_loop(item_id: Option<&str>) -> DirectWorkLoop {
    DirectWorkLoop {
        item_id: item_id.map(str::to_string),
        summary: "Direct work loop: inspect state, edit the manager workspace, verify, then close with complete_backlog_item.".to_string(),
        primary_next_tool: "complete_backlog_item".to_string(),
        optional_guidance_tool: Some("prepare_work".to_string()),
        steps: vec![
            DirectWorkStep {
                order: 1,
                phase: "inspect".to_string(),
                tool: Some("inspect_work_queue".to_string()),
                summary: "Confirm the item is direct_ready and note its acceptance criteria.".to_string(),
            },
            DirectWorkStep {
                order: 2,
                phase: "edit".to_string(),
                tool: None,
                summary: "Edit the manager workspace directly; no task, assignment, or worktree is required.".to_string(),
            },
            DirectWorkStep {
                order: 3,
                phase: "verify".to_string(),
                tool: None,
                summary: "Run the relevant checks. Put verification_status, verification_summary, and verification_refs into complete_backlog_item; call record_verification_evidence only when reusable standalone evidence is useful.".to_string(),
            },
            DirectWorkStep {
                order: 4,
                phase: "complete".to_string(),
                tool: Some("complete_backlog_item".to_string()),
                summary: "Close the backlog item with summary, changed_files, and verification_status.".to_string(),
            },
        ],
    }
}

fn queue_status_item_from_work_item(item: &WorkQueueItem) -> QueueStatusItem {
    QueueStatusItem {
        item_id: item.candidate.item_id.clone(),
        title: item.candidate.title.clone(),
        priority: item.candidate.priority.clone(),
        area: item.candidate.area.clone(),
        queue_state: item.queue_state.clone(),
        lifecycle_mode: item.lifecycle_mode.clone(),
        state_description: queue_state_description(&item.queue_state).to_string(),
        recommended_tool: item.recommended_tool.clone(),
        reason: item.reason.clone(),
    }
}

fn queue_status_item_from_inventory_item(item: &BacklogInventoryItem) -> QueueStatusItem {
    QueueStatusItem {
        item_id: item.item_id.clone(),
        title: item.title.clone(),
        priority: item.priority.clone(),
        area: item.area.clone(),
        queue_state: "dependency_blocked".to_string(),
        lifecycle_mode: "blocked".to_string(),
        state_description: queue_state_description("dependency_blocked").to_string(),
        recommended_tool: "inspect_item".to_string(),
        reason: if item.open_dependencies.is_empty() {
            "Inspect this backlog item before choosing an execution path.".to_string()
        } else {
            format!(
                "Blocked by open dependencies: {}.",
                item.open_dependencies.join(", ")
            )
        },
    }
}

fn overall_queue_state(
    items: &[WorkQueueItem],
    inventory: &WorkQueueInventorySummary,
    active_filtered: usize,
) -> String {
    if let Some(first) = items.first() {
        if first.ready_to_dispatch
            && items
                .iter()
                .filter(|item| item.ready_to_dispatch)
                .all(|item| item.queue_state == "direct_ready")
        {
            "direct_ready".to_string()
        } else if first.ready_to_dispatch {
            "ready".to_string()
        } else {
            first.queue_state.clone()
        }
    } else if inventory.pending_integration_count > 0 {
        "completed_pending_integration".to_string()
    } else if active_filtered > 0 || inventory.active_lifecycle_count > 0 {
        "active".to_string()
    } else if inventory.total_count == 0 {
        "empty_backlog".to_string()
    } else if inventory.dependency_blocked_count > 0 {
        "dependency_blocked".to_string()
    } else {
        "closed".to_string()
    }
}

fn compact_active_tasks(
    queue: &WorkQueueData,
    dispatch_blockers: &BTreeMap<String, QueueDispatchBlocker>,
    limit: usize,
) -> Vec<QueueTaskSummary> {
    let mut tasks = Vec::new();
    let mut seen = BTreeSet::new();
    for item in &queue.items {
        if let Some(task_id) = &item.task_id {
            if seen.insert(task_id.clone()) {
                tasks.push(QueueTaskSummary {
                    item_id: Some(item.candidate.item_id.clone()),
                    task_id: task_id.clone(),
                    queue_state: item.queue_state.clone(),
                    recommended_tool: item.recommended_tool.clone(),
                });
            }
        }
        if tasks.len() >= limit {
            return tasks;
        }
    }
    for (item_id, blocker) in dispatch_blockers {
        if seen.insert(blocker.task_id.clone()) {
            tasks.push(QueueTaskSummary {
                item_id: Some(item_id.clone()),
                task_id: blocker.task_id.clone(),
                queue_state: blocker.queue_state.clone(),
                recommended_tool: task_summary_tool(blocker).to_string(),
            });
        }
        if tasks.len() >= limit {
            return tasks;
        }
    }
    for (index, task_id) in queue.active_task_ids.iter().enumerate() {
        if seen.insert(task_id.clone()) {
            tasks.push(QueueTaskSummary {
                item_id: queue.active_item_ids.get(index).cloned(),
                task_id: task_id.clone(),
                queue_state: "active".to_string(),
                recommended_tool: "inspect_task".to_string(),
            });
        }
        if tasks.len() >= limit {
            return tasks;
        }
    }
    for task_id in &queue.inventory.pending_integration_task_ids {
        if seen.insert(task_id.clone()) {
            tasks.push(QueueTaskSummary {
                item_id: None,
                task_id: task_id.clone(),
                queue_state: "completed_pending_integration".to_string(),
                recommended_tool: "inspect_integration_gates".to_string(),
            });
        }
        if tasks.len() >= limit {
            break;
        }
    }
    tasks
}

fn compact_active_leases(queue: &WorkQueueData, limit: usize) -> Vec<QueueLeaseSummary> {
    queue
        .items
        .iter()
        .filter_map(|item| {
            item.active_lease_id
                .as_ref()
                .map(|lease_id| QueueLeaseSummary {
                    item_id: item.candidate.item_id.clone(),
                    lease_id: lease_id.clone(),
                    owner: item
                        .active_lease_owner
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    queue_state: item.queue_state.clone(),
                    recommended_tool: item.recommended_tool.clone(),
                })
        })
        .take(limit)
        .collect()
}

fn task_summary_tool(blocker: &QueueDispatchBlocker) -> &'static str {
    match blocker.queue_state.as_str() {
        "completed_pending_integration" => "inspect_integration_gates",
        "active" if blocker.assignment_status.as_deref() == Some("prepared") => "start_worker_task",
        "active" if blocker.assignment_status.as_deref() == Some("running") => {
            "record_worker_progress"
        }
        "active" if matches!(blocker.task_status.as_str(), "queued" | "claimed") => {
            "prepare_worker_handoff"
        }
        _ => "inspect_task",
    }
}

fn queue_state_descriptions() -> Vec<QueueStateDescription> {
    [
        "empty_backlog",
        "direct_ready",
        "ready",
        "planning_blocked",
        "approval_blocked",
        "dependency_blocked",
        "config_blocked",
        "workspace_blocked",
        "active",
        "completed_pending_integration",
        "closed",
    ]
    .into_iter()
    .map(|queue_state| QueueStateDescription {
        queue_state: queue_state.to_string(),
        description: queue_state_description(queue_state).to_string(),
    })
    .collect()
}

fn queue_state_description(queue_state: &str) -> &'static str {
    match queue_state {
        "empty_backlog" => "No backlog items exist yet; run project intake, review workflow config defaults, ask clarifying questions, and get approval before creating backlog items.",
        "direct_ready" => "Ready for host-managed direct edits in the current workspace.",
        "ready" => "Ready for a worker handoff in an isolated worktree.",
        "planning_blocked" => "Requires a valid task plan before work can start.",
        "approval_blocked" => "Requires planning approval before work can start.",
        "dependency_blocked" => "Blocked until listed backlog dependencies are closed.",
        "config_blocked" => "Project setup blocks dispatch until doctor guidance is resolved.",
        "workspace_blocked" => {
            "Manager workspace changes must be handled before worktree dispatch."
        }
        "active" => {
            "An existing task lifecycle must continue before this item can be dispatched again."
        }
        "completed_pending_integration" => "Worker output is complete and awaiting integration.",
        "closed" => {
            "All known backlog items are closed; inspect durable direction and ask the user for missing direction before creating follow-up work."
        },
        _ => "Inspect the item before choosing the next execution step.",
    }
}

pub fn get_backlog_item(
    default_root: &Path,
    params: GetBacklogItemParams,
) -> ActionResult<GetBacklogItemData> {
    let action = "get_backlog_item";
    let include_markdown = params.include_markdown.unwrap_or(true);
    let max_markdown_bytes = params
        .max_markdown_bytes
        .unwrap_or(20_000)
        .clamp(1, 100_000);
    let (root_path, snapshot) =
        match backlog::backlog_item_snapshot(default_root, params.root.as_deref(), &params.item_id)
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return ActionResult::failed(
                    action,
                    "Could not read backlog item.",
                    format!("{error}. Use inspect_work_queue to list available item ids."),
                )
            }
        };
    let root_canonical = match root_path.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not read backlog item.",
                format!("could not resolve project root: {error}"),
            )
        }
    };
    let item_canonical = match snapshot.path.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not read backlog item.",
                format!("could not resolve backlog item path: {error}"),
            )
        }
    };
    if !item_canonical.starts_with(&root_canonical) {
        return ActionResult::failed(
            action,
            "Could not read backlog item.",
            "backlog item path resolves outside the project root",
        );
    }

    let markdown_text = if include_markdown {
        match fs::read_to_string(&item_canonical) {
            Ok(markdown) => Some(markdown),
            Err(error) => {
                return ActionResult::failed(
                    action,
                    "Could not read backlog item.",
                    format!("could not read backlog item markdown: {error}"),
                )
            }
        }
    } else {
        None
    };
    let markdown_bytes = markdown_text.as_ref().map_or(0, |text| text.len());
    let (markdown, markdown_truncated) = markdown_text
        .as_deref()
        .map(|text| truncate_utf8_owned(text, max_markdown_bytes))
        .unwrap_or((None, false));
    let summary = format!("Read backlog item `{}`.", snapshot.id);
    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Completed,
        summary: summary.clone(),
        next_action: Some(
            "Use inspect_item only when queue state, findings, evidence, or task-plan context is needed."
                .to_string(),
        ),
        recovery_action: None,
        data: Some(GetBacklogItemData {
            root: root_path.display().to_string(),
            item_id: snapshot.id,
            title: snapshot.title,
            path: snapshot.path.display().to_string(),
            sections: snapshot.sections,
            external_refs: snapshot.external_refs,
            markdown,
            markdown_truncated,
            markdown_bytes,
            max_markdown_bytes,
        }),
        error: None,
    }
}

fn truncate_utf8_owned(text: &str, max_bytes: usize) -> (Option<String>, bool) {
    if text.len() <= max_bytes {
        return (Some(text.to_string()), false);
    }
    let end = text
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= max_bytes)
        .last()
        .unwrap_or(0);
    (Some(text[..end].to_string()), true)
}

pub fn inspect_item(
    default_root: &Path,
    params: InspectItemParams,
) -> ActionResult<InspectItemData> {
    let action = "inspect_item";
    let _legacy_policy_warnings =
        legacy_policy_warnings(params.require_task_plan, params.require_planning_approval);
    let inventory_result =
        backlog::inspect_backlog_inventory(default_root, params.root.as_deref(), None);
    let BacklogInventoryData { root, items, .. } = match inventory_result {
        ActionResult {
            status: ActionStatus::Completed,
            data: Some(data),
            ..
        } => data,
        ActionResult { summary, error, .. } => {
            return ActionResult::failed(
                action,
                "Could not inspect backlog item.",
                error.unwrap_or(summary),
            )
        }
    };
    let available_ids = items
        .iter()
        .map(|item| item.item_id.as_str())
        .take(20)
        .collect::<Vec<_>>()
        .join(", ");
    let Some(item) = items
        .into_iter()
        .find(|item| item.item_id == params.item_id)
    else {
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Failed,
            summary: format!("Backlog item `{}` was not found.", params.item_id),
            next_action: Some(if available_ids.is_empty() {
                "Create backlog items before inspecting one.".to_string()
            } else {
                format!("Use one of the existing item ids: {available_ids}.")
            }),
            recovery_action: None,
            data: None,
            error: Some(format!("unknown backlog item `{}`", params.item_id)),
        };
    };
    let (_root_path, snapshot) =
        match backlog::backlog_item_snapshot(default_root, Some(&root), &item.item_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return ActionResult::failed(action, "Could not inspect backlog item.", error)
            }
        };
    let dispatch_blockers = source_item_dispatch_blockers(Path::new(&root));
    let blocker = dispatch_blockers.get(&item.item_id);
    let candidate = candidate_from_item(&item, &snapshot.external_refs);
    let execution_config = match config::effective_execution_config(Path::new(&root)) {
        Ok(config) => config,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect backlog item.", error)
        }
    };
    let mut queue = if item.runnable || blocker.is_some() {
        let mut queue_item =
            work_queue_item(default_root, &root, 1, candidate.clone(), &execution_config);
        if let Some(blocker) = blocker {
            apply_dispatch_blocker(&mut queue_item, blocker);
        }
        Some(queue_item)
    } else {
        None
    };
    if item.closed {
        queue = None;
    }

    let planning = queue
        .as_ref()
        .map(|item| item.planning.clone())
        .unwrap_or_else(|| {
            let policy = effective_policy_for_candidate(
                default_root,
                &root,
                &candidate.item_id,
                &execution_config,
            );
            planning_requirement(&candidate, &policy)
        });
    let plan = queue
        .as_ref()
        .map(|item| item.plan.clone())
        .unwrap_or_else(|| {
            let policy = effective_policy_for_candidate(
                default_root,
                &root,
                &item.item_id,
                &execution_config,
            );
            normalized_task_plan_state(
                default_root,
                &root,
                &item.item_id,
                execution_policy::plan_required(&policy.planning_gate),
            )
        });
    let planning_approval = queue
        .as_ref()
        .and_then(|item| item.planning_approval.clone())
        .or_else(|| {
            let policy = effective_policy_for_candidate(
                default_root,
                &root,
                &item.item_id,
                &execution_config,
            );
            execution_policy::approval_required(&policy.planning_gate).then(|| {
                approvals::planning_approval_state(default_root, Some(&root), &item.item_id, true)
                    .unwrap_or_else(|error| PlanningApprovalState {
                        item_id: item.item_id.clone(),
                        required: true,
                        approved: false,
                        approval_id: None,
                        status: None,
                        reason: format!("Could not inspect planning approval state: {error}"),
                    })
            })
        });
    let (
        queue_state,
        task_id,
        assignment_id,
        active_lease_id,
        active_lease_owner,
        ready_to_dispatch,
        recommended_tool,
        reason,
    ) = if let Some(queue_item) = &queue {
        (
            queue_item.queue_state.clone(),
            queue_item.task_id.clone(),
            queue_item.assignment_id.clone(),
            queue_item.active_lease_id.clone(),
            queue_item.active_lease_owner.clone(),
            queue_item.ready_to_dispatch,
            queue_item.recommended_tool.clone(),
            queue_item.reason.clone(),
        )
    } else if item.closed {
        (
            "closed".to_string(),
            None,
            None,
            None,
            None,
            false,
            "inspect_work_queue".to_string(),
            item.reason.clone(),
        )
    } else if !item.open_dependencies.is_empty() {
        (
            "dependency_blocked".to_string(),
            None,
            None,
            None,
            None,
            false,
            "inspect_item".to_string(),
            item.reason.clone(),
        )
    } else {
        (
            "config_blocked".to_string(),
            None,
            None,
            None,
            None,
            false,
            "inspect_work_queue".to_string(),
            item.reason.clone(),
        )
    };
    let mut tool_params = map_params([("root", root.as_str()), ("item_id", item.item_id.as_str())]);
    if let Some(task_id) = &task_id {
        tool_params.insert("task_id".to_string(), Value::String(task_id.clone()));
    }
    if let Some(assignment_id) = &assignment_id {
        tool_params.insert(
            "assignment_id".to_string(),
            Value::String(assignment_id.clone()),
        );
    }
    if let Some(lease_id) = &active_lease_id {
        tool_params.insert("lease_id".to_string(), Value::String(lease_id.clone()));
    }
    let (findings, evidence) = item_artifacts(&root, &item.item_id, 50);
    let finding_count = findings.len();
    let evidence_count = evidence.len();
    let summary = format!(
        "Backlog item `{}` inspected: {}.",
        item.item_id, queue_state
    );
    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Completed,
        summary: summary.clone(),
        next_action: Some(reason.clone()),
        recovery_action: None,
        data: Some(InspectItemData {
            root,
            item,
            markdown: BacklogItemMarkdownState {
                path: snapshot.path.display().to_string(),
                sections: snapshot.sections,
                external_refs: snapshot.external_refs,
            },
            queue,
            plan,
            planning,
            planning_approval,
            queue_state,
            task_id,
            assignment_id,
            active_lease_id,
            active_lease_owner,
            ready_to_dispatch,
            recommended_tool,
            reason,
            params: tool_params,
            findings,
            evidence,
            finding_count,
            evidence_count,
        }),
        error: None,
    }
}

#[derive(Debug, Clone)]
struct QueueDispatchBlocker {
    task_id: String,
    task_status: String,
    assignment_id: Option<String>,
    assignment_status: Option<String>,
    queue_state: String,
}

fn source_item_dispatch_blockers(root: &Path) -> BTreeMap<String, QueueDispatchBlocker> {
    let storage = match crate::storage::connect_existing_read_only(root, None) {
        Ok(Some(storage)) => storage,
        Ok(None) | Err(_) => return BTreeMap::new(),
    };
    let closed_item_ids = backlog::closed_item_ids(root);
    match storage.repository().tasks().source_item_dispatch_blockers() {
        Ok(items) => items
            .into_iter()
            .filter(|item| {
                !(item.queue_state == "active" && closed_item_ids.contains(&item.source_item_id))
            })
            .map(|item| {
                (
                    item.source_item_id,
                    QueueDispatchBlocker {
                        task_id: item.task_id,
                        task_status: item.task_status,
                        assignment_id: item.assignment_id,
                        assignment_status: item.assignment_status,
                        queue_state: item.queue_state,
                    },
                )
            })
            .collect(),
        Err(_) => BTreeMap::new(),
    }
}

fn active_direct_claims(_default_root: &Path, root: &str) -> BTreeMap<String, LeaseRecord> {
    let storage = match crate::storage::connect_existing_read_only(Path::new(root), None) {
        Ok(Some(storage)) => storage,
        Ok(None) | Err(_) => return BTreeMap::new(),
    };
    match storage
        .repository()
        .leases()
        .list(Some("task"), None, Some("active"), false, 200)
    {
        Ok(leases) => leases
            .into_iter()
            .filter(|lease| looks_like_backlog_item_id(&lease.target_id))
            .map(|lease| (lease.target_id.clone(), lease))
            .collect(),
        Err(_) => BTreeMap::new(),
    }
}

fn looks_like_backlog_item_id(value: &str) -> bool {
    let Some((prefix, suffix)) = value.split_once('-') else {
        return false;
    };
    !prefix.is_empty()
        && prefix
            .chars()
            .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
        && !suffix.is_empty()
        && suffix.chars().all(|character| character.is_ascii_digit())
}

fn queue_inventory(
    default_root: &Path,
    root: &str,
    limit: usize,
    dispatch_blockers: &BTreeMap<String, QueueDispatchBlocker>,
) -> WorkQueueInventorySummary {
    let inventory = backlog::inspect_backlog_inventory(default_root, Some(root), None)
        .data
        .unwrap_or_else(|| BacklogInventoryData {
            root: root.to_string(),
            items: Vec::new(),
            total: 0,
            returned: 0,
            truncated: false,
            runnable: 0,
            closed: 0,
            blocked: 0,
        });
    let dependency_blocked = inventory
        .items
        .iter()
        .filter(|item| !item.closed && !item.open_dependencies.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    let closed = inventory
        .items
        .iter()
        .filter(|item| item.closed)
        .cloned()
        .collect::<Vec<_>>();
    let closed_with_evidence_count = closed.iter().filter(|item| item.has_evidence).count();
    let closed_without_evidence_count = closed.len().saturating_sub(closed_with_evidence_count);
    let active_lifecycle_item_ids = dispatch_blockers
        .iter()
        .filter(|(_, blocker)| blocker.queue_state == "active")
        .map(|(item_id, _)| item_id.clone())
        .collect::<Vec<_>>();
    let pending_integration_task_ids = dispatch_blockers
        .values()
        .filter(|blocker| blocker.queue_state == "completed_pending_integration")
        .map(|blocker| blocker.task_id.clone())
        .collect::<Vec<_>>();
    let truncated = dependency_blocked.len() > limit || closed.len() > limit;
    WorkQueueInventorySummary {
        total_count: inventory.total,
        runnable_count: inventory.runnable,
        dependency_blocked_count: dependency_blocked.len(),
        closed_count: inventory.closed,
        closed_with_evidence_count,
        closed_without_evidence_count,
        active_lifecycle_count: active_lifecycle_item_ids.len(),
        pending_integration_count: pending_integration_task_ids.len(),
        dependency_blocked_items: dependency_blocked.into_iter().take(limit).collect(),
        closed_items: closed.into_iter().take(limit).collect(),
        active_lifecycle_item_ids,
        pending_integration_task_ids,
        truncated,
    }
}

fn apply_dispatch_blocker(item: &mut WorkQueueItem, blocker: &QueueDispatchBlocker) {
    item.queue_state = blocker.queue_state.clone();
    item.task_id = Some(blocker.task_id.clone());
    item.assignment_id = blocker.assignment_id.clone();
    item.ready_to_dispatch = false;
    match blocker.queue_state.as_str() {
        "completed_pending_integration" => {
            item.recommended_tool = "integrate_worker_result".to_string();
            item.reason = format!(
                "Task `{}` for this item is completed and awaiting integration.",
                blocker.task_id
            );
        }
        "active" if blocker.assignment_status.as_deref() == Some("prepared") => {
            item.recommended_tool = "start_worker_task".to_string();
            item.reason = format!(
                "Worker assignment `{}` for task `{}` is prepared and ready to start.",
                blocker.assignment_id.as_deref().unwrap_or(""),
                blocker.task_id
            );
        }
        "active" if blocker.assignment_status.as_deref() == Some("running") => {
            item.recommended_tool = "record_worker_progress".to_string();
            item.reason = format!(
                "Worker assignment `{}` for task `{}` is running.",
                blocker.assignment_id.as_deref().unwrap_or(""),
                blocker.task_id
            );
        }
        "active" if matches!(blocker.task_status.as_str(), "queued" | "claimed") => {
            item.recommended_tool = "prepare_worker_handoff".to_string();
            item.reason = format!(
                "Task `{}` for this item is `{}` and needs a worker handoff.",
                blocker.task_id, blocker.task_status
            );
        }
        _ => {
            item.recommended_tool = "inspect_task".to_string();
            item.reason = format!(
                "Task `{}` for this item is `{}`; continue that lifecycle before dispatching this item again.",
                blocker.task_id, blocker.task_status
            );
        }
    }
    apply_execution_metadata(item);
}

fn apply_direct_claim(item: &mut WorkQueueItem, claim: &LeaseRecord) {
    item.queue_state = "active".to_string();
    item.execution_path = "direct_edit".to_string();
    item.completion_tool = Some("complete_backlog_item".to_string());
    item.active_lease_id = Some(claim.id.clone());
    item.active_lease_owner = Some(claim.owner.clone());
    item.ready_to_dispatch = false;
    item.recommended_tool = "list_leases".to_string();
    item.reason = format!(
        "Direct item `{}` is claimed by lease `{}` held by `{}` until {}. Continue that direct work, renew the lease, or release it before starting duplicate work.",
        item.candidate.item_id, claim.id, claim.owner, claim.expires_at
    );
    apply_execution_metadata(item);
}

fn candidate_from_item(
    item: &BacklogInventoryItem,
    external_refs: &[crate::models::ExternalRef],
) -> BacklogCandidate {
    BacklogCandidate {
        source: "backlog".to_string(),
        item_id: item.item_id.clone(),
        title: item.title.clone(),
        priority: item.priority.clone(),
        item_type: item.item_type.clone(),
        area: item.area.clone(),
        owned_surfaces: item.owned_surfaces.clone(),
        external_refs: external_refs.to_vec(),
    }
}

fn item_artifacts(
    root: &str,
    item_id: &str,
    limit: usize,
) -> (Vec<FindingRecord>, Vec<EvidenceRecord>) {
    let storage = match crate::storage::connect_existing_read_only(Path::new(root), None) {
        Ok(Some(storage)) => storage,
        Ok(None) | Err(_) => return (Vec::new(), Vec::new()),
    };
    let repository = storage.repository();
    let findings = repository
        .list_findings_for_item(item_id, limit)
        .unwrap_or_default();
    let evidence = repository
        .list_evidence_for_item(item_id, limit)
        .unwrap_or_default();
    (findings, evidence)
}

fn item_requires_clean_workspace(item: &WorkQueueItem) -> bool {
    item.ready_to_dispatch
        && item.effective_policy.execution_path == execution_policy::WORKER_HANDOFF
}

fn manager_dirty_paths(root: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut paths = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.len() < 4 {
            continue;
        }
        let path = line[3..].trim();
        if path.starts_with(".platy/") {
            continue;
        }
        paths.push(path.to_string());
    }
    Some(paths)
}

fn paths_are_backlog_artifacts(paths: &[String]) -> bool {
    !paths.is_empty()
        && paths.iter().all(|path| {
            (path.starts_with("backlog/items/") && path.ends_with(".md"))
                || (path.starts_with("backlog/plans/") && path.ends_with(".yaml"))
        })
}

fn legacy_policy_warnings(
    require_task_plan: Option<bool>,
    require_planning_approval: Option<bool>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if require_task_plan.is_some() {
        warnings.push(
            "Deprecated input require_task_plan was ignored. Set workflow.execution or backlog item planning_gate instead."
                .to_string(),
        );
    }
    if require_planning_approval.is_some() {
        warnings.push(
            "Deprecated input require_planning_approval was ignored. Use planning_gate=approved_task_plan instead."
                .to_string(),
        );
    }
    warnings
}

fn effective_policy_for_candidate(
    default_root: &Path,
    root: &str,
    item_id: &str,
    execution_config: &WorkflowExecutionConfig,
) -> EffectiveExecutionPolicy {
    let item_policy = backlog::backlog_item_execution_policy(default_root, Some(root), item_id)
        .unwrap_or_default();
    execution_policy::resolve_effective_policy(
        execution_config,
        item_policy.execution_path.as_deref(),
        item_policy.planning_gate.as_deref(),
    )
}

fn work_queue_item(
    default_root: &Path,
    root: &str,
    position: usize,
    candidate: BacklogCandidate,
    execution_config: &WorkflowExecutionConfig,
) -> WorkQueueItem {
    let effective_policy =
        effective_policy_for_candidate(default_root, root, &candidate.item_id, execution_config);
    let planning = planning_requirement(&candidate, &effective_policy);
    let plan_required = execution_policy::plan_required(&effective_policy.planning_gate);
    let approval_required = execution_policy::approval_required(&effective_policy.planning_gate);
    let plan = normalized_task_plan_state(default_root, root, &candidate.item_id, plan_required);
    let plan_valid = plan.status == "valid";
    let plan_ready = !plan_required || plan_valid;
    let planning_approval = if approval_required {
        match approvals::planning_approval_state(default_root, Some(root), &candidate.item_id, true)
        {
            Ok(state) => Some(state),
            Err(error) => Some(PlanningApprovalState {
                item_id: candidate.item_id.clone(),
                required: true,
                approved: false,
                approval_id: None,
                status: None,
                reason: format!("Could not inspect planning approval state: {error}"),
            }),
        }
    } else {
        None
    };
    let approval_ready = planning_approval
        .as_ref()
        .is_none_or(|state| !state.required || state.approved);
    let ready_to_dispatch = plan_ready && approval_ready;
    let (recommended_tool, reason, queue_state) = if ready_to_dispatch
        && effective_policy.execution_path == execution_policy::DIRECT_EDIT
    {
        (
            "complete_backlog_item".to_string(),
            "Direct work is ready for implementation items: edit the manager workspace, verify, then call complete_backlog_item. For planning or intake items, confirm scope and assumptions before editing or completing work. Call prepare_work only when you need optional response-local guidance.".to_string(),
            "direct_ready".to_string(),
        )
    } else if ready_to_dispatch
        && effective_policy.execution_path == execution_policy::WORKER_HANDOFF
    {
        (
            "dispatch_ready_work".to_string(),
            "Worker handoff is ready; prepare_work or dispatch_ready_work can create an isolated worktree handoff.".to_string(),
            "ready".to_string(),
        )
    } else if !plan_ready && plan.status == "missing" {
        (
            "write_task_plan".to_string(),
            "A task plan is required before dispatch. Use the host model to write explicit requirements, design notes, tasks, and verification commands, then validate_task_plan.".to_string(),
            "planning_blocked".to_string(),
        )
    } else if !plan_ready {
        (
            "validate_task_plan".to_string(),
            "Task plan must be fixed before dispatch.".to_string(),
            "planning_blocked".to_string(),
        )
    } else if approval_required {
        (
            "request_planning_approval".to_string(),
            planning_approval
                .as_ref()
                .map(|state| state.reason.clone())
                .unwrap_or_else(|| "Planning approval is required before dispatch.".to_string()),
            "approval_blocked".to_string(),
        )
    } else {
        (
            "inspect_work_queue".to_string(),
            "Backlog item is not ready to dispatch.".to_string(),
            "config_blocked".to_string(),
        )
    };
    let (execution_path, completion_tool, task_plan_required_for_worktree, execution_guidance) =
        execution_metadata(&queue_state, &effective_policy);
    let lifecycle_mode = item_lifecycle_mode(&queue_state, &effective_policy);
    let prepare_work_optional = queue_state == "direct_ready"
        && effective_policy.execution_path == execution_policy::DIRECT_EDIT
        && effective_policy.planning_gate == execution_policy::GATE_NONE;
    let minimal_direct_loop =
        (queue_state == "direct_ready").then(|| direct_work_loop(Some(candidate.item_id.as_str())));
    WorkQueueItem {
        position,
        candidate,
        planning,
        plan,
        planning_approval,
        effective_policy,
        queue_state,
        execution_path,
        lifecycle_mode,
        completion_tool,
        task_plan_required_for_worktree,
        prepare_work_optional,
        execution_guidance,
        minimal_direct_loop,
        task_id: None,
        assignment_id: None,
        active_lease_id: None,
        active_lease_owner: None,
        ready_to_dispatch,
        recommended_tool,
        reason,
    }
}

fn execution_metadata(
    queue_state: &str,
    effective_policy: &EffectiveExecutionPolicy,
) -> (String, Option<String>, bool, String) {
    let plan_required = execution_policy::plan_required(&effective_policy.planning_gate);
    match queue_state {
        "direct_ready" => (
            "direct_edit".to_string(),
            Some("complete_backlog_item".to_string()),
            true,
            "Direct edit is ready from durable execution policy. For implementation items, edit the manager workspace, verify, then call complete_backlog_item; prepare_work is optional response-local guidance only. For planning or intake items, first confirm scope and assumptions with the user before editing or completing work. inspect_work_queue stays direct_ready until completion records closure. To use a worktree handoff, set execution_path=worker_handoff on the backlog item or workflow.execution default.".to_string(),
        ),
        "ready" => (
            "worker_handoff".to_string(),
            Some("finish_work".to_string()),
            false,
            "Worktree handoff is ready because the required task-plan and approval gates are satisfied.".to_string(),
        ),
        "active" => (
            "active".to_string(),
            Some("finish_work".to_string()),
            false,
            "An existing task lifecycle is active; continue or finish that task before dispatching this item again.".to_string(),
        ),
        "completed_pending_integration" => (
            "pending_integration".to_string(),
            Some("integrate_worker_result".to_string()),
            false,
            "A completed worker result is waiting for integration.".to_string(),
        ),
        "planning_blocked" => (
            "blocked".to_string(),
            None,
            true,
            "A task plan is required or invalid before this item can use a worktree handoff.".to_string(),
        ),
        "approval_blocked" => (
            "blocked".to_string(),
            None,
            plan_required,
            "Planning approval is required before this item can continue.".to_string(),
        ),
        "workspace_blocked" | "config_blocked" => (
            "blocked".to_string(),
            None,
            plan_required,
            "Setup or manager workspace state blocks dispatch until the reported recovery action is completed.".to_string(),
        ),
        "dependency_blocked" => (
            "blocked".to_string(),
            None,
            plan_required,
            "Open dependencies block this item before planning or dispatch can continue.".to_string(),
        ),
        _ => (
            "blocked".to_string(),
            None,
            plan_required,
            "Inspect this item or queue state before choosing an execution path.".to_string(),
        ),
    }
}

fn apply_execution_metadata(item: &mut WorkQueueItem) {
    let (execution_path, completion_tool, task_plan_required_for_worktree, execution_guidance) =
        execution_metadata(&item.queue_state, &item.effective_policy);
    item.execution_path = execution_path;
    item.lifecycle_mode = item_lifecycle_mode(&item.queue_state, &item.effective_policy);
    item.completion_tool = completion_tool;
    item.task_plan_required_for_worktree = task_plan_required_for_worktree;
    item.execution_guidance = execution_guidance;
}

fn normalized_task_plan_state(
    default_root: &Path,
    root: &str,
    item_id: &str,
    require_task_plan: bool,
) -> WorkQueuePlanState {
    let mut plan = task_plan_state(default_root, root, item_id);
    if !require_task_plan && plan.status == "missing" {
        plan.status = "not_required".to_string();
        plan.errors.clear();
    }
    plan
}

fn planning_requirement(
    candidate: &BacklogCandidate,
    effective_policy: &EffectiveExecutionPolicy,
) -> PlanningClassification {
    let mode = effective_policy.planning_gate.clone();
    let reasons = vec![effective_policy.reason.clone()];
    PlanningClassification {
        item_id: candidate.item_id.clone(),
        required_mode: mode,
        required_artifact: execution_policy::plan_required(&effective_policy.planning_gate)
            .then(|| format!("backlog/plans/{}.yaml", candidate.item_id)),
        reasons,
    }
}

fn task_plan_state(default_root: &Path, root: &str, item_id: &str) -> WorkQueuePlanState {
    let listed = backlog::list_task_plans(
        default_root,
        TaskPlanQueryParams {
            root: Some(root.to_string()),
            item_id: Some(item_id.to_string()),
            include_errors: Some(true),
        },
    );
    let summary = listed
        .data
        .as_ref()
        .and_then(|data| data.plans.first())
        .cloned();
    let validation = backlog::validate_task_plan(
        default_root,
        TaskPlanQueryParams {
            root: Some(root.to_string()),
            item_id: Some(item_id.to_string()),
            include_errors: Some(true),
        },
    );
    let errors = validation
        .data
        .as_ref()
        .map(|data| data.errors.clone())
        .unwrap_or_else(|| validation.error.iter().cloned().collect());

    match summary {
        Some(summary) if validation.status == ActionStatus::Completed => WorkQueuePlanState {
            status: "valid".to_string(),
            path: Some(summary.path),
            mode: summary.mode,
            task_count: summary.task_count,
            requirement_count: summary.requirement_count,
            errors,
        },
        Some(summary) => WorkQueuePlanState {
            status: "invalid".to_string(),
            path: Some(summary.path),
            mode: summary.mode,
            task_count: summary.task_count,
            requirement_count: summary.requirement_count,
            errors,
        },
        None => WorkQueuePlanState {
            status: "missing".to_string(),
            path: None,
            mode: None,
            task_count: 0,
            requirement_count: 0,
            errors,
        },
    }
}

fn recommended_queue_action(
    root: &str,
    items: &[WorkQueueItem],
) -> (String, String, BTreeMap<String, Value>) {
    let Some(first) = items.first() else {
        return (
            "create_backlog_items".to_string(),
            format!(
                "No backlog item exists yet. {} Run project intake first: inspect docs, Git/scaffold state, and workflow config defaults; summarize known facts, assumptions, open questions, proposed first milestone, and things not to do yet; get user approval before calling create_backlog_items.",
                DIRECTION_SOURCE_GUIDANCE
            ),
            map_params([("root", root)]),
        );
    };

    let ready_count = items.iter().filter(|item| item.ready_to_dispatch).count();
    let item_id = first.candidate.item_id.as_str();
    let mut params = map_params([("root", root)]);
    match first.recommended_tool.as_str() {
        "dispatch_ready_work" => {
            params.insert(
                "max_tasks".to_string(),
                Value::Number((ready_count.max(1).min(10) as u64).into()),
            );
        }
        "prepare_work" => {
            params.insert(
                "max_tasks".to_string(),
                Value::Number((ready_count.max(1).min(10) as u64).into()),
            );
        }
        "complete_backlog_item" => {
            params.insert("item_id".to_string(), Value::String(item_id.to_string()));
        }
        "write_task_plan" | "validate_task_plan" => {
            params.insert("item_id".to_string(), Value::String(item_id.to_string()));
        }
        "prepare_worker_handoff" | "integrate_worker_result" | "inspect_task" => {
            if let Some(task_id) = &first.task_id {
                params.insert("task_id".to_string(), Value::String(task_id.clone()));
            }
            if first.recommended_tool == "integrate_worker_result"
                && crate::config::effective_workflow_config(Path::new(root))
                    .map(|config| !config.require_verification_evidence)
                    .unwrap_or(false)
            {
                params.insert("allow_unverified".to_string(), Value::Bool(true));
            }
        }
        "start_worker_task" => {
            if let Some(assignment_id) = &first.assignment_id {
                params.insert(
                    "assignment_id".to_string(),
                    Value::String(assignment_id.clone()),
                );
            }
            params.insert(
                "worker_session".to_string(),
                Value::String("external-worker-session".to_string()),
            );
        }
        "record_worker_progress" => {
            if let Some(assignment_id) = &first.assignment_id {
                params.insert(
                    "assignment_id".to_string(),
                    Value::String(assignment_id.clone()),
                );
            }
            params.insert(
                "event_type".to_string(),
                Value::String("worker_progress".to_string()),
            );
            params.insert(
                "summary".to_string(),
                Value::String("Describe the worker progress.".to_string()),
            );
        }
        _ => {}
    }
    (
        first.recommended_tool.clone(),
        if first.recommended_tool == "dispatch_ready_work" {
            format!(
                "{} {} {} ready item(s) can be selected now; pass max_tasks to control the batch size.",
                item_id,
                first.reason,
                ready_count
            )
        } else if first.recommended_tool == "prepare_work" {
            format!(
                "{} {} {} ready item(s) can be prepared now; pass max_tasks to control the batch size.",
                item_id,
                first.reason,
                ready_count
            )
        } else if first.recommended_tool == "complete_backlog_item" {
            format!("{} {}", item_id, first.reason)
        } else {
            format!("{} {}", item_id, first.reason)
        },
        params,
    )
}

fn map_params<const N: usize>(params: [(&str, &str); N]) -> BTreeMap<String, Value> {
    params
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (key.to_string(), Value::String(value.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approvals::{approval_respond, request_planning_approval};
    use crate::evidence::record_evidence;
    use crate::leases::{acquire_lease, release_lease};
    use crate::models::{
        AcquireLeaseParams, ApprovalRespondParams, RecordEvidenceParams, ReleaseLeaseParams,
        RequestPlanningApprovalParams,
    };
    use crate::tasks::{create_task_record, NewTask};
    use std::{collections::BTreeMap, fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn inspect_work_queue_does_not_create_runtime_state() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "Ready item");

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: Some(true),
            },
        );
        assert_eq!(result.status, ActionStatus::Completed);
        assert!(
            !project.path().join(".platy").exists(),
            "read-only queue inspection must not create runtime state"
        );
    }

    #[test]
    fn inspect_work_queue_reports_explicit_empty_backlog_state() {
        let project = backlog_project();
        init_git(project.path());

        let queue = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: None,
            },
        );
        let queue_data = queue.data.expect("queue data");

        assert_eq!(queue.status, ActionStatus::Skipped);
        assert_eq!(queue_data.queue_state, "empty_backlog");
        assert_eq!(queue_data.inventory.total_count, 0);
        assert_eq!(queue_data.recommended_tool, "create_backlog_items");
        assert!(queue_data.reason.contains("docs/product.md"));
        assert!(queue_data.reason.contains("ask the user"));
        assert!(queue_data.reason.contains("workflow config defaults"));
        assert!(queue_data.reason.contains("assumptions"));
        assert!(queue_data.reason.contains("approval"));
        assert_eq!(
            queue_data.claude_toolsearch_batch_selector.as_deref(),
            Some("select:mcp__platypus__create_backlog_items,mcp__platypus__inspect_work_queue")
        );
        assert_eq!(
            queue_data.host_neutral_tool_search_query.as_deref(),
            Some("platypus tools create_backlog_items inspect_work_queue")
        );
        assert!(queue_data
            .schemas_likely_needed_next
            .iter()
            .any(|hint| hint.tool_name == "create_backlog_items"
                && hint.host_neutral_query == "platypus tool create_backlog_items"
                && hint.usage == "post_intake"
                && hint.codex_tool_search_query
                    == "mcp__platypus__create_backlog_items platypus create_backlog_items"));

        let status = inspect_queue_status(
            project.path(),
            InspectQueueStatusParams {
                root: None,
                limit: Some(5),
            },
        );
        let status_data = status.data.expect("status data");

        assert_eq!(status.status, ActionStatus::Skipped);
        assert_eq!(status_data.queue_state, "empty_backlog");
        assert_eq!(
            status_data.claude_toolsearch_batch_selector.as_deref(),
            Some("select:mcp__platypus__create_backlog_items,mcp__platypus__inspect_work_queue")
        );
        assert_eq!(
            status_data.host_neutral_tool_search_query.as_deref(),
            Some("platypus tools create_backlog_items inspect_work_queue")
        );
        assert!(status_data
            .state_descriptions
            .iter()
            .any(|state| state.queue_state == "empty_backlog"));
    }

    #[test]
    fn inspect_work_queue_closed_state_points_to_direction_sources() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "Closed work");
        git(project.path(), &["add", "backlog"]);
        git(
            project.path(),
            &[
                "commit",
                "-m",
                "Close work",
                "-m",
                "Platypus-Closes: PROJ-001\nPlatypus-Verification: cargo test",
            ],
        );

        let queue = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: None,
            },
        );
        let queue_data = queue.data.expect("queue data");

        assert_eq!(queue.status, ActionStatus::Completed);
        assert_eq!(queue_data.queue_state, "closed");
        assert_eq!(queue_data.recommended_tool, "create_backlog_items");
        assert!(queue_data.reason.contains("docs/product.md"));
        assert!(queue_data.reason.contains("ask the user"));
        assert!(queue_data.reason.contains("workflow config defaults"));
        assert!(queue_data.reason.contains("assumptions"));
        assert!(queue_data.reason.contains("approval"));

        let status = inspect_queue_status(
            project.path(),
            InspectQueueStatusParams {
                root: None,
                limit: Some(5),
            },
        );
        let status_data = status.data.expect("status data");

        assert_eq!(status_data.queue_state, "closed");
        assert!(status_data
            .state_descriptions
            .iter()
            .any(|state| state.queue_state == "closed"
                && state.description.contains("durable direction")
                && state.description.contains("ask the user")));
    }

    #[test]
    fn inspect_session_combines_startup_state_without_mutation() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "Ready item");

        let result = inspect_session(
            project.path(),
            InspectSessionParams {
                root: None,
                limit: Some(10),
                detail: Some("verbose".to_string()),
                require_task_plan: None,
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("session");

        assert_eq!(result.status, ActionStatus::Completed);
        assert!(data.ok);
        assert_eq!(data.detail, "verbose");
        assert_eq!(data.compact.recommended_tool, "complete_backlog_item");
        assert_eq!(data.compact.queue_state.as_deref(), Some("direct_ready"));
        assert_eq!(data.recommended_tool, "complete_backlog_item");
        assert_eq!(
            data.minimal_direct_loop
                .as_ref()
                .expect("direct loop")
                .primary_next_tool,
            "complete_backlog_item"
        );
        assert!(data.doctor.as_ref().expect("doctor").ok);
        assert_eq!(data.status.as_ref().expect("status").backlog_items, 1);
        assert!(data.workflow.is_some());
        assert_eq!(data.queue.as_ref().expect("queue").items.len(), 1);
        assert_eq!(
            data.claude_toolsearch_batch_selector.as_deref(),
            Some(
                "select:mcp__platypus__complete_backlog_item,mcp__platypus__record_verification_evidence,mcp__platypus__prepare_work"
            )
        );
        assert_eq!(
            data.host_neutral_tool_search_query.as_deref(),
            Some("platypus tools complete_backlog_item record_verification_evidence prepare_work")
        );
        assert!(data
            .schemas_likely_needed_next
            .iter()
            .any(|hint| hint.tool_name == "complete_backlog_item"
                && hint.usage == "required"
                && hint.host_neutral_query == "platypus tool complete_backlog_item"
                && hint.codex_tool_search_query
                    == "mcp__platypus__complete_backlog_item platypus complete_backlog_item"));

        let compact = inspect_session(
            project.path(),
            InspectSessionParams {
                root: None,
                limit: Some(10),
                detail: None,
                require_task_plan: None,
                require_planning_approval: None,
            },
        )
        .data
        .expect("compact session");
        assert_eq!(compact.detail, "compact");
        assert!(compact.doctor.is_none());
        assert!(compact.status.is_none());
        assert!(compact.workflow.is_none());
        assert!(compact.queue.is_none());
        assert_eq!(compact.compact.recommended_tool, "complete_backlog_item");
        assert!(
            !project.path().join(".platy").exists(),
            "read-only session inspection must not create runtime state"
        );
    }

    #[test]
    fn inspect_work_queue_surfaces_active_direct_claim_lease() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "Ready item");

        let lease = acquire_lease(
            project.path(),
            AcquireLeaseParams {
                root: None,
                scope: "task".to_string(),
                target_id: "PROJ-001".to_string(),
                owner: "manager-a".to_string(),
                ttl_seconds: Some(600),
                metadata: BTreeMap::new(),
            },
        )
        .data
        .expect("lease")
        .lease;

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.queue_state, "active");
        assert_eq!(data.active_count, 1);
        assert_eq!(data.active_lease_ids, vec![lease.id.clone()]);
        assert_eq!(data.items[0].queue_state, "active");
        assert_eq!(
            data.items[0].active_lease_id.as_deref(),
            Some(lease.id.as_str())
        );
        assert_eq!(
            data.items[0].active_lease_owner.as_deref(),
            Some("manager-a")
        );
        assert_eq!(data.items[0].recommended_tool, "list_leases");
        assert!(!data.items[0].ready_to_dispatch);

        let status = inspect_queue_status(
            project.path(),
            InspectQueueStatusParams {
                root: None,
                limit: Some(5),
            },
        )
        .data
        .expect("status");
        assert_eq!(status.queue_state, "active");
        assert_eq!(status.counts.active_count, 1);
        assert_eq!(status.active_leases[0].lease_id, lease.id);
        assert_eq!(status.active_leases[0].owner, "manager-a");

        release_lease(
            project.path(),
            ReleaseLeaseParams {
                root: None,
                lease_id: lease.id,
                owner: "manager-a".to_string(),
            },
        )
        .data
        .expect("released lease");

        let released_queue = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: None,
            },
        )
        .data
        .expect("released queue");
        assert_eq!(released_queue.queue_state, "direct_ready");
        assert!(released_queue.active_lease_ids.is_empty());
        assert!(released_queue.items[0].active_lease_id.is_none());
    }

    #[test]
    fn inspect_work_queue_reports_missing_git_before_dispatch() {
        let project = backlog_project();
        write_item(project.path(), "PROJ-001", "Ready item");

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.recommended_tool, "complete_backlog_item");
        assert_eq!(data.queue_state, "direct_ready");
        assert!(data.items[0].ready_to_dispatch);
        assert_eq!(data.items[0].queue_state, "direct_ready");
    }

    #[test]
    fn inspect_work_queue_reports_unborn_head_before_dispatch() {
        let project = backlog_project();
        git(project.path(), &["init"]);
        write_item(project.path(), "PROJ-001", "Ready item");

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.recommended_tool, "complete_backlog_item");
        assert!(data.items[0].ready_to_dispatch);
        assert_eq!(data.items[0].queue_state, "direct_ready");
    }

    #[test]
    fn inspect_work_queue_can_require_planning_approval() {
        let project = backlog_project();
        write_item(project.path(), "PROJ-001", "Ready item");
        mark_worker_policy(project.path(), "PROJ-001", "approved_task_plan");
        fs::create_dir_all(project.path().join("backlog/plans")).expect("plans");
        fs::write(
            project.path().join("backlog/plans/PROJ-001.yaml"),
            r#"item_id: PROJ-001
version: 1
mode: standard
requirements:
  - id: R1
    text: Do the work.
design:
  summary: Focused implementation.
  owned_surfaces:
    - src/lib.rs
  notes: null
tasks:
  - id: PROJ-001-T01
    title: Implement first item
    goal: Complete the first item.
    requirement_refs:
      - R1
    depends_on: []
    owned_surfaces:
      - src/lib.rs
    verification:
      - make check
    acceptance:
      - The item is implemented and verified.
    notes: null
"#,
        )
        .expect("plan");
        init_git(project.path());

        let blocked = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: None,
            },
        );
        let blocked_data = blocked.data.expect("blocked queue");

        assert_eq!(blocked.status, ActionStatus::Completed);
        assert!(!blocked_data.items[0].ready_to_dispatch);
        assert_eq!(
            blocked_data.items[0].recommended_tool,
            "request_planning_approval"
        );
        assert_eq!(
            blocked_data.items[0]
                .planning_approval
                .as_ref()
                .expect("planning approval")
                .required,
            true
        );

        let approval = request_planning_approval(
            project.path(),
            RequestPlanningApprovalParams {
                root: None,
                item_ids: vec!["PROJ-001".to_string()],
                requested_by: Some("manager".to_string()),
                summary: None,
            },
        )
        .data
        .expect("approval")
        .approval;
        approval_respond(
            project.path(),
            ApprovalRespondParams {
                root: None,
                approval_id: approval.id,
                decision: "approve".to_string(),
                responder: Some("user".to_string()),
                reason: Some("Reviewed.".to_string()),
            },
        );

        let ready = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: None,
            },
        );
        let ready_data = ready.data.expect("ready queue");

        assert!(ready_data.items[0].ready_to_dispatch);
        assert_eq!(ready_data.items[0].recommended_tool, "dispatch_ready_work");
        assert!(
            ready_data.items[0]
                .planning_approval
                .as_ref()
                .expect("planning approval")
                .approved
        );
    }

    #[test]
    fn inspects_work_queue_with_missing_required_task_plan() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        mark_worker_policy(project.path(), "PROJ-001", "task_plan");
        git(project.path(), &["add", "backlog"]);
        git(project.path(), &["commit", "-m", "Add backlog item"]);

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(data.recommended_tool, "write_task_plan");
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].plan.status, "missing");
        assert!(!data.items[0].ready_to_dispatch);
        assert_eq!(data.params["item_id"], "PROJ-001");
        assert!(data
            .schemas_likely_needed_next
            .iter()
            .any(|hint| hint.tool_name == "write_task_plan"
                && hint.host_neutral_query == "platypus tool write_task_plan"
                && hint.claude_toolsearch_selector.as_deref()
                    == Some("select:mcp__platypus__write_task_plan")));
    }

    #[test]
    fn direct_items_do_not_require_task_plan() {
        let project = backlog_project();
        init_git(project.path());
        write_item_with(
            project.path(),
            ItemFixture {
                id: "PROJ-001",
                title: "Update docs",
                item_type: "docs",
                area: "docs",
                owned_surfaces: &["README.md"],
            },
        );
        git(project.path(), &["add", "backlog"]);
        git(project.path(), &["commit", "-m", "Add backlog item"]);

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(data.recommended_tool, "complete_backlog_item");
        assert_eq!(data.items[0].planning.required_mode, "none");
        assert_eq!(data.items[0].queue_state, "direct_ready");
        assert_eq!(data.items[0].execution_path, "direct_edit");
        assert_eq!(
            data.items[0].completion_tool.as_deref(),
            Some("complete_backlog_item")
        );
        assert!(data.items[0].task_plan_required_for_worktree);
        assert!(data.items[0].prepare_work_optional);
        assert!(data.items[0]
            .execution_guidance
            .contains("prepare_work is optional"));
        assert!(data.items[0]
            .execution_guidance
            .contains("confirm scope and assumptions"));
        assert!(data
            .schemas_likely_needed_next
            .iter()
            .any(|hint| hint.tool_name == "complete_backlog_item"
                && hint.host_neutral_query == "platypus tool complete_backlog_item"
                && hint.claude_toolsearch_selector.as_deref()
                    == Some("select:mcp__platypus__complete_backlog_item")));
        assert_eq!(data.items[0].recommended_tool, "complete_backlog_item");
        assert_eq!(
            data.items[0]
                .minimal_direct_loop
                .as_ref()
                .expect("item direct loop")
                .optional_guidance_tool
                .as_deref(),
            Some("prepare_work")
        );
        assert_eq!(data.items[0].plan.status, "not_required");
        assert!(data.items[0].plan.errors.is_empty());
        assert!(data.items[0].ready_to_dispatch);
    }

    #[test]
    fn inspect_work_queue_surfaces_dirty_workspace_before_dispatch() {
        let project = backlog_project();
        init_git(project.path());
        write_item_with(
            project.path(),
            ItemFixture {
                id: "PROJ-001",
                title: "Update docs",
                item_type: "docs",
                area: "docs",
                owned_surfaces: &["README.md"],
            },
        );
        fs::write(project.path().join("README.md"), "# Dirty\n").expect("dirty readme");

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(data.recommended_tool, "complete_backlog_item");
        assert_eq!(data.items[0].queue_state, "direct_ready");
        assert!(data.preflight_warnings.is_empty());
        assert!(data.reason.contains("Direct work is ready"));
    }

    #[test]
    fn inspect_work_queue_does_not_block_direct_work_for_missing_worker_config() {
        let project = backlog_project();
        fs::write(project.path().join("platy.yaml"), "project: test\n").expect("config");
        init_git(project.path());
        write_item_with(
            project.path(),
            ItemFixture {
                id: "PROJ-001",
                title: "Update docs",
                item_type: "docs",
                area: "docs",
                owned_surfaces: &["README.md"],
            },
        );
        git(project.path(), &["add", "backlog", "platy.yaml"]);
        git(project.path(), &["commit", "-m", "Add backlog item"]);

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(data.recommended_tool, "complete_backlog_item");
        assert!(data.preflight_warnings.is_empty());
        assert!(data.items[0].ready_to_dispatch);
        assert_eq!(data.items[0].queue_state, "direct_ready");
        assert_eq!(data.items[0].recommended_tool, "complete_backlog_item");
    }

    #[test]
    fn inspect_work_queue_ignores_legacy_agent_config() {
        let project = backlog_project();
        fs::write(project.path().join("platy.yaml"), "project: test\n").expect("config");
        init_git(project.path());
        write_item_with(
            project.path(),
            ItemFixture {
                id: "PROJ-001",
                title: "Update docs",
                item_type: "docs",
                area: "docs",
                owned_surfaces: &["README.md"],
            },
        );
        git(project.path(), &["add", "backlog", "platy.yaml"]);
        git(project.path(), &["commit", "-m", "Add backlog item"]);

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(data.recommended_tool, "complete_backlog_item");
        assert!(data.preflight_warnings.is_empty());
        assert_eq!(data.items[0].queue_state, "direct_ready");
    }

    #[test]
    fn explicit_task_plan_requirement_applies_to_any_item() {
        let project = backlog_project();
        init_git(project.path());
        write_item_with(
            project.path(),
            ItemFixture {
                id: "PROJ-001",
                title: "Set up React frontend project structure",
                item_type: "foundation",
                area: "tooling",
                owned_surfaces: &["frontend"],
            },
        );
        mark_worker_policy(project.path(), "PROJ-001", "task_plan");
        git(project.path(), &["add", "backlog"]);
        git(project.path(), &["commit", "-m", "Add backlog item"]);

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(true),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(data.recommended_tool, "write_task_plan");
        assert_eq!(data.queue_state, "planning_blocked");
        assert_eq!(data.items[0].planning.required_mode, "task_plan");
        assert_eq!(
            data.items[0].planning.required_artifact.as_deref(),
            Some("backlog/plans/PROJ-001.yaml")
        );
        assert_eq!(data.items[0].plan.status, "missing");
        assert!(!data.items[0].ready_to_dispatch);
    }

    #[test]
    fn inspects_work_queue_and_recommends_dispatch_with_valid_plan() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        mark_worker_policy(project.path(), "PROJ-001", "task_plan");
        fs::create_dir_all(project.path().join("backlog/plans")).expect("plans");
        fs::write(
            project.path().join("backlog/plans/PROJ-001.yaml"),
            r#"item_id: PROJ-001
version: 1
mode: standard
requirements:
  - id: R1
    text: Do the work.
design:
  summary: Focused implementation.
  owned_surfaces:
    - src/lib.rs
  notes: null
tasks:
  - id: PROJ-001-T01
    title: Implement first item
    goal: Complete the first item.
    requirement_refs:
      - R1
    depends_on: []
    owned_surfaces:
      - src/lib.rs
    verification:
      - make check
    acceptance:
      - The item is implemented and verified.
    notes: null
"#,
        )
        .expect("plan");
        git(project.path(), &["add", "backlog"]);
        git(
            project.path(),
            &["commit", "-m", "Add planned backlog item"],
        );

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(true),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.recommended_tool, "dispatch_ready_work");
        assert_eq!(data.items[0].plan.status, "valid");
        assert!(data.items[0].ready_to_dispatch);
        assert_eq!(data.items[0].queue_state, "ready");
        assert_eq!(data.items[0].recommended_tool, "dispatch_ready_work");
        assert_eq!(data.items[0].plan.task_count, 1);
    }

    #[test]
    fn inspect_work_queue_reports_items_with_active_tasks() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Already dispatched".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.active_count, 1);
        assert_eq!(data.active_item_ids, vec!["PROJ-001"]);
        assert_eq!(data.active_task_ids, vec!["PROJ-001-T001"]);
        assert_eq!(data.items[0].queue_state, "active");
        assert_eq!(data.items[0].recommended_tool, "prepare_worker_handoff");
        assert_eq!(data.items[0].task_id.as_deref(), Some("PROJ-001-T001"));
        assert!(!data.items[0].ready_to_dispatch);
        assert_eq!(data.recommended_tool, "prepare_worker_handoff");
        assert!(data.summary.contains("blocked by active lifecycle"));
        assert!(data
            .preflight_warnings
            .iter()
            .any(|warning| warning.contains("active tasks or completed tasks")));
    }

    #[test]
    fn inspect_work_queue_includes_blocked_inventory_context() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        write_item_with_deps(project.path(), "PROJ-002", "Second item", &["PROJ-001"]);
        git(project.path(), &["add", "backlog"]);
        git(
            project.path(),
            &["commit", "-m", "Add dependent backlog items"],
        );

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.inventory.total_count, 2);
        assert_eq!(data.inventory.runnable_count, 1);
        assert_eq!(data.inventory.dependency_blocked_count, 1);
        assert_eq!(
            data.inventory.dependency_blocked_items[0].item_id,
            "PROJ-002"
        );
    }

    #[test]
    fn inspect_queue_status_returns_compact_counts_and_top_items() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        write_item(project.path(), "PROJ-002", "Second item");
        write_item_with_deps(project.path(), "PROJ-003", "Third item", &["PROJ-002"]);
        git(project.path(), &["add", "backlog"]);
        git(project.path(), &["commit", "-m", "Add queue items"]);

        let result = inspect_queue_status(
            project.path(),
            InspectQueueStatusParams {
                root: None,
                limit: Some(2),
            },
        );
        let data = result.data.expect("queue status");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.counts.total_count, 3);
        assert_eq!(data.counts.runnable_count, 2);
        assert_eq!(data.counts.ready_count, 2);
        assert_eq!(data.counts.dependency_blocked_count, 1);
        assert_eq!(data.counts.blocked_count, 1);
        assert_eq!(data.queue_state, "direct_ready");
        assert_eq!(data.top_ready_items.len(), 2);
        assert_eq!(data.top_ready_items[0].item_id, "PROJ-001");
        assert_eq!(data.top_ready_items[0].queue_state, "direct_ready");
        assert_eq!(data.top_blocked_items.len(), 1);
        assert_eq!(data.top_blocked_items[0].item_id, "PROJ-003");
        assert_eq!(data.top_blocked_items[0].queue_state, "dependency_blocked");
        assert!(data
            .state_descriptions
            .iter()
            .any(|state| state.queue_state == "planning_blocked"));
        assert!(data.active_tasks.is_empty());
    }

    #[test]
    fn queue_inventory_exposes_closed_item_evidence_counts_without_details() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "Closed with evidence");
        git(project.path(), &["add", "backlog"]);
        git(
            project.path(),
            &[
                "commit",
                "-m",
                "Close PROJ-001",
                "-m",
                "Platypus-Closes: PROJ-001",
            ],
        );
        let recorded = record_evidence(
            project.path(),
            RecordEvidenceParams {
                root: None,
                id: None,
                source_item_id: Some("PROJ-001".to_string()),
                source_task_id: None,
                kind: "note".to_string(),
                summary: "Closure evidence exists.".to_string(),
                refs: vec!["manual:test".to_string()],
                metadata: BTreeMap::new(),
            },
        );
        assert_eq!(recorded.status, ActionStatus::Completed);

        let queue = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: None,
            },
        );
        let queue_data = queue.data.expect("queue data");
        let closed = &queue_data.inventory.closed_items[0];

        assert_eq!(queue_data.inventory.closed_count, 1);
        assert_eq!(queue_data.inventory.closed_with_evidence_count, 1);
        assert_eq!(queue_data.inventory.closed_without_evidence_count, 0);
        assert_eq!(closed.item_id, "PROJ-001");
        assert_eq!(closed.has_evidence, true);
        assert_eq!(closed.evidence_count, 1);

        let status = inspect_queue_status(
            project.path(),
            InspectQueueStatusParams {
                root: None,
                limit: Some(5),
            },
        );
        let status_data = status.data.expect("status data");

        assert_eq!(status_data.counts.closed_count, 1);
        assert_eq!(status_data.counts.closed_with_evidence_count, 1);
        assert_eq!(status_data.counts.closed_without_evidence_count, 0);
    }

    #[test]
    fn inspect_queue_status_includes_active_tasks() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        git(project.path(), &["add", "backlog"]);
        git(project.path(), &["commit", "-m", "Add queue item"]);
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Active item".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");

        let result = inspect_queue_status(
            project.path(),
            InspectQueueStatusParams {
                root: None,
                limit: Some(5),
            },
        );
        let data = result.data.expect("queue status");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.counts.active_count, 1);
        assert_eq!(data.active_tasks.len(), 1);
        assert_eq!(data.active_tasks[0].item_id.as_deref(), Some("PROJ-001"));
        assert_eq!(data.active_tasks[0].task_id, task.id);
        assert_eq!(data.active_tasks[0].queue_state, "active");
        assert_eq!(data.top_blocked_items[0].queue_state, "active");
    }

    #[test]
    fn inspect_queue_status_ignores_active_tasks_for_closed_items() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        git(project.path(), &["add", "backlog"]);
        git(project.path(), &["commit", "-m", "Add queue item"]);
        create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Stale item".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        git(
            project.path(),
            &[
                "commit",
                "--allow-empty",
                "-m",
                "Close item",
                "-m",
                "Platypus-Closes: PROJ-001\nPlatypus-Verification: checked",
            ],
        );

        let result = inspect_queue_status(
            project.path(),
            InspectQueueStatusParams {
                root: None,
                limit: Some(5),
            },
        );
        let data = result.data.expect("queue status");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.counts.active_count, 0);
        assert!(data.active_tasks.is_empty());
        assert!(data.top_blocked_items.is_empty());
    }

    #[test]
    fn inspect_queue_status_preserves_pending_integration_for_closed_items() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        git(project.path(), &["add", "backlog"]);
        git(project.path(), &["commit", "-m", "Add queue item"]);
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Completed item".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        crate::tasks::claim_next_task(
            project.path(),
            crate::models::ClaimNextTaskParams {
                root: None,
                worker: Some("coder".to_string()),
                claimant: Some("runner-1".to_string()),
            },
        );
        crate::tasks::mark_task_running(project.path(), None, &task.id).expect("running");
        crate::tasks::finish_task(project.path(), None, &task.id, "completed").expect("completed");
        git(
            project.path(),
            &[
                "commit",
                "--allow-empty",
                "-m",
                "Close item",
                "-m",
                "Platypus-Closes: PROJ-001\nPlatypus-Verification: checked",
            ],
        );

        let result = inspect_queue_status(
            project.path(),
            InspectQueueStatusParams {
                root: None,
                limit: Some(5),
            },
        );
        let data = result.data.expect("queue status");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.queue_state, "completed_pending_integration");
        assert_eq!(data.recommended_tool, "inspect_integration_gates");
        assert_eq!(data.counts.active_count, 0);
        assert_eq!(data.counts.pending_integration_count, 1);
        assert_eq!(data.active_tasks.len(), 1);
        assert_eq!(data.active_tasks[0].task_id, task.id);
        assert_eq!(
            data.active_tasks[0].queue_state,
            "completed_pending_integration"
        );
        assert_eq!(
            data.active_tasks[0].recommended_tool,
            "inspect_integration_gates"
        );
    }

    #[test]
    fn inspect_queue_status_prioritizes_runtime_tasks_over_empty_backlog() {
        let project = backlog_project();
        init_git(project.path());
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Completed item without backlog file".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        crate::tasks::claim_next_task(
            project.path(),
            crate::models::ClaimNextTaskParams {
                root: None,
                worker: Some("coder".to_string()),
                claimant: Some("runner-1".to_string()),
            },
        );
        crate::tasks::mark_task_running(project.path(), None, &task.id).expect("running");
        crate::tasks::finish_task(project.path(), None, &task.id, "completed").expect("completed");

        let result = inspect_queue_status(
            project.path(),
            InspectQueueStatusParams {
                root: None,
                limit: Some(5),
            },
        );
        let data = result.data.expect("queue status");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.queue_state, "completed_pending_integration");
        assert_eq!(data.recommended_tool, "inspect_integration_gates");
        assert_eq!(data.active_tasks[0].task_id, task.id);
    }

    #[test]
    fn inspect_queue_status_keeps_item_blocker_state_when_pending_integration_is_inventory_only() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "Needs a plan");
        mark_worker_policy(project.path(), "PROJ-001", "task_plan");
        git(project.path(), &["add", "backlog"]);
        git(
            project.path(),
            &["commit", "-m", "Add planned backlog item"],
        );
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-002".to_string(),
                title: "Completed runtime-only item".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        crate::tasks::claim_next_task(
            project.path(),
            crate::models::ClaimNextTaskParams {
                root: None,
                worker: Some("coder".to_string()),
                claimant: Some("runner-1".to_string()),
            },
        );
        crate::tasks::mark_task_running(project.path(), None, &task.id).expect("running");
        crate::tasks::finish_task(project.path(), None, &task.id, "completed").expect("completed");

        let result = inspect_queue_status(
            project.path(),
            InspectQueueStatusParams {
                root: None,
                limit: Some(5),
            },
        );
        let data = result.data.expect("queue status");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.queue_state, "planning_blocked");
        assert_eq!(data.recommended_tool, "write_task_plan");
        assert_eq!(data.counts.pending_integration_count, 1);
    }

    #[test]
    fn inspect_queue_status_follows_first_blocked_item_before_later_integration_item() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "Needs a plan");
        mark_worker_policy(project.path(), "PROJ-001", "task_plan");
        write_item(project.path(), "PROJ-002", "Awaiting integration");
        git(project.path(), &["add", "backlog"]);
        git(project.path(), &["commit", "-m", "Add mixed backlog items"]);
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-002".to_string(),
                title: "Completed later item".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        crate::tasks::claim_next_task(
            project.path(),
            crate::models::ClaimNextTaskParams {
                root: None,
                worker: Some("coder".to_string()),
                claimant: Some("runner-1".to_string()),
            },
        );
        crate::tasks::mark_task_running(project.path(), None, &task.id).expect("running");
        crate::tasks::finish_task(project.path(), None, &task.id, "completed").expect("completed");

        let result = inspect_queue_status(
            project.path(),
            InspectQueueStatusParams {
                root: None,
                limit: Some(5),
            },
        );
        let data = result.data.expect("queue status");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.queue_state, "planning_blocked");
        assert_eq!(data.recommended_tool, "write_task_plan");
        assert_eq!(data.top_blocked_items[0].item_id, "PROJ-001");
        assert_eq!(data.counts.pending_integration_count, 1);
    }

    #[test]
    fn inspect_item_reports_blocked_item_without_creating_runtime_state() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        write_item_with_deps(project.path(), "PROJ-002", "Second item", &["PROJ-001"]);
        git(project.path(), &["add", "backlog"]);
        git(
            project.path(),
            &["commit", "-m", "Add dependent backlog items"],
        );

        let result = inspect_item(
            project.path(),
            InspectItemParams {
                root: None,
                item_id: "PROJ-002".to_string(),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("item");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.item.item_id, "PROJ-002");
        assert_eq!(data.queue_state, "dependency_blocked");
        assert_eq!(data.recommended_tool, "inspect_item");
        assert_eq!(data.plan.status, "not_required");
        assert!(data.queue.is_none());
        assert!(data.findings.is_empty());
        assert!(data.evidence.is_empty());
        assert!(data.markdown.sections.contains(&"Goal".to_string()));
    }

    #[test]
    fn inspect_work_queue_reports_completed_items_awaiting_integration() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Completed item".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        crate::tasks::claim_next_task(
            project.path(),
            crate::models::ClaimNextTaskParams {
                root: None,
                worker: Some("coder".to_string()),
                claimant: Some("runner-1".to_string()),
            },
        );
        crate::tasks::mark_task_running(project.path(), None, &task.id).expect("running");
        crate::tasks::finish_task(project.path(), None, &task.id, "completed").expect("completed");

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.queue_state, "completed_pending_integration");
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].queue_state, "completed_pending_integration");
        assert_eq!(data.items[0].recommended_tool, "integrate_worker_result");
        assert!(data.items[0].reason.contains("awaiting integration"));
        assert!(data
            .schemas_likely_needed_next
            .iter()
            .any(|hint| hint.tool_name == "inspect_integration_gates"));
    }

    #[test]
    fn inspect_work_queue_applies_limit_before_active_task_filtering() {
        let project = backlog_project();
        init_git(project.path());
        for index in 1..=11 {
            let item_id = format!("PROJ-{index:03}");
            write_item(project.path(), &item_id, &format!("Item {index}"));
        }
        git(project.path(), &["add", "backlog"]);
        git(project.path(), &["commit", "-m", "Add queue items"]);
        for index in 1..=10 {
            let item_id = format!("PROJ-{index:03}");
            create_task_record(
                project.path(),
                None,
                NewTask {
                    source_item_id: item_id,
                    title: format!("Active item {index}"),
                    worker: Some("coder".to_string()),
                },
            )
            .expect("task");
        }

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(1),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].candidate.item_id, "PROJ-001");
        assert_eq!(data.items[0].queue_state, "active");
        assert_eq!(data.active_count, 1);
        assert_eq!(data.recommended_tool, "prepare_worker_handoff");
    }

    #[test]
    fn inspect_work_queue_honors_limit_above_one_hundred() {
        let project = backlog_project();
        init_git(project.path());
        for index in 1..=101 {
            let item_id = format!("PROJ-{index:03}");
            write_item(project.path(), &item_id, &format!("Item {index}"));
        }
        git(project.path(), &["add", "backlog"]);
        git(project.path(), &["commit", "-m", "Add large queue"]);
        for index in 1..=100 {
            let item_id = format!("PROJ-{index:03}");
            create_task_record(
                project.path(),
                None,
                NewTask {
                    source_item_id: item_id,
                    title: format!("Active item {index}"),
                    worker: Some("coder".to_string()),
                },
            )
            .expect("task");
        }

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(101),
                require_task_plan: Some(false),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.items.len(), 101);
        assert_eq!(data.items[0].candidate.item_id, "PROJ-001");
        assert_eq!(data.items[100].candidate.item_id, "PROJ-101");
        assert_eq!(data.active_count, 100);
        assert_eq!(data.recommended_tool, "prepare_worker_handoff");
    }

    fn backlog_project() -> TempDir {
        let project = TempDir::new().expect("temp dir");
        write_ready_config(project.path());
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

    fn write_ready_config(root: &Path) {
        fs::write(root.join("platy.yaml"), "project: test\n").expect("config");
    }

    fn init_git(root: &Path) {
        git(root, &["init"]);
        git(root, &["config", "user.name", "Platypus Test"]);
        git(root, &["config", "user.email", "platypus@example.invalid"]);
        fs::write(root.join("README.md"), "# Test\n").expect("readme");
        git(root, &["add", "--all"]);
        git(root, &["commit", "-m", "Initial commit"]);
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

    fn write_item(root: &Path, id: &str, title: &str) {
        write_item_with(
            root,
            ItemFixture {
                id,
                title,
                item_type: "feature",
                area: "general",
                owned_surfaces: &["src/lib.rs"],
            },
        );
    }

    fn mark_worker_policy(root: &Path, id: &str, planning_gate: &str) {
        let path = root.join("backlog/items").join(format!("{id}.md"));
        let text = fs::read_to_string(&path).expect("item");
        let text = text.replace(
            "\n---\n\n#",
            &format!("\nexecution_path: worker_handoff\nplanning_gate: {planning_gate}\n---\n\n#"),
        );
        fs::write(path, text).expect("item policy");
    }

    fn write_item_with_deps(root: &Path, id: &str, title: &str, depends_on: &[&str]) {
        let depends = if depends_on.is_empty() {
            "[]".to_string()
        } else {
            format!(
                "[{}]",
                depends_on
                    .iter()
                    .map(|dependency| format!("\"{dependency}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        write_item_raw(
            root,
            id,
            title,
            "feature",
            "general",
            &["src/lib.rs"],
            &depends,
        );
    }

    struct ItemFixture<'a> {
        id: &'a str,
        title: &'a str,
        item_type: &'a str,
        area: &'a str,
        owned_surfaces: &'a [&'a str],
    }

    fn write_item_with(root: &Path, fixture: ItemFixture<'_>) {
        write_item_raw(
            root,
            fixture.id,
            fixture.title,
            fixture.item_type,
            fixture.area,
            fixture.owned_surfaces,
            "[]",
        );
    }

    fn write_item_raw(
        root: &Path,
        id: &str,
        title: &str,
        item_type: &str,
        area: &str,
        owned_surfaces: &[&str],
        depends_on: &str,
    ) {
        let surfaces = owned_surfaces
            .iter()
            .map(|surface| format!("- {surface}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            root.join("backlog/items").join(format!("{}.md", id)),
            format!(
                r#"---
id: {id}
title: {title}
priority: P1
type: {item_type}
area: {area}
epic: general
depends_on: {depends_on}
owned_surfaces:
{surfaces}
---

# {id} {title}

## Goal

Deliver the item.

## Implementation Contract

Keep the change scoped.

## Acceptance

- The item is implemented.
"#,
                id = id,
                title = title,
                item_type = item_type,
                area = area,
                depends_on = depends_on,
                surfaces = surfaces
            ),
        )
        .expect("item");
    }
}
