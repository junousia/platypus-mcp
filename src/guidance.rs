use crate::{
    approvals, backlog, config,
    git_readiness::{inspect_git_readiness, GitReadinessStatus},
    models::{
        ActionResult, ActionStatus, BacklogCandidate, BacklogListData, ClassifyPlanningNeedsParams,
        ClassifyWorkflowFitParams, InspectWorkQueueParams, NextSafeActionData,
        NextSafeActionParams, PlanningApprovalState, PlanningClassification,
        PlanningClassificationData, TaskPlanQueryParams, WorkQueueData, WorkQueueItem,
        WorkQueuePlanState, WorkflowFitData,
    },
    state::{sqlite::SqliteProjectState, NextSafeActionQuery, ProjectState},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

pub fn next_safe_action(
    default_root: &Path,
    params: NextSafeActionParams,
) -> ActionResult<NextSafeActionData> {
    let action = "next_safe_action";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not inspect next safe action.",
                error.to_string(),
            )
        }
    };
    let readiness = inspect_git_readiness(state.root(), false);
    if !readiness.ready() {
        let reason = readiness
            .next_action
            .clone()
            .unwrap_or_else(|| "Inspect Git setup before dispatching work.".to_string());
        return ActionResult::completed(
            action,
            readiness.summary.clone(),
            NextSafeActionData {
                root: state.root().display().to_string(),
                recommended_tool: "doctor_snapshot".to_string(),
                summary: readiness.summary,
                reason,
                params: BTreeMap::from([(
                    "root".to_string(),
                    Value::String(state.root().display().to_string()),
                )]),
            },
        );
    }
    if let Some(reason) = missing_worker_profile_reason(default_root, state.root()) {
        let root_string = state.root().to_string_lossy().to_string();
        return ActionResult::completed(
            action,
            "Runnable backlog work needs a ready worker profile.",
            NextSafeActionData {
                root: root_string.clone(),
                recommended_tool: "configure_agent_profile".to_string(),
                summary: "Runnable backlog work exists, but no ready worker profile is configured."
                    .to_string(),
                reason,
                params: worker_profile_params(&root_string),
            },
        );
    }
    match state.next_safe_action(NextSafeActionQuery::default()) {
        Ok(snapshot) => {
            let snapshot = verification_policy_adjusted_snapshot(state.root(), snapshot);
            ActionResult::completed(
                action,
                snapshot.summary.clone(),
                NextSafeActionData {
                    root: state.root().display().to_string(),
                    recommended_tool: snapshot.recommended_tool,
                    summary: snapshot.summary,
                    reason: snapshot.reason,
                    params: snapshot.params,
                },
            )
        }
        Err(error) => ActionResult::failed(
            action,
            "Could not inspect next safe action.",
            error.to_string(),
        ),
    }
}

fn missing_worker_profile_reason(default_root: &Path, root: &Path) -> Option<String> {
    let readiness = config::inspect_agent_profile_readiness(root).ok()?;
    if readiness.worker_ready() {
        return None;
    }
    let listed =
        backlog::list_backlog(default_root, Some(root.to_string_lossy().as_ref()), Some(1));
    let has_runnable = matches!(
        listed,
        ActionResult {
            status: ActionStatus::Completed,
            data: Some(BacklogListData { candidates, .. }),
            ..
        } if !candidates.is_empty()
    );
    has_runnable.then(|| {
        format!(
            "{} Manual handoff remains possible: call dispatch_ready_work with prepare_handoffs=true and have an MCP host or external agent execute the returned assignment.",
            config::worker_profile_setup_guidance()
        )
    })
}

