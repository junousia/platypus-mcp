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
use std::{collections::BTreeSet, path::Path, process::Command};

pub fn dispatch_next_work(
    default_root: &Path,
    params: RootParams,
) -> ActionResult<DispatchNextWorkData> {
    let action = "dispatch_next_work";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    if let Some(blocked) = dispatch_readiness_result(action, state.root(), false, &BTreeSet::new())
    {
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
    let active_source_items = active_source_item_ids(state.root());
    if params.dry_run.unwrap_or(false) {
        let listed = backlog::list_backlog(default_root, Some(root.as_str()), Some(100));
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
        let candidates = filter_candidates_for_dispatch(
            candidates,
            params.item_id.as_deref(),
            params.worker.as_deref(),
            &active_source_items,
        );
        let available_before_dispatch = candidates.len();
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
                available_before_dispatch,
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
    let listed = backlog::list_backlog(default_root, Some(root.as_str()), Some(100));
    let candidates = match listed {
        ActionResult {
            status: ActionStatus::Completed | ActionStatus::Skipped,
            data: Some(data),
            ..
        } => data.candidates,
        ActionResult { summary, error, .. } => {
            return ActionResult::failed(
                action,
                "Could not inspect dispatchable work.",
                error.unwrap_or(summary),
            )
        }
    };
    let candidates = filter_candidates_for_dispatch(
        candidates,
        params.item_id.as_deref(),
        params.worker.as_deref(),
        &active_source_items,
    );
    let available_before_dispatch = candidates.len();
    let requested = params
        .max_tasks
        .unwrap_or_else(|| available_before_dispatch.clamp(1, 10))
        .clamp(1, 10);
    let selected_item_ids = candidates
        .iter()
        .take(requested)
        .map(|candidate| candidate.item_id.clone())
        .collect::<BTreeSet<_>>();
    if let Some(blocked) = dispatch_readiness_result(
        action,
        state.root(),
        auto_commit_artifacts,
        &selected_item_ids,
    ) {
        return blocked;
    }
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
    selected_item_ids: &BTreeSet<String>,
) -> Option<ActionResult<T>> {
    if !auto_commit_artifacts {
        if let Some(paths) = manager_dirty_paths(root) {
            if !paths.is_empty() && dispatch_artifact_paths_only(&paths) {
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
                    "Manager workspace has uncommitted Platypus dispatch artifacts.",
                    format!(
                        "Pending artifact paths: {listed}{suffix}. Set auto_commit_artifacts=true on dispatch_ready_work, or stage and commit the Platypus scaffold/backlog artifacts before dispatch."
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
        match auto_commit_dispatch_artifacts(root, selected_item_ids) {
            Ok(true) => {
                readiness = inspect_git_readiness(root, true);
            }
            Ok(false) => {}
            Err(error) => {
                return Some(ActionResult::failed(
                    action,
                    "Could not auto-commit Platypus dispatch artifacts.",
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
    git_dirty_paths(root).ok()
}

fn git_dirty_paths(root: &Path) -> Result<Vec<String>, String> {
    Ok(git_dirty_entries(root)?
        .into_iter()
        .flat_map(|entry| entry.paths())
        .collect())
}

fn git_dirty_entries(root: &Path) -> Result<Vec<DirtyEntry>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .output()
        .map_err(|error| format!("failed to inspect git status: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(parse_porcelain_z_entries(&output.stdout))
}

#[cfg(test)]
fn parse_porcelain_z_paths(output: &[u8]) -> Vec<String> {
    parse_porcelain_z_entries(output)
        .into_iter()
        .flat_map(|entry| entry.paths())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirtyEntry {
    status: String,
    path: String,
    source: Option<String>,
}

impl DirtyEntry {
    fn paths(self) -> Vec<String> {
        let mut paths = vec![self.path];
        if let Some(source) = self.source {
            paths.push(source);
        }
        paths
    }
}

fn parse_porcelain_z_entries(output: &[u8]) -> Vec<DirtyEntry> {
    let mut paths = Vec::new();
    let mut entries = output
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let status = &entry[..2];
        let Some(path) = dirty_path(&entry[3..]) else {
            continue;
        };
        let mut dirty = DirtyEntry {
            status: String::from_utf8_lossy(status).to_string(),
            path,
            source: None,
        };
        if matches!(status.first(), Some(b'R' | b'C')) || matches!(status.get(1), Some(b'R' | b'C'))
        {
            if let Some(source) = entries.next() {
                dirty.source = dirty_path(source);
            }
        }
        paths.push(dirty);
    }
    paths
}

fn dirty_path(path: &[u8]) -> Option<String> {
    let path = String::from_utf8_lossy(path).trim().to_string();
    if path.is_empty() || path.starts_with(".platy/") {
        return None;
    }
    Some(path)
}

fn dispatch_artifact_paths_only(paths: &[String]) -> bool {
    paths.iter().all(|path| is_dispatch_artifact_path(path))
}

fn is_dispatch_artifact_path(path: &str) -> bool {
    matches!(
        path,
        ".gitignore" | "AGENTS.md" | "CLAUDE.md" | "WORKFLOW.md" | "platy.yaml"
    ) || path == "backlog/README.md"
        || (path.starts_with("backlog/items/") && path.ends_with(".md"))
        || (path.starts_with("backlog/plans/") && path.ends_with(".yaml"))
        || (path.starts_with("backlog/epics/") && path.ends_with(".md"))
        || (path.starts_with("backlog/templates/")
            && (path.ends_with(".md") || path.ends_with(".yaml")))
}

fn auto_commit_dispatch_artifacts(
    root: &Path,
    selected_item_ids: &BTreeSet<String>,
) -> Result<bool, String> {
    let entries = git_dirty_entries(root)?;
    if entries.is_empty() {
        return Ok(false);
    }
    let paths = entries
        .iter()
        .flat_map(|entry| entry.clone().paths())
        .collect::<Vec<_>>();
    if !dispatch_artifact_paths_only(&paths) {
        return Ok(false);
    }
    let unrelated = entries
        .iter()
        .filter(|entry| !auto_commit_entry_allowed(entry, selected_item_ids))
        .flat_map(|entry| entry.clone().paths())
        .collect::<Vec<_>>();
    if !unrelated.is_empty() {
        let listed = unrelated
            .iter()
            .take(5)
            .map(|path| format!("`{path}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if unrelated.len() > 5 {
            format!(" and {} more", unrelated.len() - 5)
        } else {
            String::new()
        };
        return Err(format!(
            "refusing to auto-commit unrelated backlog artifacts: {listed}{suffix}"
        ));
    }

    let staging_paths = entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>();
    let add = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("add")
        .arg("--all")
        .arg("--")
        .args(staging_paths)
        .output()
        .map_err(|error| format!("failed to stage planning artifacts: {error}"))?;
    if !add.status.success() {
        return Err(String::from_utf8_lossy(&add.stderr).trim().to_string());
    }

    let commit = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["commit", "-m", "Commit Platypus artifacts for dispatch"])
        .output()
        .map_err(|error| format!("failed to commit dispatch artifacts: {error}"))?;
    if !commit.status.success() {
        return Err(String::from_utf8_lossy(&commit.stderr).trim().to_string());
    }
    Ok(true)
}

fn auto_commit_entry_allowed(entry: &DirtyEntry, selected_item_ids: &BTreeSet<String>) -> bool {
    fixed_dispatch_artifact_path(&entry.path)
        || bootstrap_backlog_artifact_entry(entry)
        || selected_backlog_artifact_entry(entry, selected_item_ids)
}

fn fixed_dispatch_artifact_path(path: &str) -> bool {
    matches!(
        path,
        ".gitignore" | "AGENTS.md" | "CLAUDE.md" | "WORKFLOW.md" | "platy.yaml"
    )
}

fn bootstrap_backlog_artifact_entry(entry: &DirtyEntry) -> bool {
    if entry.status != "??" {
        return false;
    }
    matches!(
        entry.path.as_str(),
        "backlog/README.md"
            | "backlog/epics/general.md"
            | "backlog/templates/item.md"
            | "backlog/templates/plan.yaml"
            | "backlog/templates/epic.md"
    )
}

fn selected_backlog_artifact_entry(
    entry: &DirtyEntry,
    selected_item_ids: &BTreeSet<String>,
) -> bool {
    backlog_artifact_item_id(&entry.path).is_some_and(|item_id| selected_item_ids.contains(item_id))
}

fn backlog_artifact_item_id(path: &str) -> Option<&str> {
    if let Some(item_id) = path
        .strip_prefix("backlog/items/")
        .and_then(|value| value.strip_suffix(".md"))
    {
        return Some(item_id);
    }
    path.strip_prefix("backlog/plans/")
        .and_then(|value| value.strip_suffix(".yaml"))
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
    worker: Option<&str>,
    active_source_items: &BTreeSet<String>,
) -> Vec<BacklogCandidate> {
    candidates
        .into_iter()
        .filter(|candidate| item_id.is_none_or(|item_id| candidate.item_id == item_id))
        .filter(|candidate| {
            worker.is_none_or(|worker| candidate.suggested_worker.as_deref() == Some(worker))
        })
        .filter(|candidate| !active_source_items.contains(&candidate.item_id))
        .collect()
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
    fn dispatch_next_work_does_not_auto_commit_platypus_artifacts() {
        let project = backlog_project(true);
        let head_before = git_head(project.path());
        fs::write(
            project.path().join("backlog/items/PROJ-003.md"),
            "# Draft artifact\n",
        )
        .expect("dirty backlog artifact");

        let result = dispatch_next_work(project.path(), RootParams { root: None });

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.summary.contains("uncommitted Platypus"));
        assert_eq!(git_head(project.path()), head_before);
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
    fn dispatch_ready_work_dry_run_skips_active_items_like_real_dispatch() {
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
                dry_run: Some(true),
                verification_command: Vec::new(),
            },
        );
        let data = result.data.expect("dry-run data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.dispatched, 0);
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].item_id, "PROJ-002");
        assert_eq!(data.items[0].status, "preview");
    }

    #[test]
    fn dispatch_ready_work_dry_run_filters_by_worker_like_real_dispatch() {
        let project = backlog_project(true);

        let result = dispatch_ready_work(
            project.path(),
            DispatchReadyWorkParams {
                root: None,
                item_id: None,
                max_tasks: Some(1),
                worker: Some("qa".to_string()),
                claimant: Some("tester".to_string()),
                prepare_handoffs: Some(false),
                auto_start: None,
                auto_commit_artifacts: None,
                dry_run: Some(true),
                verification_command: Vec::new(),
            },
        );
        let data = result.data.expect("dry-run data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.available_before_dispatch, 0);
        assert_eq!(data.selected, 0);
        assert!(data.items.is_empty());
        assert_eq!(data.stopped_reason, "no_runnable_items");
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
        assert_eq!(data.available_before_dispatch, 2);
        assert_eq!(data.selected, 1);
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

    #[test]
    fn porcelain_parser_splits_rename_records() {
        let paths = parse_porcelain_z_paths(
            b"R  backlog/items/PROJ-002.md\0backlog/items/PROJ-001.md\0?? .platy/runtime.sqlite3\0",
        );

        assert_eq!(
            paths,
            vec![
                "backlog/items/PROJ-002.md".to_string(),
                "backlog/items/PROJ-001.md".to_string()
            ]
        );
    }

    #[test]
    fn auto_commit_dispatch_artifacts_handles_backlog_renames() {
        let project = backlog_project(true);
        git(
            project.path(),
            &[
                "mv",
                "backlog/items/PROJ-001.md",
                "backlog/items/PROJ-010.md",
            ],
        );
        let item_path = project.path().join("backlog/items/PROJ-010.md");
        let item = fs::read_to_string(&item_path)
            .expect("renamed item")
            .replace("PROJ-001", "PROJ-010");
        fs::write(&item_path, item).expect("update renamed item");
        let selected = BTreeSet::from(["PROJ-010".to_string()]);

        let committed =
            auto_commit_dispatch_artifacts(project.path(), &selected).expect("auto commit");

        assert!(committed);
        let status = Command::new("git")
            .args(["status", "--porcelain=v1", "--untracked-files=all"])
            .current_dir(project.path())
            .output()
            .expect("git status");
        assert!(status.status.success());
        assert!(
            String::from_utf8_lossy(&status.stdout).trim().is_empty(),
            "workspace should be clean after auto-commit"
        );
    }

    #[test]
    fn auto_commit_dispatch_artifacts_rejects_unselected_backlog_paths() {
        let project = backlog_project(true);
        fs::write(
            project.path().join("backlog/items/PROJ-001.md"),
            "selected edit\n",
        )
        .expect("selected edit");
        fs::write(
            project.path().join("backlog/items/PROJ-002.md"),
            "unrelated edit\n",
        )
        .expect("unrelated edit");
        let selected = BTreeSet::from(["PROJ-001".to_string()]);

        let error = auto_commit_dispatch_artifacts(project.path(), &selected)
            .expect_err("unselected backlog dirt should fail closed");

        assert!(error.contains("PROJ-002.md"));
        let staged = Command::new("git")
            .args(["diff", "--cached", "--name-only"])
            .current_dir(project.path())
            .output()
            .expect("git diff");
        assert!(staged.status.success());
        assert!(
            String::from_utf8_lossy(&staged.stdout).trim().is_empty(),
            "auto-commit should not stage a partial backlog set"
        );
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

    fn git_head(root: &Path) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git rev-parse");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}
