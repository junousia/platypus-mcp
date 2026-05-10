use crate::{
    assignments, backlog, config,
    git_readiness::inspect_git_readiness,
    models::{
        ActionResult, ActionStatus, AgentProfilesParams, BacklogCandidate, DispatchNextWorkData,
        DispatchReadyWorkData, DispatchReadyWorkItem, DispatchReadyWorkParams,
        PrepareWorkerAssignmentParams, RootParams, TaskRecord,
    },
    state::{
        sqlite::SqliteProjectState, BacklogCandidateSnapshot, DispatchWorkCommand, ProjectState,
        ProjectStateError, TaskSnapshot,
    },
};
use std::{path::Path, process::Command};

pub fn dispatch_next_work(
    default_root: &Path,
    params: RootParams,
) -> ActionResult<DispatchNextWorkData> {
    let action = "dispatch_next_work";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    if let Some(blocked) = dispatch_readiness_result(action, state.root(), true) {
        return blocked;
    }
    match state.dispatch_work(DispatchWorkCommand {
        summary: None,
        preferred_worker: None,
        source_item_id: None,
    }) {
        Ok(outcome) => {
            let task = task_record(outcome.task);
            ActionResult {
                action: action.to_string(),
                status: ActionStatus::Completed,
                summary: format!("Dispatched {} as task {}.", task.source_item_id, task.id),
                next_action: Some(
                    "Use inspect_task_events and an external harness to execute the queued task."
                        .to_string(),
                ),
                data: Some(DispatchNextWorkData {
                    root: state.root().display().to_string(),
                    candidate: backlog_candidate(outcome.candidate),
                    task,
                }),
                error: None,
            }
        }
        Err(ProjectStateError::NotFound { .. }) => ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No runnable backlog items to dispatch.".to_string(),
            next_action: Some("Create or unblock backlog items.".to_string()),
            data: None,
            error: None,
        },
        Err(ProjectStateError::Conflict { message }) if message.contains("leased by") => {
            ActionResult::skipped(
                action,
                message,
                "Wait for the lease to expire or release it before dispatching work.",
            )
        }
        Err(error) => state_error(action, "Could not dispatch runnable backlog work.", error),
    }
}