fn verification_policy_adjusted_snapshot(
    root: &Path,
    mut snapshot: crate::state::SafeActionSnapshot,
) -> crate::state::SafeActionSnapshot {
    if snapshot.recommended_tool != "record_verification_evidence" {
        return snapshot;
    }
    let Ok(config) = crate::config::effective_workflow_config(root) else {
        return snapshot;
    };
    if config.require_verification_evidence {
        return snapshot;
    }
    let Some(task_id) = snapshot
        .params
        .get("source_task_id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
    else {
        return snapshot;
    };
    snapshot.recommended_tool = "integrate_worker_result".to_string();
    snapshot.summary = format!("Completed task `{task_id}` can be integrated.");
    snapshot.reason = "Workflow policy does not require separate verification evidence; integrate now or record verification evidence first if useful.".to_string();
    let root_string = root.to_string_lossy().to_string();
    snapshot.params = map_params([("root", root_string.as_str()), ("task_id", &task_id)]);
    snapshot
        .params
        .insert("allow_unverified".to_string(), Value::Bool(true));
    snapshot
}

pub fn inspect_work_queue(
    default_root: &Path,
    params: InspectWorkQueueParams,
) -> ActionResult<WorkQueueData> {
    let action = "inspect_work_queue";
    let require_task_plan = params.require_task_plan.unwrap_or(false);
    let require_planning_approval = params.require_planning_approval.unwrap_or(false);
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

    let active_source_items = active_source_item_ids(Path::new(&root));
    let mut active_item_ids = Vec::new();
    let mut active_filtered = 0usize;
    let filtered_candidates = candidates
        .into_iter()
        .filter(|candidate| {
            let blocked = active_source_items.contains(&candidate.item_id);
            if blocked {
                active_filtered += 1;
                active_item_ids.push(candidate.item_id.clone());
            }
            !blocked
        })
        .collect::<Vec<_>>();
    let mut items: Vec<WorkQueueItem> = filtered_candidates
        .into_iter()
        .take(requested_limit)
        .enumerate()
        .map(|(index, candidate)| {
            work_queue_item(
                default_root,
                &root,
                index + 1,
                candidate,
                require_task_plan,
                require_planning_approval,
            )
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
                preflight_warnings.push(
                    "Info: manager workspace has only backlog artifacts pending. Enable auto_commit_artifacts=true if you want dispatch_ready_work to auto-commit these artifacts."
                        .to_string(),
                );
            } else {
                let warning = readiness.next_action.clone().unwrap_or_else(|| {
                    "Commit, stash, or discard local changes before dispatch.".to_string()
                });
                let reason = format!(
                    "{warning} Dispatch will be blocked until the manager workspace is clean."
                );
                preflight_warnings.push(reason.clone());
                dispatch_blocker = Some((
                    "next_safe_action".to_string(),
                    format!(
                        "{reason} After cleanup, rerun inspect_work_queue or dispatch_ready_work."
                    ),
                ));
                for item in &mut items {
                    if item.ready_to_dispatch {
                        dispatch_blocker_applies = true;
                        item.ready_to_dispatch = false;
                        item.recommended_tool = "next_safe_action".to_string();
                        item.reason = "Planning is ready, but dispatch is currently blocked by local manager workspace changes.".to_string();
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
                if item.ready_to_dispatch {
                    dispatch_blocker_applies = true;
                    item.ready_to_dispatch = false;
                    item.recommended_tool = "doctor_snapshot".to_string();
                    item.reason = "Planning is ready, but dispatch is currently blocked because Git is not ready.".to_string();
                }
            }
        }
    }
    let initial_ready_count = items.iter().filter(|item| item.ready_to_dispatch).count();
    let agent_readiness = config::inspect_agent_profile_readiness(Path::new(&root)).ok();
    let worker_ready = agent_readiness
        .as_ref()
        .is_some_and(|readiness| readiness.worker_ready());
    let ready_worker_profiles = agent_readiness
        .as_ref()
        .map(|readiness| readiness.ready_worker_count)
        .unwrap_or(0);
    let worker_profile_blocker_applies =
        !items.is_empty() && initial_ready_count > 0 && !worker_ready;
    if let Some(readiness) = agent_readiness.as_ref() {
        if !readiness.worker_ready() {
            preflight_warnings.extend(readiness.warnings.iter().filter_map(|warning| {
                warning
                    .to_ascii_lowercase()
                    .contains("worker")
                    .then(|| warning.clone())
            }));
        }
    } else if !items.is_empty() {
        preflight_warnings.push(
            "Could not inspect worker profiles. Run doctor_snapshot or configure_agent_profile before managed dispatch."
                .to_string(),
        );
    }
    if worker_profile_blocker_applies {
        for item in &mut items {
            if item.ready_to_dispatch {
                item.ready_to_dispatch = false;
                item.recommended_tool = "configure_agent_profile".to_string();
                item.reason = "Planning is ready, but no ready worker profile is configured for managed execution. Configure a worker profile, or explicitly use manual handoff with dispatch_ready_work prepare_handoffs=true.".to_string();
            }
        }
    }
    let ready_count = items.iter().filter(|item| item.ready_to_dispatch).count();
    let blocked_count = items.len().saturating_sub(ready_count);
    if active_filtered > 0 {
        preflight_warnings.push(format!(
            "{active_filtered} backlog item(s) already have active tasks and were hidden from ready dispatch candidates."
        ));
    }
    let (mut recommended_tool, mut reason, mut params) = recommended_queue_action(&root, &items);
    if let Some((blocked_tool, blocked_reason)) =
        dispatch_blocker.filter(|_| dispatch_blocker_applies)
    {
        recommended_tool = blocked_tool;
        reason = blocked_reason;
        params = map_params([("root", root.as_str())]);
    } else if items.is_empty() && active_filtered > 0 {
        recommended_tool = "next_safe_action".to_string();
        reason = format!(
            "{active_filtered} backlog item(s) already have active tasks. Use next_safe_action or inspect_task to continue active work instead of creating new backlog items."
        );
        params = map_params([("root", root.as_str())]);
    } else if worker_profile_blocker_applies {
        recommended_tool = "configure_agent_profile".to_string();
        reason = format!(
            "Runnable work exists, but no ready worker profile is configured. {} Manual handoff remains possible with dispatch_ready_work prepare_handoffs=true if an external agent will execute the returned assignment.",
            config::worker_profile_setup_guidance()
        );
        params = worker_profile_params(&root);
    }
    let summary = if items.is_empty() {
        if active_filtered > 0 {
            format!(
                "No dispatchable backlog items; {active_filtered} backlog item(s) already have active tasks."
            )
        } else {
            "No runnable backlog items.".to_string()
        }
    } else {
        format!(
            "{} runnable backlog item(s) inspected: {} ready, {} blocked by planning.",
            items.len(),
            ready_count,
            blocked_count
        )
    };
    let status = if items.is_empty() && active_filtered == 0 {
        ActionStatus::Skipped
    } else {
        ActionStatus::Completed
    };
    ActionResult {
        action: action.to_string(),
        status,
        summary: summary.clone(),
        next_action: Some(reason.clone()),
        data: Some(WorkQueueData {
            root,
            require_task_plan,
            ready_count,
            blocked_count,
            active_count: active_filtered,
            active_item_ids,
            preflight_warnings,
            worker_ready,
            ready_worker_profiles,
            recommended_tool,
            summary,
            reason,
            params,
            items,
        }),
        error: None,
    }
}

fn active_source_item_ids(root: &Path) -> BTreeSet<String> {
    let storage = match crate::storage::connect_existing_read_only(root, None) {
        Ok(Some(storage)) => storage,
        Ok(None) | Err(_) => return BTreeSet::new(),
    };
    match storage.repository().tasks().active_source_items() {
        Ok(items) => items.into_iter().collect(),
        Err(_) => BTreeSet::new(),
    }
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

pub fn classify_planning_needs(
    default_root: &Path,
    params: ClassifyPlanningNeedsParams,
) -> ActionResult<PlanningClassificationData> {
    let action = "classify_planning_needs";
    let listed = backlog::list_backlog(default_root, params.root.as_deref(), params.limit);
    let (root, candidates) = match listed {
        ActionResult {
            status: ActionStatus::Completed | ActionStatus::Skipped,
            data: Some(BacklogListData { root, candidates }),
            ..
        } => (root, candidates),
        ActionResult { summary, error, .. } => {
            return ActionResult::failed(
                action,
                "Could not classify planning needs.",
                error.unwrap_or(summary),
            )
        }
    };
    let item_filter = params
        .item_id
        .as_deref()
        .map(|value| value.to_ascii_uppercase());
    let classifications = candidates
        .iter()
        .filter(|candidate| {
            item_filter
                .as_ref()
                .map(|filter| candidate.item_id == *filter)
                .unwrap_or(true)
        })
        .map(classify_candidate)
        .collect::<Vec<_>>();
    let returned = classifications.len();

    if item_filter.is_some() && returned == 0 {
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No runnable backlog item matched the requested item.".to_string(),
            next_action: Some(
                "Use list_backlog or inspect_work_queue to inspect runnable items.".to_string(),
            ),
            data: Some(PlanningClassificationData {
                root,
                classifications,
                returned,
            }),
            error: None,
        };
    }

    ActionResult::completed(
        action,
        format!("Classified {returned} backlog item(s)."),
        PlanningClassificationData {
            root,
            classifications,
            returned,
        },
    )
}

pub fn classify_workflow_fit(
    default_root: &Path,
    params: ClassifyWorkflowFitParams,
) -> ActionResult<WorkflowFitData> {
    let action = "classify_workflow_fit";
    let root = match crate::project::paths::resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not classify workflow fit.",
                error.to_string(),
            )
        }
    };
    let goal = params.goal.trim();
    if goal.is_empty() {
        return ActionResult::failed(
            action,
            "Could not classify workflow fit.",
            "goal is required",
        );
    }

    let inventory =
        backlog::inspect_backlog_inventory(&root, Some(root.to_string_lossy().as_ref()), None);
    let (backlog_items, runnable_backlog_items) = match inventory {
        ActionResult {
            data: Some(data),
            status: ActionStatus::Completed,
            ..
        } => (data.total, data.runnable),
        _ => (0, 0),
    };
    let meaningful_files = meaningful_project_entries(&root);
    let goal_lower = goal.to_ascii_lowercase();
    let owned_surfaces = params
        .owned_surfaces
        .iter()
        .map(|surface| surface.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let touches_high_risk_surface = high_risk_surface(&owned_surfaces);
    let starter_app_goal = keyword_score(
        &goal_lower,
        &[
            "fastapi",
            "react",
            "vite",
            "starter app",
            "starter project",
            "simple webapp",
            "simple web app",
            "scaffold",
            "new app",
        ],
    ) >= 2;
    let scaffold_score = keyword_score(
        &goal_lower,
        &[
            "scaffold",
            "starter",
            "new project",
            "new app",
            "simple webapp",
            "simple web app",
            "hello world",
            "vite",
            "create react",
            "fastapi/react",
            "fastapi react",
            "bootstrap",
            "minimal app",
        ],
    );
    let explicit_structured_score = keyword_score(
        &goal_lower,
        &[
            "backlog",
            "roadmap",
            "traceability",
            "evidence",
            "worker",
            "approval",
            "security",
            "migration",
            "refactor",
            "production",
            "ci",
            "integration",
            "multi-step",
            "multiple",
        ],
    );
    let structured_score = explicit_structured_score + owned_surfaces.len();

    let mut reasons = Vec::new();
    if scaffold_score > 0 {
        reasons.push("goal looks like initial scaffolding or starter-app creation".to_string());
    }
    if structured_score > 0 {
        reasons.push("goal asks for structured, multi-step, or traceable work".to_string());
    }
    if touches_high_risk_surface {
        reasons.push("goal touches high-risk lifecycle or persistence surfaces".to_string());
    }
    if backlog_items > 0 {
        reasons.push(format!("{backlog_items} backlog item(s) already exist"));
    }
    if meaningful_files <= 2 {
        reasons.push("project root has little existing product surface".to_string());
    } else {
        reasons.push(format!(
            "project root already has {meaningful_files} meaningful top-level entries"
        ));
    }

    let recommended_mode =
        if backlog_items > 0 || explicit_structured_score >= 2 || touches_high_risk_surface {
            "platypus_workflow"
        } else if (starter_app_goal || scaffold_score > 0) && meaningful_files <= 2 {
            "direct_scaffold"
        } else if scaffold_score > 0 || starter_app_goal {
            "hybrid"
        } else {
            "platypus_workflow"
        };
    let (summary, next_action) = match recommended_mode {
        "direct_scaffold" => (
            "Direct scaffold is the best first step.".to_string(),
            "No Platypus scaffold tool is available for direct_scaffold. Use the host's native file edits or scaffold command first, commit the baseline, then create backlog items for follow-up work.".to_string(),
        ),
        "hybrid" => (
            "Use a hybrid flow: direct scaffold for the first files, then Platypus for follow-up work.".to_string(),
            "No Platypus scaffold tool is available for the direct part of hybrid mode. Use the host's native file edits or scaffold command for the initial files, commit them, then use Platypus backlog and task tools for the next implementation slices.".to_string(),
        ),
        _ => (
            "Use the full Platypus workflow.".to_string(),
            "Create or inspect backlog items, classify planning needs, validate task plans when required, then dispatch through worktrees.".to_string(),
        ),
    };

    ActionResult::completed(
        action,
        summary.clone(),
        WorkflowFitData {
            root: root.display().to_string(),
            recommended_mode: recommended_mode.to_string(),
            summary,
            reasons,
            next_action,
            backlog_items,
            runnable_backlog_items,
        },
    )
}

