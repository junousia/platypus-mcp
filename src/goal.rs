use crate::{
    backlog, config, dispatch, guidance,
    models::{
        ActionResult, ActionStatus, CreateBacklogItemParams, CreatedBacklogItemData,
        DispatchReadyWorkParams, GoalWorkBlockedItem, InitProjectParams, PlanGoalWorkData,
        PlanGoalWorkParams, StartGoalWorkData, StartGoalWorkParams,
    },
    project,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const ACTION: &str = "start_goal_work";
const PLAN_ACTION: &str = "plan_goal_work";

pub fn plan_goal_work(
    default_root: &Path,
    params: PlanGoalWorkParams,
) -> ActionResult<PlanGoalWorkData> {
    let goal = params.goal.trim().to_string();
    if goal.is_empty() {
        return ActionResult::failed(PLAN_ACTION, "Could not plan goal work.", "goal is required");
    }

    let fit = guidance::classify_workflow_fit(
        default_root,
        crate::models::ClassifyWorkflowFitParams {
            root: params.root.clone(),
            goal: goal.clone(),
            owned_surfaces: params.owned_surfaces.clone(),
        },
    );
    let fit = match fit {
        ActionResult {
            status: ActionStatus::Completed,
            data: Some(data),
            ..
        } => data,
        ActionResult { summary, error, .. } => {
            return ActionResult::failed(
                PLAN_ACTION,
                "Could not classify goal workflow.",
                error.unwrap_or(summary),
            )
        }
    };

    let recommended_mode =
        match requested_or_inferred_mode(params.mode.as_deref(), &fit.recommended_mode) {
            Ok(mode) => mode,
            Err(error) => {
                return ActionResult::failed(PLAN_ACTION, "Could not plan goal work.", error);
            }
        };

    let tracking_recommended = true;
    let dispatch = true;
    let mut recommended_arguments = BTreeMap::new();
    recommended_arguments.insert(
        "root".to_string(),
        serde_json::Value::String(fit.root.clone()),
    );
    recommended_arguments.insert("goal".to_string(), serde_json::Value::String(goal.clone()));
    recommended_arguments.insert(
        "mode".to_string(),
        serde_json::Value::String(recommended_mode.clone()),
    );
    recommended_arguments.insert("dispatch".to_string(), serde_json::Value::Bool(dispatch));
    if !params.owned_surfaces.is_empty() {
        recommended_arguments.insert(
            "owned_surfaces".to_string(),
            serde_json::Value::Array(
                params
                    .owned_surfaces
                    .iter()
                    .map(|surface| serde_json::Value::String(surface.clone()))
                    .collect(),
            ),
        );
    }

    let recommended_call = serde_json::to_string(&recommended_arguments).unwrap_or_else(|_| {
        "the structured recommended_arguments returned in this response".to_string()
    });
    let next_action = match recommended_mode.as_str() {
        "direct_scaffold" => {
            format!(
                "Call start_goal_work with {recommended_call} to create a tracking anchor and prepare the first task worktree. Use dispatch=false only when you intentionally want to edit the manager workspace directly."
            )
        }
        "hybrid" => {
            format!(
                "Call start_goal_work with {recommended_call} to create a tracking anchor and prepare the first task worktree. Create follow-up backlog items after the baseline is committed."
            )
        }
        "platypus_workflow" => {
            format!(
                "Call start_goal_work with {recommended_call} to create tracked work and prepare the first worker task."
            )
        }
        _ => "Call start_goal_work after selecting an explicit workflow mode.".to_string(),
    };

    ActionResult::completed(
        PLAN_ACTION,
        format!("{recommended_mode} is recommended; no project state was changed."),
        PlanGoalWorkData {
            root: fit.root,
            recommended_mode,
            summary: fit.summary,
            reasons: fit.reasons,
            tracking_recommended,
            recommended_tool: "start_goal_work".to_string(),
            recommended_arguments,
            next_action,
            backlog_items: fit.backlog_items,
            runnable_backlog_items: fit.runnable_backlog_items,
        },
    )
}

pub fn start_goal_work(
    default_root: &Path,
    params: StartGoalWorkParams,
) -> ActionResult<StartGoalWorkData> {
    let goal = params.goal.trim().to_string();
    if goal.is_empty() {
        return ActionResult::failed(ACTION, "Could not start goal work.", "goal is required");
    }
    if params.scaffold_in_place.is_some() {
        return ActionResult {
            action: ACTION.to_string(),
            status: ActionStatus::Failed,
            summary: "scaffold_in_place was removed from start_goal_work.".to_string(),
            next_action: Some(
                "Use plan_goal_work for read-only guidance, or call start_goal_work with dispatch=true for tracked worktree execution."
                    .to_string(),
            ),
            data: None,
            error: Some(
                "scaffold_in_place is no longer supported; plan first with plan_goal_work or start tracked work with dispatch=true."
                    .to_string(),
            ),
        };
    }
    let mut bootstrap_warnings = Vec::new();
    if let Some(summary) = ensure_project_scaffold(default_root, params.root.clone()) {
        bootstrap_warnings.push(summary);
    }

    let fit = guidance::classify_workflow_fit(
        default_root,
        crate::models::ClassifyWorkflowFitParams {
            root: params.root.clone(),
            goal: goal.clone(),
            owned_surfaces: params.owned_surfaces.clone(),
        },
    );
    let fit = match fit {
        ActionResult {
            status: ActionStatus::Completed,
            data: Some(data),
            ..
        } => data,
        ActionResult { summary, error, .. } => {
            return ActionResult::failed(
                ACTION,
                "Could not classify goal workflow.",
                error.unwrap_or(summary),
            )
        }
    };

    let recommended_mode =
        match requested_or_inferred_mode(params.mode.as_deref(), &fit.recommended_mode) {
            Ok(mode) => mode,
            Err(error) => {
                return ActionResult::failed(ACTION, "Could not start goal work.", error);
            }
        };

    let dispatch_requested = params.dispatch.unwrap_or(false);
    let effective_dispatch_requested = dispatch_requested;
    let mut next_action = mode_next_action(&recommended_mode);
    let mut warnings = fit.reasons;
    warnings.extend(bootstrap_warnings);
    if recommended_mode == "direct_scaffold" {
        warnings.retain(|warning| {
            let normalized = warning.to_ascii_lowercase();
            !(normalized.contains("structured")
                || normalized.contains("multi-step")
                || normalized.contains("traceable"))
        });
    }
    let tracking_item_type = if recommended_mode == "direct_scaffold" {
        "foundation"
    } else {
        "feature"
    };
    let mut created_items = Vec::new();
    let mut reused_item_id: Option<String> = None;
    let track_by_default = dispatch_requested || recommended_mode == "direct_scaffold";
    let also_track_enabled = params.also_track.unwrap_or(track_by_default) || dispatch_requested;
    if also_track_enabled {
        if let Some(existing_id) =
            find_existing_tracking_item(default_root, params.root.as_deref(), &goal)
        {
            reused_item_id = Some(existing_id.clone());
            warnings.push(format!(
                "Tracking item `{existing_id}` already matches this goal; no new backlog item was created."
            ));
            next_action =
                "Reused existing backlog tracking item. Scaffold the baseline now.".to_string();
            if !dispatch_requested {
                next_action.push_str(
                    " For tracked execution, commit backlog artifacts or call dispatch_ready_work with auto_commit_artifacts=true.",
                );
            }
        } else {
            match create_tracking_item(default_root, &params, &goal, tracking_item_type) {
                Ok(item) => {
                    created_items.push(item);
                    next_action = "Created a backlog tracking item.".to_string();
                    if !dispatch_requested {
                        next_action.push_str(
                            " For tracked execution, dispatch first (commit backlog artifacts or call dispatch_ready_work with auto_commit_artifacts=true), then scaffold in the assigned task worktree.",
                        );
                    }
                    if !params.verification_command.is_empty() {
                        next_action.push_str(&format!(
                            " Verification command is set to: `{}`.",
                            params.verification_command.join(" ")
                        ));
                    } else {
                        if effective_dispatch_requested || recommended_mode != "direct_scaffold" {
                            warnings.push(
                            "No verification_command was provided. Dispatch can proceed, but run_task_verification will skip; pass verification_command when this work has a known check."
                                .to_string(),
                        );
                        }
                    }
                    if effective_dispatch_requested
                        && tracking_item_type == "foundation"
                        && params.owned_surfaces.len() > 1
                    {
                        warnings.push(
                            "This foundation item spans multiple surfaces. Consider write_task_plan to clarify execution slices before dispatch."
                                .to_string(),
                        );
                    }
                }
                Err(error) => {
                    return ActionResult::failed(
                        ACTION,
                        "Could not create goal tracking item.",
                        error,
                    )
                }
            }
        }
    } else if matches!(recommended_mode.as_str(), "direct_scaffold" | "hybrid") {
        warnings.push(
            "No tracking item was created. Set also_track=true if you want a backlog anchor now."
                .to_string(),
        );
        next_action = format!(
            "{next_action} This call was advisory only. Re-run start_goal_work with also_track=true{} when you want Platypus task tracking.",
            if dispatch_requested {
                ", dispatch=true"
            } else {
                " and optionally dispatch=true"
            }
        );
    } else if also_track_enabled && !dispatch_requested {
        next_action = format!(
            "{next_action} Commit backlog/items/*.md, then run dispatch_ready_work before creating baseline files so work starts inside the task worktree."
        );
    }
    let mut dispatched_tasks = Vec::new();
    let mut dispatched_assignment_ids = Vec::new();
    let mut blocked_items = Vec::new();
    if effective_dispatch_requested {
        if let Some(worker_warning) =
            dispatch_agent_profile_warning(default_root, params.root.as_deref(), &fit.root)
        {
            warnings.push(worker_warning);
        }
    }
    if effective_dispatch_requested {
        let auto_commit_artifacts = params.auto_commit_artifacts.or_else(|| {
            if !created_items.is_empty() {
                Some(true)
            } else {
                None
            }
        });
        if auto_commit_artifacts == Some(true) && params.auto_commit_artifacts.is_none() {
            warnings.push(
                "Dispatch auto-commit was enabled for this call because start_goal_work created new tracking artifacts and dispatch=true was requested."
                    .to_string(),
            );
        }
        let dispatch_result = dispatch::dispatch_ready_work(
            default_root,
            DispatchReadyWorkParams {
                root: params.root.clone(),
                max_tasks: params.max_tasks.or(Some(1)),
                worker: params.suggested_worker.clone(),
                claimant: Some("start_goal_work".to_string()),
                prepare_handoffs: params.prepare_handoffs.or(Some(true)),
                auto_start: params.auto_start.or(Some(false)),
                auto_commit_artifacts,
                dry_run: Some(false),
                verification_command: params.verification_command.clone(),
            },
        );
        match dispatch_result {
            ActionResult {
                status: ActionStatus::Completed | ActionStatus::Skipped,
                summary,
                next_action: dispatch_next,
                data: Some(data),
                ..
            } => {
                dispatched_tasks.extend(data.items.clone());
                dispatched_assignment_ids.extend(data.items.iter().filter_map(|item| {
                    item.assignment
                        .as_ref()
                        .map(|assignment| assignment.id.clone())
                }));
                blocked_items.extend(data.items.iter().filter_map(|item| {
                    if matches!(item.status.as_str(), "failed" | "skipped") {
                        Some(GoalWorkBlockedItem {
                            item_id: item.item_id.clone(),
                            title: item.title.clone(),
                            recommended_tool: "inspect_work_queue".to_string(),
                            reason: item.reason.clone(),
                        })
                    } else {
                        None
                    }
                }));
                if dispatched_tasks
                    .iter()
                    .all(|item| !matches!(item.status.as_str(), "queued" | "prepared" | "started"))
                {
                    warnings.push(summary);
                } else {
                    next_action = dispatch_next.unwrap_or_else(|| {
                        "Track assignment lifecycle with start_worker_task and complete_worker_task."
                            .to_string()
                    });
                    if let Some(first_assignment) = dispatched_assignment_ids.first() {
                        next_action =
                            format!("{next_action} Start with assignment_id `{first_assignment}`.");
                    }
                }
            }
            ActionResult { summary, error, .. } => {
                return ActionResult::failed(
                    ACTION,
                    "Could not dispatch goal work.",
                    error.unwrap_or(summary),
                )
            }
        }
    }
    let data = StartGoalWorkData {
        root: fit.root,
        recommended_mode: recommended_mode.clone(),
        summary: fit.summary,
        created_item_id: created_items
            .first()
            .map(|item| item.item_id.clone())
            .or(reused_item_id),
        created_items,
        created_plans: Vec::new(),
        dispatched_tasks,
        dispatched_assignment_ids,
        blocked_items,
        warnings,
        next_action: next_action.clone(),
    };
    ActionResult {
        action: ACTION.to_string(),
        status: ActionStatus::Completed,
        summary: if effective_dispatch_requested {
            format!(
                "{recommended_mode} workflow started; {} task(s) were dispatched.",
                data.dispatched_tasks.len()
            )
        } else {
            format!("{recommended_mode} is recommended before Platypus task dispatch.")
        },
        next_action: Some(next_action),
        data: Some(data),
        error: None,
    }
}

fn requested_or_inferred_mode(
    requested_mode: Option<&str>,
    inferred_mode: &str,
) -> Result<String, String> {
    let requested_mode = requested_mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("auto")
        .to_ascii_lowercase();
    match requested_mode.as_str() {
        "auto" => Ok(inferred_mode.to_string()),
        "direct_scaffold" | "hybrid" | "platypus_workflow" => Ok(requested_mode),
        _ => Err("mode must be auto, direct_scaffold, hybrid, or platypus_workflow".to_string()),
    }
}

fn dispatch_agent_profile_warning(
    default_root: &Path,
    root_arg: Option<&str>,
    resolved_root: &str,
) -> Option<String> {
    let root = root_arg
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| resolved_root.to_string());
    let profiles = config::list_agent_profiles(
        default_root,
        crate::models::AgentProfilesParams { root: Some(root) },
    )
    .data?;
    if profiles.profiles.is_empty() {
        return Some(
            "No managed worker profiles are configured. Dispatch can still prepare worktrees for MCP-host or external-worker execution; background managed workers will remain idle until profiles are configured."
                .to_string(),
        );
    }
    let has_ready_worker = profiles
        .profiles
        .iter()
        .any(|profile| profile.role == "worker" && profile.ready);
    if !has_ready_worker {
        return Some(
            "No ready managed worker profile is configured. Dispatch may proceed for MCP-host or external-worker execution; background managed workers will remain idle until a ready worker profile exists."
                .to_string(),
        );
    }
    None
}

