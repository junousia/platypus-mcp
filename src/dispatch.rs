use crate::{
    assignments, backlog,
    git_readiness::inspect_git_readiness,
    models::{
        ActionResult, ActionStatus, BacklogCandidate, DispatchNextWorkData, DispatchReadyWorkData,
        DispatchReadyWorkItem, DispatchReadyWorkParams, PrepareWorkerAssignmentParams, RootParams,
        TaskRecord,
    },
    state::{
        sqlite::SqliteProjectState, BacklogCandidateSnapshot, DispatchWorkCommand, ProjectState,
        ProjectStateError, TaskSnapshot,
    },
};
use std::path::Path;

pub fn dispatch_next_work(
    default_root: &Path,
    params: RootParams,
) -> ActionResult<DispatchNextWorkData> {
    let action = "dispatch_next_work";
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => return state_error(action, "Could not open project state.", error),
    };
    if let Some(blocked) = dispatch_readiness_result(action, state.root()) {
        return blocked;
    }
    match state.dispatch_work(DispatchWorkCommand {
        summary: None,
        preferred_worker: None,
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
    if let Some(blocked) = dispatch_readiness_result(action, state.root()) {
        return blocked;
    }

    let root = state.root().display().to_string();
    let available_before_dispatch =
        backlog::list_backlog(default_root, params.root.as_deref(), Some(100))
            .data
            .map(|data| data.candidates.len())
            .unwrap_or(0);
    let requested = params
        .max_tasks
        .unwrap_or_else(|| available_before_dispatch.clamp(1, 10))
        .clamp(1, 10);
    let prepare_handoffs = params.prepare_handoffs.unwrap_or(true);
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
        failed: 0,
        stopped_reason: "max_tasks_reached".to_string(),
        items: Vec::new(),
    };

    for _ in 0..requested {
        let outcome = match state.dispatch_work(DispatchWorkCommand {
            summary: None,
            preferred_worker: params.worker.clone(),
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
                report.items.push(DispatchReadyWorkItem {
                    item_id: candidate.item_id,
                    title: candidate.title,
                    status: "prepared".to_string(),
                    reason: summary,
                    task: Some(task),
                    assignment: Some(data.assignment),
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
    ActionResult {
        action: action.to_string(),
        status,
        summary: summary.clone(),
        next_action: Some(if report.prepared > 0 {
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
        }),
        data: Some(report),
        error: None,
    }
}

fn dispatch_readiness_result<T: schemars::JsonSchema + serde::Serialize>(
    action: &str,
    root: &Path,
) -> Option<ActionResult<T>> {
    let readiness = inspect_git_readiness(root, true);
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

fn state_error<T: schemars::JsonSchema + serde::Serialize>(
    action: &str,
    summary: &str,
    error: ProjectStateError,
) -> ActionResult<T> {
    ActionResult::failed(action, summary, error.to_string())
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
                max_tasks: Some(2),
                worker: Some("coder".to_string()),
                claimant: Some("tester".to_string()),
                prepare_handoffs: None,
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
                max_tasks: Some(1),
                worker: Some("coder".to_string()),
                claimant: Some("tester".to_string()),
                prepare_handoffs: Some(false),
                verification_command: Vec::new(),
            },
        );
        let data = result.data.expect("batch data");

        assert_eq!(data.dispatched, 1);
        assert_eq!(data.items[0].item_id, "PROJ-002");
        assert_eq!(data.items[0].status, "queued");
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
                max_tasks: Some(1),
                worker: Some("coder".to_string()),
                claimant: Some("tester".to_string()),
                prepare_handoffs: None,
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