fn work_queue_item(
    default_root: &Path,
    root: &str,
    position: usize,
    candidate: BacklogCandidate,
    require_task_plan: bool,
    require_planning_approval: bool,
) -> WorkQueueItem {
    let planning = classify_candidate(&candidate);
    let mut plan = task_plan_state(default_root, root, &candidate.item_id);
    let plan_valid = plan.status == "valid";
    let planning_required = require_task_plan && planning.required_mode != "direct";
    if !planning_required && plan.status == "missing" {
        plan.status = "not_required".to_string();
        plan.errors.clear();
    }
    let plan_ready = !planning_required || plan_valid;
    let approval_required = require_planning_approval && planning.required_mode != "direct";
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
    } else if require_planning_approval {
        Some(PlanningApprovalState {
            item_id: candidate.item_id.clone(),
            required: false,
            approved: true,
            approval_id: None,
            status: None,
            reason: "Direct work does not require planning approval.".to_string(),
        })
    } else {
        None
    };
    let approval_ready = planning_approval
        .as_ref()
        .is_none_or(|state| !state.required || state.approved);
    let ready_to_dispatch = plan_ready && approval_ready;
    let (recommended_tool, reason) = if ready_to_dispatch {
        (
            "dispatch_ready_work".to_string(),
            "Backlog item is runnable.".to_string(),
        )
    } else if !plan_ready && plan.status == "missing" {
        (
            "draft_task_plan".to_string(),
            "A task plan is required before dispatch.".to_string(),
        )
    } else if !plan_ready {
        (
            "validate_task_plan".to_string(),
            "Task plan must be fixed before dispatch.".to_string(),
        )
    } else if approval_required {
        (
            "request_planning_approval".to_string(),
            planning_approval
                .as_ref()
                .map(|state| state.reason.clone())
                .unwrap_or_else(|| "Planning approval is required before dispatch.".to_string()),
        )
    } else {
        (
            "inspect_work_queue".to_string(),
            "Backlog item is not ready to dispatch.".to_string(),
        )
    };
    WorkQueueItem {
        position,
        candidate,
        planning,
        plan,
        planning_approval,
        ready_to_dispatch,
        recommended_tool,
        reason,
    }
}