fn ensure_project_scaffold(default_root: &Path, root: Option<String>) -> Option<String> {
    let target_root = root
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_root.to_path_buf());
    if target_root.join("platy.yaml").is_file() && target_root.join("backlog").is_dir() {
        return None;
    }
    let result = project::init_project(
        default_root,
        InitProjectParams {
            root,
            project_name: None,
            overwrite: Some(false),
        },
    );
    if !matches!(
        result.status,
        ActionStatus::Completed | ActionStatus::Skipped
    ) {
        return Some("Project scaffold initialization failed during start_goal_work. Run init_project and retry.".to_string());
    }
    Some("Project scaffold was initialized automatically (platy.yaml, backlog/, and workflow files).".to_string())
}

fn create_tracking_item(
    default_root: &Path,
    params: &StartGoalWorkParams,
    goal: &str,
    tracking_item_type: &str,
) -> Result<CreatedBacklogItemData, String> {
    let owned_surfaces = inferred_owned_surfaces(goal, &params.owned_surfaces);
    let implementation_contract =
        generated_implementation_contract(goal, &owned_surfaces, &params.verification_command);
    let acceptance = generated_acceptance(goal, &owned_surfaces, &params.verification_command);
    let result = backlog::create_backlog_item(
        default_root,
        CreateBacklogItemParams {
            root: params.root.clone(),
            id: None,
            id_prefix: None,
            title: goal_title(goal),
            priority: Some("P1".to_string()),
            item_type: Some(tracking_item_type.to_string()),
            area: Some("general".to_string()),
            epic: Some("general".to_string()),
            depends_on: Vec::new(),
            suggested_worker: params
                .suggested_worker
                .clone()
                .or_else(|| Some("coder".to_string())),
            owned_surfaces,
            external_refs: Vec::new(),
            goal: goal.to_string(),
            implementation_contract: Some(implementation_contract),
            contract: None,
            acceptance,
            notes: Some(
                "Created by start_goal_work with also_track=true. Review before dispatch."
                    .to_string(),
            ),
        },
    );
    match result {
        ActionResult {
            status: ActionStatus::Completed,
            data: Some(data),
            ..
        } => Ok(data),
        ActionResult { summary, error, .. } => Err(error.unwrap_or(summary)),
    }
}