pub fn dispatch_ready_work(
    default_root: &Path,
    params: DispatchReadyWorkParams,
) -> ActionResult<DispatchReadyWorkData> {
    let action = "dispatch_ready_work";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    let root = state.root().display().to_string();
    let requested = params.max_tasks.unwrap_or(10).clamp(1, 10);
    if params.dry_run.unwrap_or(false) {
        let listed = backlog::list_backlog(default_root, Some(root.as_str()), Some(requested));
        let candidates = match listed {
            ActionResult {
                status: ActionStatus::Completed | ActionStatus::Skipped,
                data: Some(data),
                ..
            } => data.candidates,
            ActionResult { summary, error, .. } => {
                return ActionResult::failed(
                    action,
                    "Could not preview dispatchable work.",
                    error.unwrap_or(summary),
                )
            }
        };
        let candidates = filter_candidates_for_dispatch(candidates, params.item_id.as_deref());
        let selected = candidates.into_iter().take(requested).collect::<Vec<_>>();
        let items = selected
            .iter()
            .map(|candidate| DispatchReadyWorkItem {
                item_id: candidate.item_id.clone(),
                title: candidate.title.clone(),
                status: "preview".to_string(),
                reason: "Dry run only; no task or assignment was created.".to_string(),
                task: None,
                assignment: None,
            })
            .collect::<Vec<_>>();
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Completed,
            summary: format!("Dry run selected {} dispatch candidate(s).", items.len()),
            next_action: Some(
                "Call dispatch_ready_work again with dry_run=false (or omit dry_run) to dispatch."
                    .to_string(),
            ),
            data: Some(DispatchReadyWorkData {
                root,
                requested,
                available_before_dispatch: selected.len(),
                selected: selected.len(),
                dispatched: 0,
                prepared: 0,
                started: 0,
                failed: 0,
                stopped_reason: if selected.is_empty() {
                    "no_runnable_items".to_string()
                } else {
                    "dry_run".to_string()
                },
                items,
            }),
            error: None,
        };
    }

    let auto_commit_artifacts = params.auto_commit_artifacts.unwrap_or_else(|| {
        config::effective_dispatch_defaults(state.root())
            .map(|value| value.auto_commit_artifacts_default)
            .unwrap_or(false)
    });
    if let Some(blocked) = dispatch_readiness_result(action, state.root(), auto_commit_artifacts) {
        return blocked;
    }

    let available_before_dispatch =
        backlog::list_backlog(default_root, params.root.as_deref(), Some(100))
            .data
            .map(|data| {
                filter_candidates_for_dispatch(data.candidates, params.item_id.as_deref()).len()
            })
            .unwrap_or(0);
    let requested = params
        .max_tasks
        .unwrap_or_else(|| available_before_dispatch.clamp(1, 10))
        .clamp(1, 10);
    let prepare_handoffs = params.prepare_handoffs.unwrap_or(true);
    let auto_start = params.auto_start.unwrap_or(false);
    let claimant = params
        .claimant
        .clone()
        .or_else(|| params.worker.clone())
        .unwrap_or_else(|| "external-worker".to_string());
    let mut report = DispatchReadyWorkData {
        root: root.clone(),
        requested,
        available_before_dispatch,
        selected: 0,
        dispatched: 0,
        prepared: 0,
        started: 0,
        failed: 0,
        stopped_reason: "max_tasks_reached".to_string(),
        items: Vec::new(),
    };

    for _ in 0..requested {
        let outcome = match state.dispatch_work(DispatchWorkCommand {
            summary: None,
            preferred_worker: params.worker.clone(),
            source_item_id: params.item_id.clone(),
        }) {
            Ok(outcome) => outcome,
            Err(ProjectStateError::NotFound { .. }) => {
                report.stopped_reason = "no_runnable_items".to_string();
                break;
            }
            Err(ProjectStateError::Conflict { message }) if message.contains("leased by") => {
                report.stopped_reason = "project_leased".to_string();
                report.items.push(DispatchReadyWorkItem {
                    item_id: String::new(),
                    title: "Project lease".to_string(),
                    status: "skipped".to_string(),
                    reason: message,
                    task: None,
                    assignment: None,
                });
                break;
            }
            Err(error) => {
                report.failed += 1;
                report.stopped_reason = "dispatch_failed".to_string();
                report.items.push(DispatchReadyWorkItem {
                    item_id: String::new(),
                    title: "Dispatch failed".to_string(),
                    status: "failed".to_string(),
                    reason: error.to_string(),
                    task: None,
                    assignment: None,
                });
                break;
            }
        };

        let candidate = backlog_candidate(outcome.candidate);
        let task = task_record(outcome.task);
        report.dispatched += 1;
        report.selected += 1;
        if !prepare_handoffs {
            report.items.push(DispatchReadyWorkItem {
                item_id: candidate.item_id,
                title: candidate.title,
                status: "queued".to_string(),
                reason: "Dispatched task without preparing a worker handoff.".to_string(),
                task: Some(task),
                assignment: None,
            });
            continue;
        }

        let prepared = assignments::prepare_worker_assignment(
            default_root,
            PrepareWorkerAssignmentParams {
                root: Some(root.clone()),
                task_id: Some(task.id.clone()),
                worker: task.worker.clone().or_else(|| params.worker.clone()),
                claimant: Some(claimant.clone()),
                base_ref: None,
                verification_command: params.verification_command.clone(),
            },
        );
        match prepared {
            ActionResult {
                status: ActionStatus::Completed | ActionStatus::Skipped,
                data: Some(data),
                summary,
                ..
            } => {
                report.prepared += 1;
                let mut status = "prepared".to_string();
                let mut reason = summary;
                let mut assignment = data.assignment.clone();
                if auto_start {
                    let start_result = assignments::start_worker_execution(
                        default_root,
                        crate::models::StartWorkerExecutionParams {
                            root: Some(root.clone()),
                            assignment_id: Some(assignment.id.clone()),
                            task_id: None,
                            worker_session: None,
                        },
                    );
                    if let ActionResult {
                        status: ActionStatus::Completed | ActionStatus::Skipped,
                        data: Some(started_data),
                        summary: started_summary,
                        ..
                    } = start_result
                    {
                        report.started += 1;
                        status = "started".to_string();
                        reason = format!("{reason} {started_summary}");
                        assignment = started_data.assignment;
                    } else {
                        let ActionResult { summary, error, .. } = start_result;
                        report.failed += 1;
                        report.items.push(DispatchReadyWorkItem {
                            item_id: candidate.item_id,
                            title: candidate.title,
                            status: "failed".to_string(),
                            reason: error.unwrap_or(summary),
                            task: Some(task),
                            assignment: Some(assignment),
                        });
                        continue;
                    }
                }
                report.items.push(DispatchReadyWorkItem {
                    item_id: candidate.item_id,
                    title: candidate.title,
                    status,
                    reason,
                    task: Some(task),
                    assignment: Some(assignment),
                });
            }
            ActionResult { summary, error, .. } => {
                report.failed += 1;
                report.items.push(DispatchReadyWorkItem {
                    item_id: candidate.item_id,
                    title: candidate.title,
                    status: "failed".to_string(),
                    reason: error.unwrap_or(summary),
                    task: Some(task),
                    assignment: None,
                });
            }
        }
    }

    let status = if prepare_handoffs && report.failed > 0 && report.prepared == 0 {
        ActionStatus::Failed
    } else if report.dispatched == 0 && report.failed == 0 {
        ActionStatus::Skipped
    } else {
        ActionStatus::Completed
    };
    if report.stopped_reason == "max_tasks_reached" && report.selected < report.requested {
        report.stopped_reason = "selection_exhausted".to_string();
    }
    let summary = if prepare_handoffs && report.failed > 0 && report.prepared == 0 {
        format!(
            "Dispatched {} task(s), but no worker handoff could be prepared.",
            report.dispatched
        )
    } else if report.started > 0 {
        format!(
            "Dispatched {} of {} available task(s), prepared {} handoff(s), and marked {} assignment(s) running.",
            report.dispatched, report.available_before_dispatch, report.prepared, report.started
        )
    } else if report.prepared > 0 {
        format!(
            "Dispatched {} of {} available task(s) and prepared {} worker handoff(s).",
            report.dispatched, report.available_before_dispatch, report.prepared
        )
    } else if report.dispatched > 0 {
        format!("Dispatched {} task(s).", report.dispatched)
    } else {
        "No runnable backlog items were dispatched.".to_string()
    };
    let profile_warning = if auto_start {
        dispatch_agent_profile_warning(default_root, &root)
    } else {
        None
    };
    let mut next_action = if report.started > 0 {
        "Assignments were marked running in local lifecycle state. Launch or continue external workers in the prepared worktrees, record progress events, then complete_worker_task."
            .to_string()
    } else if report.prepared > 0 {
        if report.stopped_reason == "max_tasks_reached"
            && report.available_before_dispatch > report.selected
        {
            format!(
                "Assign each prepared bundle/worktree to its worker. {} additional runnable item(s) were left by max_tasks; call dispatch_ready_work again or pass a larger max_tasks value.",
                report.available_before_dispatch.saturating_sub(report.selected)
            )
        } else {
            "Assign each prepared bundle/worktree to its worker, then use start_worker_task and complete_worker_task for lifecycle updates.".to_string()
        }
    } else {
        "Inspect the returned per-item reasons and create or unblock backlog items if needed."
            .to_string()
    };
    if let Some(warning) = profile_warning.as_deref() {
        next_action = format!("{next_action} {warning}");
    }
    next_action.push_str(
        " Planning rationale for each queued item is available via inspect_work_queue and classify_planning_needs.",
    );
    let summary = if let Some(warning) = profile_warning {
        format!("{summary} {warning}")
    } else {
        summary
    };
    ActionResult {
        action: action.to_string(),
        status,
        summary: summary.clone(),
        next_action: Some(next_action),
        data: Some(report),
        error: None,
    }
}