fn keyword_score(text: &str, keywords: &[&str]) -> usize {
    keywords
        .iter()
        .filter(|keyword| text.contains(**keyword))
        .count()
}

fn meaningful_project_entries(root: &Path) -> usize {
    fs::read_dir(root)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !matches!(
                name.as_ref(),
                ".git"
                    | ".platy"
                    | "target"
                    | "node_modules"
                    | ".DS_Store"
                    | "AGENTS.md"
                    | "CLAUDE.md"
                    | "WORKFLOW.md"
                    | "platy.yaml"
                    | "backlog"
            )
        })
        .count()
}

pub(crate) fn classify_candidate(candidate: &BacklogCandidate) -> PlanningClassification {
    let mut reasons = Vec::new();
    let mut mode = "direct";
    let text = format!(
        "{} {} {} {}",
        candidate.title, candidate.area, candidate.item_type, candidate.priority
    )
    .to_ascii_lowercase();
    let surfaces = candidate
        .owned_surfaces
        .iter()
        .map(|surface| surface.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let simple_scaffold = is_simple_single_surface_scaffold(&text, &surfaces)
        || is_simple_dual_surface_scaffold(&text, &surfaces);

    if candidate.owned_surfaces.len() > 1 && !simple_scaffold {
        mode = "standard";
        reasons.push("touches multiple owned surfaces".to_string());
    }
    if !simple_scaffold
        && matches!(
            candidate.item_type.as_str(),
            "foundation" | "feature" | "safety" | "ux"
        )
    {
        mode = max_mode(mode, "standard");
        reasons.push(format!(
            "{} item type usually needs planned execution",
            candidate.item_type
        ));
    }
    if surfaces
        .iter()
        .any(|surface| surface == "src/server.rs" || surface == "src/models.rs")
    {
        mode = max_mode(mode, "standard");
        reasons.push("changes MCP tool schema or server surface".to_string());
    }
    if text.contains("workflow") {
        mode = max_mode(mode, "standard");
        reasons.push("changes user or developer workflow".to_string());
    }
    if text.contains("security") || text.contains("approval") || text.contains("safe") {
        mode = "full";
        reasons.push("touches security or approval-sensitive behavior".to_string());
    }
    if high_risk_surface(&surfaces) {
        mode = "full";
        reasons.push("touches high-risk lifecycle or persistence surfaces".to_string());
    }
    if text.contains("migration")
        || text.contains("daemon")
        || text.contains("network")
        || text.contains("protocol")
        || text.contains("architecture")
    {
        mode = "full";
        reasons.push(
            "mentions architecture, protocol, migration, daemon, or network behavior".to_string(),
        );
    }
    if reasons.is_empty() {
        if simple_scaffold {
            reasons.push("single-surface scaffold/setup item can be handled directly".to_string());
        } else {
            reasons.push("small low-risk item can be handled directly".to_string());
        }
    }

    PlanningClassification {
        item_id: candidate.item_id.clone(),
        required_mode: mode.to_string(),
        required_artifact: match mode {
            "standard" | "full" => Some(format!("backlog/plans/{}.yaml", candidate.item_id)),
            _ => None,
        },
        reasons,
    }
}

fn is_simple_single_surface_scaffold(text: &str, surfaces: &[String]) -> bool {
    surfaces.len() == 1
        && !high_risk_surface(surfaces)
        && [
            "scaffold",
            "setup",
            "set up",
            "bootstrap",
            "initial",
            "starter",
        ]
        .iter()
        .any(|keyword| text.contains(keyword))
}

fn is_simple_dual_surface_scaffold(text: &str, surfaces: &[String]) -> bool {
    if surfaces.len() != 2 || high_risk_surface(surfaces) {
        return false;
    }
    let normalized = surfaces
        .iter()
        .map(|surface| surface.trim_end_matches('/').to_string())
        .collect::<BTreeSet<_>>();
    if normalized != BTreeSet::from(["backend".to_string(), "frontend".to_string()]) {
        return false;
    }
    [
        "scaffold",
        "setup",
        "set up",
        "bootstrap",
        "initial",
        "starter",
        "placeholder",
        "baseline",
        "simple",
        "create",
    ]
    .iter()
    .any(|keyword| text.contains(keyword))
}

fn max_mode(current: &str, candidate: &str) -> &'static str {
    if current == "full" || candidate == "full" {
        "full"
    } else if current == "standard" || candidate == "standard" {
        "standard"
    } else {
        "direct"
    }
}