fn find_existing_tracking_item(
    default_root: &Path,
    root: Option<&str>,
    goal: &str,
) -> Option<String> {
    let expected_title = goal_title(goal).to_ascii_lowercase();
    let inventory = backlog::inspect_backlog_inventory(default_root, root, Some(200));
    let data = inventory.data?;
    data.items
        .into_iter()
        .find(|item| !item.closed && item.title.trim().eq_ignore_ascii_case(&expected_title))
        .map(|item| item.item_id)
}

fn inferred_owned_surfaces(goal: &str, provided: &[String]) -> Vec<String> {
    if !provided.is_empty() {
        return provided.to_vec();
    }
    let goal_lc = goal.to_ascii_lowercase();
    let mut surfaces = Vec::new();
    if goal_lc.contains("fastapi")
        || goal_lc.contains("backend")
        || goal_lc.contains("api")
        || goal_lc.contains("server")
    {
        surfaces.push("backend/".to_string());
    }
    if goal_lc.contains("react")
        || goal_lc.contains("frontend")
        || goal_lc.contains("dashboard")
        || goal_lc.contains("ui")
    {
        surfaces.push("frontend/".to_string());
    }
    surfaces
}

fn generated_implementation_contract(
    goal: &str,
    owned_surfaces: &[String],
    verification_command: &[String],
) -> String {
    let mut lines = vec![
        format!("Deliver a tracked baseline for this goal: {goal}"),
        "Keep scope to a first vertical slice that can be extended in follow-up tasks.".to_string(),
    ];
    if !owned_surfaces.is_empty() {
        lines.push(format!(
            "Confine initial edits to these surfaces: {}.",
            owned_surfaces.join(", ")
        ));
    }
    if !verification_command.is_empty() {
        lines.push(format!(
            "Record or run verification with: {}.",
            verification_command.join(" ")
        ));
    }
    lines
        .into_iter()
        .map(|line| format!("- {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn generated_acceptance(
    goal: &str,
    owned_surfaces: &[String],
    verification_command: &[String],
) -> Vec<String> {
    let mut acceptance = vec![format!(
        "A tracked baseline for `{goal}` exists and can be continued with dispatched Platypus tasks."
    )];
    if !owned_surfaces.is_empty() {
        acceptance.push(format!(
            "Changed files stay within declared owned surfaces: {}.",
            owned_surfaces.join(", ")
        ));
    }
    if !verification_command.is_empty() {
        acceptance.push(format!(
            "Verification command(s) are recorded for follow-up execution: {}.",
            verification_command.join(" ")
        ));
    }
    acceptance
}

fn goal_title(goal: &str) -> String {
    const MAX_CHARS: usize = 80;
    let normalized = goal.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut title = if normalized.chars().count() <= MAX_CHARS {
        normalized
    } else {
        let mut chars = normalized.chars();
        let mut value = chars.by_ref().take(MAX_CHARS).collect::<String>();
        if chars.next().is_some() {
            if let Some(last_space) = value.rfind(' ') {
                value.truncate(last_space);
            }
        }
        value
    };
    while trailing_stop_word(&title) && title.split_whitespace().count() > 4 {
        if let Some(index) = title.rfind(' ') {
            title.truncate(index);
        } else {
            break;
        }
    }
    title
        .trim_matches(|character: char| matches!(character, ',' | ';' | ':' | '-'))
        .trim()
        .to_string()
}

fn trailing_stop_word(value: &str) -> bool {
    value.split_whitespace().last().is_some_and(|word| {
        matches!(
            word.to_ascii_lowercase().as_str(),
            "a" | "an" | "the" | "and" | "or" | "to" | "for" | "with" | "of" | "in"
        )
    })
}

fn mode_next_action(mode: &str) -> String {
    match mode {
        "direct_scaffold" => {
            "Scaffold directly for the first slice. Keep edits in owned_surfaces.".to_string()
        }
        "hybrid" => {
            "Scaffold a baseline first, then track remaining work through backlog and dispatch."
                .to_string()
        }
        "platypus_workflow" => {
            "Create concrete backlog items, validate, then dispatch tracked work.".to_string()
        }
        _ => "Inspect workflow guidance before dispatching work.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InitProjectParams;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn simple_greenfield_goal_recommends_direct_scaffold_with_tracking_anchor() {
        let project = TempDir::new().expect("temp dir");

        let result = start_goal_work(
            project.path(),
            StartGoalWorkParams {
                root: None,
                goal: "create a simple web app".to_string(),
                mode: None,
                dispatch: None,
                prepare_handoffs: None,
                auto_start: None,
                auto_commit_artifacts: None,
                also_track: None,
                scaffold_in_place: None,
                max_tasks: None,
                suggested_worker: None,
                owned_surfaces: Vec::new(),
                verification_command: Vec::new(),
            },
        );

        assert_eq!(result.status, ActionStatus::Completed);
        let data = result.data.expect("goal data");
        assert_eq!(data.recommended_mode, "direct_scaffold");
        assert_eq!(data.created_items.len(), 1);
        assert!(data.created_plans.is_empty());
        assert!(data.dispatched_tasks.is_empty());
        assert!(data.next_action.to_ascii_lowercase().contains("scaffold"));
    }

    #[test]
    fn fastapi_react_starter_creates_tracking_anchor_by_default() {
        let project = initialized_git_project();

        let result = start_goal_work(
            project.path(),
            StartGoalWorkParams {
                root: None,
                goal: "Build a simple FastAPI + React web app with auth and a dashboard"
                    .to_string(),
                mode: None,
                dispatch: None,
                prepare_handoffs: None,
                auto_start: None,
                auto_commit_artifacts: None,
                also_track: None,
                scaffold_in_place: None,
                max_tasks: None,
                suggested_worker: None,
                owned_surfaces: vec!["backend/".to_string(), "frontend/".to_string()],
                verification_command: Vec::new(),
            },
        );

        assert_eq!(result.status, ActionStatus::Completed);
        let data = result.data.expect("goal data");
        assert_eq!(data.recommended_mode, "direct_scaffold");
        assert_eq!(data.created_items.len(), 1);
        assert_eq!(data.created_item_id.as_deref(), Some("PROJ-001"));
    }

    #[test]
    fn direct_scaffold_can_still_create_tracking_item_when_requested() {
        let project = initialized_git_project();

        let result = start_goal_work(
            project.path(),
            StartGoalWorkParams {
                root: None,
                goal: "Build a simple FastAPI + React web app with auth".to_string(),
                mode: None,
                dispatch: None,
                prepare_handoffs: None,
                auto_start: None,
                auto_commit_artifacts: None,
                also_track: Some(true),
                scaffold_in_place: None,
                max_tasks: None,
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: vec!["backend/".to_string(), "frontend/".to_string()],
                verification_command: Vec::new(),
            },
        );

        assert_eq!(result.status, ActionStatus::Completed);
        let data = result.data.expect("goal data");
        assert_eq!(data.recommended_mode, "direct_scaffold");
        assert_eq!(data.created_items.len(), 1);
        assert_eq!(data.created_items[0].item_id, "PROJ-001");
        assert!(data
            .next_action
            .to_ascii_lowercase()
            .contains("dispatch first"));
        assert!(data.created_items[0].item_id.starts_with("PROJ-"));

        let created =
            std::fs::read_to_string(&data.created_items[0].path).expect("created item text");
        assert!(created.contains("type: foundation"));
        assert!(!created.contains("with authentication and a is implemented"));
        assert!(!data
            .warnings
            .iter()
            .any(|warning| warning.contains("No verification_command was provided")));
    }

    #[test]
    fn platypus_goal_recommends_manual_or_sampled_item_creation_without_artifacts() {
        let project = initialized_git_project();

        let result = start_goal_work(
            project.path(),
            StartGoalWorkParams {
                root: None,
                goal: "add traceable authentication workflow".to_string(),
                mode: Some("platypus_workflow".to_string()),
                dispatch: Some(true),
                prepare_handoffs: Some(true),
                auto_start: Some(false),
                auto_commit_artifacts: Some(true),
                also_track: None,
                scaffold_in_place: None,
                max_tasks: Some(1),
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: vec!["src/auth.rs".to_string()],
                verification_command: vec!["cargo test".to_string()],
            },
        );

        assert_eq!(result.status, ActionStatus::Completed);
        let data = result.data.expect("goal data");
        assert_eq!(data.recommended_mode, "platypus_workflow");
        assert_eq!(data.created_items.len(), 1);
        assert!(data.created_plans.is_empty());
        assert_eq!(data.dispatched_tasks.len(), 1);
        assert!(data.next_action.contains("start_worker_task"));
    }

    #[test]
    fn platypus_goal_does_not_synthesize_fastapi_react_backlog_items() {
        let project = initialized_git_project();

        let result = start_goal_work(
            project.path(),
            StartGoalWorkParams {
                root: None,
                goal: "Build a simple FastAPI + React web app with auth and a dashboard"
                    .to_string(),
                mode: Some("platypus_workflow".to_string()),
                dispatch: Some(true),
                prepare_handoffs: Some(true),
                auto_start: Some(false),
                auto_commit_artifacts: Some(true),
                also_track: None,
                scaffold_in_place: None,
                max_tasks: Some(2),
                suggested_worker: None,
                owned_surfaces: Vec::new(),
                verification_command: vec!["make check".to_string()],
            },
        );

        assert_eq!(result.status, ActionStatus::Completed);
        let data = result.data.expect("goal data");
        assert_eq!(data.recommended_mode, "platypus_workflow");
        assert_eq!(data.created_items.len(), 1);
        assert!(data.created_plans.is_empty());
        assert_eq!(data.dispatched_tasks.len(), 1);
        assert!(data.next_action.contains("start_worker_task"));
    }

    #[test]
    fn plan_goal_work_returns_read_only_next_tool_guidance() {
        let project = TempDir::new().expect("temp dir");
        let result = plan_goal_work(
            project.path(),
            PlanGoalWorkParams {
                root: None,
                goal: "create a simple web app".to_string(),
                mode: None,
                owned_surfaces: vec!["backend/".to_string(), "frontend/".to_string()],
            },
        );
        assert_eq!(result.status, ActionStatus::Completed);
        let data = result.data.expect("goal data");
        assert_eq!(data.recommended_tool, "start_goal_work");
        assert!(data.tracking_recommended);
        assert_eq!(
            data.recommended_arguments
                .get("dispatch")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert!(data.next_action.contains("start_goal_work"));
        assert!(data.next_action.contains("\"dispatch\":true"));
        assert!(!project.path().join("platy.yaml").exists());
    }

    #[test]
    fn plan_goal_work_can_recommend_dispatch_for_tracked_workflow() {
        let project = TempDir::new().expect("temp dir");
        let result = plan_goal_work(
            project.path(),
            PlanGoalWorkParams {
                root: None,
                goal: "add production authentication workflow with audit trail".to_string(),
                mode: Some("platypus_workflow".to_string()),
                owned_surfaces: vec!["src/auth.rs".to_string()],
            },
        );
        assert_eq!(result.status, ActionStatus::Completed);
        let data = result.data.expect("goal data");
        assert_eq!(data.recommended_mode, "platypus_workflow");
        assert_eq!(
            data.recommended_arguments
                .get("dispatch")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert!(data.next_action.contains("start_goal_work"));
        assert!(data.next_action.contains("\"dispatch\":true"));
        assert!(!project.path().join("platy.yaml").exists());
    }

    #[test]
    fn start_goal_work_rejects_legacy_scaffold_in_place_before_mutation() {
        let project = TempDir::new().expect("temp dir");
        let result = start_goal_work(
            project.path(),
            StartGoalWorkParams {
                root: None,
                goal: "create a simple web app".to_string(),
                mode: None,
                dispatch: Some(false),
                prepare_handoffs: None,
                auto_start: None,
                auto_commit_artifacts: None,
                also_track: None,
                scaffold_in_place: Some(true),
                max_tasks: None,
                suggested_worker: None,
                owned_surfaces: Vec::new(),
                verification_command: Vec::new(),
            },
        );
        assert_eq!(result.status, ActionStatus::Failed);
        assert!(result
            .error
            .as_deref()
            .expect("error")
            .contains("plan_goal_work"));
        assert!(!project.path().join("platy.yaml").exists());
    }

    #[test]
    fn repeated_goal_reuses_existing_tracking_item() {
        let project = initialized_git_project();
        let first = start_goal_work(
            project.path(),
            StartGoalWorkParams {
                root: None,
                goal: "Build a simple FastAPI + React web app with auth".to_string(),
                mode: None,
                dispatch: None,
                prepare_handoffs: None,
                auto_start: None,
                auto_commit_artifacts: None,
                also_track: Some(true),
                scaffold_in_place: None,
                max_tasks: None,
                suggested_worker: None,
                owned_surfaces: vec!["backend/".to_string(), "frontend/".to_string()],
                verification_command: Vec::new(),
            },
        );
        assert_eq!(first.status, ActionStatus::Completed);
        let second = start_goal_work(
            project.path(),
            StartGoalWorkParams {
                root: None,
                goal: "Build a simple FastAPI + React web app with auth".to_string(),
                mode: None,
                dispatch: None,
                prepare_handoffs: None,
                auto_start: None,
                auto_commit_artifacts: None,
                also_track: Some(true),
                scaffold_in_place: None,
                max_tasks: None,
                suggested_worker: None,
                owned_surfaces: vec!["backend/".to_string(), "frontend/".to_string()],
                verification_command: Vec::new(),
            },
        );
        assert_eq!(second.status, ActionStatus::Completed);
        let second_data = second.data.expect("goal data");
        assert!(second_data.created_items.is_empty());
        assert_eq!(second_data.created_item_id.as_deref(), Some("PROJ-001"));
        assert!(second_data
            .warnings
            .iter()
            .any(|warning| warning.contains("already matches this goal")));
    }

    fn initialized_git_project() -> TempDir {
        let project = TempDir::new().expect("temp dir");
        crate::project::init_project(
            project.path(),
            InitProjectParams {
                root: None,
                project_name: Some("Goal Test".to_string()),
                overwrite: None,
            },
        );
        git(project.path(), &["init"]);
        git(project.path(), &["config", "user.name", "Platypus Test"]);
        git(
            project.path(),
            &["config", "user.email", "platypus@example.invalid"],
        );
        git(project.path(), &["add", "."]);
        git(project.path(), &["commit", "-m", "Initial project"]);
        project
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