fn dispatch_agent_profile_warning(default_root: &Path, root: &str) -> Option<String> {
    let profiles = config::list_agent_profiles(
        default_root,
        AgentProfilesParams {
            root: Some(root.to_string()),
        },
    )
    .data?;
    if profiles.profiles.is_empty() {
        return Some(
            "No agent profiles are configured; prepared worktrees may remain idle until external workers are started."
                .to_string(),
        );
    }
    let has_ready_worker = profiles
        .profiles
        .iter()
        .any(|profile| profile.role == "worker" && profile.ready);
    if !has_ready_worker {
        return Some(
            "No ready worker profile is configured; dispatch prepared tasks but nothing can execute them yet."
                .to_string(),
        );
    }
    None
}

fn dispatch_readiness_result<T: schemars::JsonSchema + serde::Serialize>(
    action: &str,
    root: &Path,
    auto_commit_artifacts: bool,
) -> Option<ActionResult<T>> {
    if !auto_commit_artifacts {
        if let Some(paths) = manager_dirty_paths(root) {
            if !paths.is_empty() && planning_artifact_paths_only(&paths) {
                let listed = paths
                    .iter()
                    .take(5)
                    .map(|path| format!("`{path}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let suffix = if paths.len() > 5 {
                    format!(" and {} more", paths.len() - 5)
                } else {
                    String::new()
                };
                return Some(ActionResult::failed(
                    action,
                    "Manager workspace has uncommitted backlog artifacts.",
                    format!(
                        "Pending artifact paths: {listed}{suffix}. Set auto_commit_artifacts=true on dispatch_ready_work, or run `git add backlog/items/*.md backlog/plans/*.yaml && git commit -m \"Track planning artifacts\"` before dispatch."
                    ),
                ));
            }
        }
    }
    let mut readiness = inspect_git_readiness(root, true);
    if auto_commit_artifacts
        && matches!(
            readiness.status,
            crate::git_readiness::GitReadinessStatus::Dirty
        )
    {
        match auto_commit_planning_artifacts(root) {
            Ok(true) => {
                readiness = inspect_git_readiness(root, true);
            }
            Ok(false) => {}
            Err(error) => {
                return Some(ActionResult::failed(
                    action,
                    "Could not auto-commit planning artifacts.",
                    error,
                ));
            }
        }
    }
    if readiness.ready() {
        return None;
    }
    Some(ActionResult {
        action: action.to_string(),
        status: ActionStatus::Failed,
        summary: readiness.summary,
        next_action: readiness.next_action,
        data: None,
        error: readiness.details,
    })
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

fn planning_artifact_paths_only(paths: &[String]) -> bool {
    paths.iter().all(|path| {
        (path.starts_with("backlog/items/") && path.ends_with(".md"))
            || (path.starts_with("backlog/plans/") && path.ends_with(".yaml"))
    })
}

fn auto_commit_planning_artifacts(root: &Path) -> Result<bool, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .map_err(|error| format!("failed to inspect git status: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
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
    if paths.is_empty() {
        return Ok(false);
    }
    if !paths.iter().all(|path| {
        path.starts_with("backlog/items/") && path.ends_with(".md")
            || path.starts_with("backlog/plans/") && path.ends_with(".yaml")
    }) {
        return Ok(false);
    }

    let add = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("add")
        .args(paths.iter().map(String::as_str))
        .output()
        .map_err(|error| format!("failed to stage planning artifacts: {error}"))?;
    if !add.status.success() {
        return Err(String::from_utf8_lossy(&add.stderr).trim().to_string());
    }

    let commit = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["commit", "-m", "Commit planning artifacts for dispatch"])
        .output()
        .map_err(|error| format!("failed to commit planning artifacts: {error}"))?;
    if !commit.status.success() {
        return Err(String::from_utf8_lossy(&commit.stderr).trim().to_string());
    }
    Ok(true)
}

fn state_error<T: schemars::JsonSchema + serde::Serialize>(
    action: &str,
    summary: &str,
    error: ProjectStateError,
) -> ActionResult<T> {
    ActionResult::failed(action, summary, error.to_string())
}

fn filter_candidates_for_dispatch(
    candidates: Vec<BacklogCandidate>,
    item_id: Option<&str>,
) -> Vec<BacklogCandidate> {
    match item_id {
        Some(item_id) => candidates
            .into_iter()
            .filter(|candidate| candidate.item_id == item_id)
            .collect(),
        None => candidates,
    }
}

fn backlog_candidate(candidate: BacklogCandidateSnapshot) -> BacklogCandidate {
    BacklogCandidate {
        source: candidate.source,
        item_id: candidate.item_id,
        title: candidate.title,
        priority: candidate.priority,
        item_type: candidate.item_type,
        area: candidate.area,
        suggested_worker: candidate.suggested_worker,
        owned_surfaces: candidate.owned_surfaces,
        external_refs: candidate.external_refs,
    }
}

fn task_record(task: TaskSnapshot) -> TaskRecord {
    let (workspace_path, workspace_branch, workspace_base_ref) = match task.worker_workspace {
        Some(workspace) => (
            Some(workspace.path),
            Some(workspace.branch),
            Some(workspace.base_ref),
        ),
        None => (None, None, None),
    };
    TaskRecord {
        id: task.id,
        source_item_id: task.source_item_id,
        title: task.title,
        status: task.status,
        worker: task.worker,
        claimed_by: task.claimed_by,
        claimed_at: task.claimed_at,
        started_at: task.started_at,
        finished_at: task.finished_at,
        workspace_path,
        workspace_branch,
        workspace_base_ref,
        created_at: task.created_at,
        updated_at: task.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DispatchReadyWorkParams;
    use std::{fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn dispatch_next_work_blocks_missing_git() {
        let project = backlog_project(false);

        let result = dispatch_next_work(project.path(), RootParams { root: None });

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.summary.contains("not initialized"));
        assert!(result.next_action.unwrap().contains("git init"));
    }

    #[test]
    fn dispatch_next_work_blocks_dirty_manager_workspace() {
        let project = backlog_project(true);
        fs::write(project.path().join("local.txt"), "dirty\n").expect("dirty");

        let result = dispatch_next_work(project.path(), RootParams { root: None });

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.summary.contains("local changes"));
        assert!(result.next_action.unwrap().contains("commit, stash"));
    }

    #[test]
    fn dispatch_ready_work_prepares_two_independent_handoffs() {
        let project = backlog_project(true);

        let result = dispatch_ready_work(
            project.path(),
            DispatchReadyWorkParams {
                root: None,
                item_id: None,
                max_tasks: Some(2),
                worker: Some("coder".to_string()),
                claimant: Some("tester".to_string()),
                prepare_handoffs: None,
                auto_start: None,
                auto_commit_artifacts: None,
                dry_run: None,
                verification_command: vec!["make check".to_string()],
            },
        );
        let data = result.data.expect("batch data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.dispatched, 2);
        assert_eq!(data.prepared, 2);
        assert_eq!(data.items.len(), 2);
        assert_eq!(data.items[0].item_id, "PROJ-001");
        assert_eq!(data.items[1].item_id, "PROJ-002");
        for item in data.items {
            assert_eq!(item.status, "prepared");
            assert!(item.task.as_ref().unwrap().id.starts_with(&item.item_id));
            assert!(item
                .assignment
                .as_ref()
                .unwrap()
                .worktree_path
                .contains(".platy/worktrees"));
        }
    }

    #[test]
    fn dispatch_ready_work_skips_active_item_and_dispatches_next() {
        let project = backlog_project(true);
        let first = dispatch_next_work(project.path(), RootParams { root: None });
        assert!(matches!(first.status, ActionStatus::Completed));

        let result = dispatch_ready_work(
            project.path(),
            DispatchReadyWorkParams {
                root: None,
                item_id: None,
                max_tasks: Some(1),
                worker: Some("coder".to_string()),
                claimant: Some("tester".to_string()),
                prepare_handoffs: Some(false),
                auto_start: None,
                auto_commit_artifacts: None,
                dry_run: None,
                verification_command: Vec::new(),
            },
        );
        let data = result.data.expect("batch data");

        assert_eq!(data.dispatched, 1);
        assert_eq!(data.items[0].item_id, "PROJ-002");
        assert_eq!(data.items[0].status, "queued");
    }

    #[test]
    fn dispatch_ready_work_can_target_specific_item() {
        let project = backlog_project(true);

        let result = dispatch_ready_work(
            project.path(),
            DispatchReadyWorkParams {
                root: None,
                item_id: Some("PROJ-002".to_string()),
                max_tasks: Some(1),
                worker: Some("coder".to_string()),
                claimant: Some("tester".to_string()),
                prepare_handoffs: Some(false),
                auto_start: None,
                auto_commit_artifacts: None,
                dry_run: None,
                verification_command: Vec::new(),
            },
        );
        let data = result.data.expect("batch data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.dispatched, 1);
        assert_eq!(data.items[0].item_id, "PROJ-002");
        assert_eq!(
            data.items[0].task.as_ref().unwrap().source_item_id,
            "PROJ-002"
        );
    }

    #[test]
    fn dispatch_ready_work_fails_when_no_handoff_can_be_prepared() {
        let project = backlog_project(true);
        fs::create_dir_all(project.path().join(".platy/worktrees/PROJ-001-T001"))
            .expect("worktree obstruction");
        fs::write(
            project
                .path()
                .join(".platy/worktrees/PROJ-001-T001/blocked.txt"),
            "not a worktree\n",
        )
        .expect("obstruction file");
        git(
            project.path(),
            &["add", ".platy/worktrees/PROJ-001-T001/blocked.txt"],
        );
        git(
            project.path(),
            &["commit", "-m", "Add obstructed worktree path"],
        );

        let result = dispatch_ready_work(
            project.path(),
            DispatchReadyWorkParams {
                root: None,
                item_id: None,
                max_tasks: Some(1),
                worker: Some("coder".to_string()),
                claimant: Some("tester".to_string()),
                prepare_handoffs: None,
                auto_start: None,
                auto_commit_artifacts: None,
                dry_run: None,
                verification_command: Vec::new(),
            },
        );
        let data = result.data.expect("batch data");

        assert!(matches!(result.status, ActionStatus::Failed));
        assert_eq!(data.dispatched, 1);
        assert_eq!(data.prepared, 0);
        assert_eq!(data.failed, 1);
        assert_eq!(data.items[0].status, "failed");
        assert!(result.summary.contains("no worker handoff"));
    }

    #[test]
    fn dispatch_ready_work_can_auto_start_prepared_assignments() {
        let project = backlog_project(true);

        let result = dispatch_ready_work(
            project.path(),
            DispatchReadyWorkParams {
                root: None,
                item_id: None,
                max_tasks: Some(1),
                worker: Some("coder".to_string()),
                claimant: Some("tester".to_string()),
                prepare_handoffs: Some(true),
                auto_start: Some(true),
                auto_commit_artifacts: None,
                dry_run: None,
                verification_command: vec!["make check".to_string()],
            },
        );
        let data = result.data.expect("batch data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.dispatched, 1);
        assert_eq!(data.prepared, 1);
        assert_eq!(data.started, 1);
        assert_eq!(data.items[0].status, "started");
        assert!(data.items[0].reason.contains("Started worker assignment"));
    }

    #[test]
    fn dispatch_ready_work_dry_run_previews_without_mutation() {
        let project = backlog_project(true);

        let result = dispatch_ready_work(
            project.path(),
            DispatchReadyWorkParams {
                root: None,
                item_id: None,
                max_tasks: Some(1),
                worker: Some("coder".to_string()),
                claimant: Some("tester".to_string()),
                prepare_handoffs: Some(true),
                auto_start: Some(true),
                auto_commit_artifacts: Some(true),
                dry_run: Some(true),
                verification_command: vec!["make check".to_string()],
            },
        );
        let data = result.data.expect("dry-run data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.dispatched, 0);
        assert_eq!(data.prepared, 0);
        assert_eq!(data.started, 0);
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].status, "preview");

        let queue = backlog::list_backlog(
            project.path(),
            Some(project.path().to_str().unwrap()),
            Some(10),
        )
        .data
        .expect("queue");
        assert_eq!(queue.candidates.len(), 2);
    }

    fn backlog_project(with_git: bool) -> TempDir {
        let project = TempDir::new().expect("temp dir");
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
        write_item(project.path(), "PROJ-001", "Frontend", "frontend");
        write_item(project.path(), "PROJ-002", "Backend", "backend");
        if with_git {
            git(project.path(), &["init"]);
            git(project.path(), &["config", "user.name", "Platypus Test"]);
            git(
                project.path(),
                &["config", "user.email", "platypus@example.invalid"],
            );
            fs::write(project.path().join("README.md"), "# Test\n").expect("readme");
            git(project.path(), &["add", "--all"]);
            git(project.path(), &["commit", "-m", "Initial commit"]);
        }
        project
    }

    fn write_item(root: &Path, id: &str, title: &str, surface: &str) {
        fs::write(
            root.join("backlog/items").join(format!("{id}.md")),
            format!(
                r#"---
id: {id}
title: {title}
priority: P1
type: feature
area: app
epic: general
depends_on: []
suggested_worker: coder
owned_surfaces:
- {surface}
---

# {id} {title}

## Goal

Build {title}.

## Implementation Contract

Edit {surface}.

## Acceptance

- Done.
"#,
            ),
        )
        .expect("item");
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
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