fn high_risk_surface(surfaces: &[String]) -> bool {
    surfaces.iter().any(|surface| {
        surface.starts_with("src/storage")
            || surface.starts_with("src/workspace")
            || surface.starts_with("src/approvals")
            || surface.starts_with("src/assignments")
            || surface.starts_with("src/runner")
            || surface.starts_with("src/workers")
            || surface.starts_with("src/reconcile")
            || surface == "src/dispatch.rs"
    })
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
            "create_backlog_item".to_string(),
            "Create or draft a backlog item before dispatching work.".to_string(),
            map_params([("root", root), ("summary", "Describe the work item.")]),
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
        "draft_task_plan" | "validate_task_plan" => {
            params.insert("item_id".to_string(), Value::String(item_id.to_string()));
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

fn worker_profile_params(root: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("root".to_string(), Value::String(root.to_string())),
        ("name".to_string(), Value::String("coder".to_string())),
        ("role".to_string(), Value::String("worker".to_string())),
        ("harness".to_string(), Value::String("codex".to_string())),
        ("executable".to_string(), Value::String("codex".to_string())),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approvals::{approval_respond, request_planning_approval};
    use crate::models::{ApprovalRespondParams, RequestPlanningApprovalParams};
    use crate::tasks::{create_task_record, NewTask};
    use std::{fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn recommends_preparing_queued_task() {
        let project = TempDir::new().expect("temp dir");
        init_git(project.path());
        create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Queued task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");

        let result = next_safe_action(project.path(), NextSafeActionParams { root: None });
        let data = result.data.expect("next action");

        assert_eq!(data.recommended_tool, "prepare_worker_handoff");
        assert_eq!(data.params["task_id"], "PROJ-001-T001");
    }

    #[test]
    fn recommends_preparing_claimed_task_before_dispatching_more_backlog() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Claimed task".to_string(),
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

        let result = next_safe_action(project.path(), NextSafeActionParams { root: None });
        let data = result.data.expect("next action");

        assert_eq!(data.recommended_tool, "prepare_worker_handoff");
        assert_eq!(data.params["task_id"], "PROJ-001-T001");
        assert!(data.summary.contains("already claimed"));
    }

    #[test]
    fn next_safe_action_integrates_completed_task_when_verification_not_required() {
        let project = TempDir::new().expect("temp dir");
        init_git(project.path());
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Completed task".to_string(),
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

        let result = next_safe_action(project.path(), NextSafeActionParams { root: None });
        let data = result.data.expect("next action");

        assert_eq!(data.recommended_tool, "integrate_worker_result");
        assert_eq!(data.params["task_id"], task.id);
        assert_eq!(data.params["allow_unverified"], true);
    }

    #[test]
    fn skips_active_tasks_for_closed_backlog_items() {
        let project = backlog_project();
        init_git_with_closed_item(project.path(), "PROJ-001");
        write_item(project.path(), "PROJ-001", "Closed item");
        write_item(project.path(), "PROJ-002", "Open item");
        create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Stale queued task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");

        let result = next_safe_action(project.path(), NextSafeActionParams { root: None });
        let data = result.data.expect("next action");

        assert_eq!(data.recommended_tool, "dispatch_ready_work");
        assert!(data.summary.contains("PROJ-002"));
    }

    #[test]
    fn next_safe_action_reports_missing_git_before_dispatch() {
        let project = backlog_project();
        write_item(project.path(), "PROJ-001", "First item");

        let result = next_safe_action(project.path(), NextSafeActionParams { root: None });
        let data = result.data.expect("next action");

        assert_eq!(data.recommended_tool, "doctor_snapshot");
        assert!(data.summary.contains("not initialized"));
        assert!(data.reason.contains("git init"));
    }

    #[test]
    fn next_safe_action_reports_unborn_head_before_dispatch() {
        let project = backlog_project();
        git(project.path(), &["init"]);
        write_item(project.path(), "PROJ-001", "First item");

        let result = next_safe_action(project.path(), NextSafeActionParams { root: None });
        let data = result.data.expect("next action");

        assert_eq!(data.recommended_tool, "doctor_snapshot");
        assert!(data.summary.contains("no initial commit"));
        assert!(data.reason.contains("initial commit"));
    }

    #[test]
    fn next_safe_action_recommends_worker_profile_when_runnable_work_exists() {
        let project = backlog_project();
        fs::write(project.path().join("platy.yaml"), "project: test\n").expect("config");
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
        git(project.path(), &["add", "backlog", "platy.yaml"]);
        git(project.path(), &["commit", "-m", "Add backlog item"]);

        let result = next_safe_action(project.path(), NextSafeActionParams { root: None });
        let data = result.data.expect("next action");

        assert_eq!(data.recommended_tool, "configure_agent_profile");
        assert!(data.reason.contains("configure_agent_profile"));
        assert_eq!(data.params["role"], "worker");
        assert_eq!(data.params["name"], "coder");
    }

    #[test]
    fn next_safe_action_prefers_queue_inspection_when_backlog_exists_but_is_not_runnable() {
        let project = backlog_project();
        init_git(project.path());
        fs::write(
            project.path().join("backlog/items/PROJ-001.md"),
            r#"---
id: PROJ-001
title: Closed item
status: done
priority: P1
type: feature
area: backend
epic: general
suggested_worker: coder
owned_surfaces:
  - src
acceptance:
  - Already complete
---

## Goal
- This item is already closed.

## Implementation Contract
- No further action needed.
"#,
        )
        .expect("write blocked item");

        let result = next_safe_action(project.path(), NextSafeActionParams { root: None });
        let data = result.data.expect("next action");
        assert_eq!(data.recommended_tool, "inspect_work_queue");
        assert!(data.summary.contains("Backlog has 1 item"));
    }

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
        assert_eq!(data.recommended_tool, "doctor_snapshot");
        assert!(!data.items[0].ready_to_dispatch);
        assert!(data.reason.contains("git init"));
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
        assert_eq!(data.recommended_tool, "doctor_snapshot");
        assert!(!data.items[0].ready_to_dispatch);
        assert!(data.reason.contains("initial commit"));
    }

    #[test]
    fn inspect_work_queue_can_require_planning_approval() {
        let project = backlog_project();
        write_item(project.path(), "PROJ-001", "Ready item");
        init_git(project.path());

        let blocked = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: None,
                require_planning_approval: Some(true),
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
                require_planning_approval: Some(true),
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
        write_item(project.path(), "PROJ-001", "First item");

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
        assert_eq!(data.recommended_tool, "draft_task_plan");
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].plan.status, "missing");
        assert!(!data.items[0].ready_to_dispatch);
        assert_eq!(data.params["item_id"], "PROJ-001");
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
                require_task_plan: Some(true),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(data.recommended_tool, "dispatch_ready_work");
        assert_eq!(data.items[0].planning.required_mode, "direct");
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
                require_task_plan: Some(true),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(data.recommended_tool, "next_safe_action");
        assert!(!data.preflight_warnings.is_empty());
        assert!(data.reason.contains("workspace"));
        assert!(data.reason.contains("Dispatch will be blocked"));
        assert!(data.reason.contains("After cleanup"));
    }

    #[test]
    fn inspect_work_queue_reports_missing_worker_profile_for_runnable_work() {
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
                require_task_plan: Some(true),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(data.recommended_tool, "configure_agent_profile");
        assert!(!data.worker_ready);
        assert_eq!(data.ready_worker_profiles, 0);
        assert!(data
            .preflight_warnings
            .iter()
            .any(|warning| warning.contains("No worker profile")));
        assert!(!data.items[0].ready_to_dispatch);
        assert_eq!(data.items[0].recommended_tool, "configure_agent_profile");
    }

    #[test]
    fn inspect_work_queue_reports_non_ready_worker_profile() {
        let project = backlog_project();
        fs::write(
            project.path().join("platy.yaml"),
            r#"agents:
  profiles:
    manager:
      role: manager
      harness: codex
      executable: git
    coder:
      role: worker
      harness: codex
      executable: definitely-missing-platypus-agent
"#,
        )
        .expect("config");
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
                require_task_plan: Some(true),
                require_planning_approval: None,
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(data.recommended_tool, "configure_agent_profile");
        assert!(!data.worker_ready);
        assert!(data
            .preflight_warnings
            .iter()
            .any(|warning| warning.contains("none are ready")));
    }

    #[test]
    fn simple_single_surface_scaffolds_do_not_require_task_plan() {
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

        assert_eq!(data.recommended_tool, "dispatch_ready_work");
        assert_eq!(data.items[0].planning.required_mode, "direct");
        assert_eq!(data.items[0].plan.status, "not_required");
        assert!(data.items[0].plan.errors.is_empty());
        assert!(data.items[0].ready_to_dispatch);
    }

    #[test]
    fn simple_dual_surface_foundation_scaffold_stays_direct() {
        let project = backlog_project();
        init_git(project.path());
        write_item_with(
            project.path(),
            ItemFixture {
                id: "PROJ-001",
                title: "Create simple backend and frontend starter scaffolds",
                item_type: "foundation",
                area: "tooling",
                owned_surfaces: &["backend/", "frontend/"],
            },
        );
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

        assert_eq!(data.recommended_tool, "dispatch_ready_work");
        assert_eq!(data.items[0].planning.required_mode, "direct");
        assert_eq!(data.items[0].plan.status, "not_required");
        assert!(data.items[0].ready_to_dispatch);
    }

    #[test]
    fn classifies_full_planning_for_high_risk_surfaces() {
        let project = backlog_project();
        write_item_with(
            project.path(),
            ItemFixture {
                id: "PROJ-001",
                title: "Change approval security",
                item_type: "safety",
                area: "approvals",
                owned_surfaces: &["src/approvals.rs"],
            },
        );

        let result = classify_planning_needs(
            project.path(),
            ClassifyPlanningNeedsParams {
                root: None,
                item_id: Some("PROJ-001".to_string()),
                limit: Some(10),
            },
        );
        let data = result.data.expect("classification");

        assert_eq!(data.returned, 1);
        assert_eq!(data.classifications[0].required_mode, "full");
        assert_eq!(
            data.classifications[0].required_artifact.as_deref(),
            Some("backlog/plans/PROJ-001.yaml")
        );
    }

    #[test]
    fn classifies_workflow_fit_for_greenfield_scaffold() {
        let project = TempDir::new().expect("temp dir");

        let result = classify_workflow_fit(
            project.path(),
            ClassifyWorkflowFitParams {
                root: None,
                goal: "Create a simple FastAPI React web app scaffold".to_string(),
                owned_surfaces: Vec::new(),
            },
        );
        let data = result.data.expect("workflow fit");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.recommended_mode, "direct_scaffold");
        assert!(data.next_action.contains("No Platypus scaffold tool"));
        assert!(data.next_action.contains("host's native file edits"));
    }

    #[test]
    fn classifies_platypus_initialized_scaffold_as_direct_scaffold() {
        let project = TempDir::new().expect("temp dir");
        crate::project::init_project(
            project.path(),
            crate::models::InitProjectParams {
                root: None,
                project_name: Some("Scaffold".to_string()),
                overwrite: None,
            },
        );

        let result = classify_workflow_fit(
            project.path(),
            ClassifyWorkflowFitParams {
                root: None,
                goal: "Build a simple FastAPI + React web app with authentication and a dashboard"
                    .to_string(),
                owned_surfaces: vec!["backend/".to_string(), "frontend/".to_string()],
            },
        );
        let data = result.data.expect("workflow fit");

        assert_eq!(data.recommended_mode, "direct_scaffold");
    }

    #[test]
    fn classifies_workflow_fit_for_existing_traceable_work() {
        let project = backlog_project();
        write_item(project.path(), "PROJ-001", "First item");

        let result = classify_workflow_fit(
            project.path(),
            ClassifyWorkflowFitParams {
                root: None,
                goal: "Implement the next roadmap item with evidence and CI validation".to_string(),
                owned_surfaces: vec!["src/server.rs".to_string(), "src/guidance.rs".to_string()],
            },
        );
        let data = result.data.expect("workflow fit");

        assert_eq!(data.recommended_mode, "platypus_workflow");
        assert_eq!(data.backlog_items, 1);
        assert!(data.next_action.contains("dispatch through worktrees"));
    }

    #[test]
    fn classifies_workflow_fit_high_risk_surface_as_platypus_workflow() {
        let project = TempDir::new().expect("temp dir");

        let result = classify_workflow_fit(
            project.path(),
            ClassifyWorkflowFitParams {
                root: None,
                goal: "Scaffold approval handling".to_string(),
                owned_surfaces: vec!["src/approvals.rs".to_string()],
            },
        );
        let data = result.data.expect("workflow fit");

        assert_eq!(data.recommended_mode, "platypus_workflow");
        assert!(data
            .reasons
            .iter()
            .any(|reason| reason.contains("high-risk")));
    }

    #[test]
    fn classifies_workflow_fit_for_hybrid_scaffold_in_existing_project() {
        let project = TempDir::new().expect("temp dir");
        fs::write(project.path().join("README.md"), "# Existing\n").expect("readme");
        fs::write(project.path().join("package.json"), "{}\n").expect("package");
        fs::write(project.path().join("src.txt"), "src\n").expect("src");

        let result = classify_workflow_fit(
            project.path(),
            ClassifyWorkflowFitParams {
                root: None,
                goal: "Add a Vite starter UI".to_string(),
                owned_surfaces: Vec::new(),
            },
        );
        let data = result.data.expect("workflow fit");

        assert_eq!(data.recommended_mode, "hybrid");
        assert!(data.next_action.contains("No Platypus scaffold tool"));
        assert!(data.next_action.contains("commit"));
    }

    #[test]
    fn inspects_work_queue_and_recommends_dispatch_with_valid_plan() {
        let project = backlog_project();
        init_git(project.path());
        write_item(project.path(), "PROJ-001", "First item");
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
    suggested_worker: coder
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
        assert_eq!(data.items[0].plan.task_count, 1);
    }

    #[test]
    fn inspect_work_queue_hides_items_with_active_tasks() {
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
        assert_eq!(data.items.len(), 0);
        assert_eq!(data.active_count, 1);
        assert_eq!(data.active_item_ids, vec!["PROJ-001"]);
        assert_eq!(data.recommended_tool, "next_safe_action");
        assert!(data.summary.contains("already have active tasks"));
        assert!(data
            .preflight_warnings
            .iter()
            .any(|warning| warning.contains("already have active tasks")));
    }

    #[test]
    fn inspect_work_queue_applies_limit_after_active_task_filtering() {
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
        assert_eq!(data.items[0].candidate.item_id, "PROJ-011");
        assert_eq!(data.active_count, 10);
        assert_eq!(data.recommended_tool, "dispatch_ready_work");
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
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].candidate.item_id, "PROJ-101");
        assert_eq!(data.active_count, 100);
        assert_eq!(data.recommended_tool, "dispatch_ready_work");
    }

    fn backlog_project() -> TempDir {
        let project = TempDir::new().expect("temp dir");
        write_ready_profiles(project.path());
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

    fn write_ready_profiles(root: &Path) {
        fs::write(
            root.join("platy.yaml"),
            r#"agents:
  profiles:
    manager:
      role: manager
      harness: codex
      executable: git
    coder:
      role: worker
      harness: codex
      executable: git
"#,
        )
        .expect("config");
    }

    fn init_git_with_closed_item(root: &Path, item_id: &str) {
        git(root, &["init"]);
        git(root, &["config", "user.name", "Platypus Test"]);
        git(root, &["config", "user.email", "platypus@example.invalid"]);
        fs::write(root.join("README.md"), "# Test\n").expect("readme");
        git(root, &["add", "--all"]);
        git(
            root,
            &[
                "commit",
                "-m",
                &format!("Close item\n\nPlatypus-Closes: {item_id}\nPlatypus-Verification: test"),
            ],
        );
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

    struct ItemFixture<'a> {
        id: &'a str,
        title: &'a str,
        item_type: &'a str,
        area: &'a str,
        owned_surfaces: &'a [&'a str],
    }

    fn write_item_with(root: &Path, fixture: ItemFixture<'_>) {
        let surfaces = fixture
            .owned_surfaces
            .iter()
            .map(|surface| format!("- {surface}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            root.join("backlog/items")
                .join(format!("{}.md", fixture.id)),
            format!(
                r#"---
id: {id}
title: {title}
priority: P1
type: {item_type}
area: {area}
epic: general
depends_on: []
suggested_worker: coder
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
                id = fixture.id,
                title = fixture.title,
                item_type = fixture.item_type,
                area = fixture.area,
                surfaces = surfaces
            ),
        )
        .expect("item");
    }
}
