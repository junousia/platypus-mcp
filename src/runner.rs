use crate::{
    bundle,
    models::{
        ActionResult, ActionStatus, ClaimNextTaskParams, GenerateTaskBundleParams,
        RunnerPrepareParams, RunnerReportData, RunnerTaskSummary, WorktreeCreateParams,
    },
    storage,
    tasks::{self, NewTaskEvent},
    workspace,
};
use anyhow::Result;
use std::path::Path;

const DEFAULT_MAX_TASKS: usize = 1;
const MAX_TASKS: usize = 10;

pub fn prepare_next(
    default_root: &Path,
    params: RunnerPrepareParams,
) -> ActionResult<RunnerReportData> {
    let action = "runner_prepare_next";
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not open runner storage.",
                error.to_string(),
            )
        }
    };
    let root = storage.storage.root.display().to_string();
    let requested = params
        .max_tasks
        .unwrap_or(DEFAULT_MAX_TASKS)
        .clamp(1, MAX_TASKS);
    if params.dry_run.unwrap_or(false) {
        return ActionResult::skipped(
            action,
            "Dry run did not claim or prepare tasks.",
            "Run without dry_run to prepare queued tasks.",
        );
    }

    let mut report = RunnerReportData {
        root: root.clone(),
        requested,
        claimed: 0,
        prepared: 0,
        stopped_reason: "max_tasks_reached".to_string(),
        tasks: Vec::new(),
    };

    for _ in 0..requested {
        let claimed = tasks::claim_next_task(
            default_root,
            ClaimNextTaskParams {
                root: Some(root.clone()),
                worker: params.worker.clone(),
                claimant: params
                    .claimant
                    .clone()
                    .or_else(|| Some("local-runner".to_string())),
            },
        );
        let Some(claimed_data) = claimed.data else {
            report.stopped_reason = match claimed.status {
                ActionStatus::Skipped => "no_queued_tasks".to_string(),
                ActionStatus::Failed => "claim_failed".to_string(),
                ActionStatus::Completed => "claim_missing_data".to_string(),
            };
            return finish_report(action, report);
        };
        let task = claimed_data.task;
        report.claimed += 1;
        if let Err(error) = record_event(
            default_root,
            &root,
            &task.id,
            "runner_prepare_started",
            "Runner started task preparation.",
        ) {
            return error;
        }

        let worktree = workspace::worktree_create(
            default_root,
            WorktreeCreateParams {
                root: Some(root.clone()),
                task_id: task.id.clone(),
                base_ref: None,
            },
        );
        let Some(worktree_data) = worktree.data else {
            report.stopped_reason = "worktree_failed".to_string();
            return finish_report(action, report);
        };
        if matches!(worktree.status, ActionStatus::Failed) {
            report.stopped_reason = "worktree_failed".to_string();
            return finish_report(action, report);
        }

        let bundle = bundle::generate_task_bundle(
            default_root,
            GenerateTaskBundleParams {
                root: Some(root.clone()),
                task_id: task.id.clone(),
                verification_command: params.verification_command.clone(),
            },
        );
        let Some(bundle_data) = bundle.data else {
            report.stopped_reason = "bundle_failed".to_string();
            return finish_report(action, report);
        };
        if let Err(error) = record_event(
            default_root,
            &root,
            &task.id,
            "task_bundle_generated",
            "Generated task bundle for worker execution.",
        ) {
            return error;
        }
        if let Err(error) = record_event(
            default_root,
            &root,
            &task.id,
            "runner_prepare_stopped",
            "Runner stopped after preparing task.",
        ) {
            return error;
        }
        report.prepared += 1;
        report.tasks.push(RunnerTaskSummary {
            task_id: task.id,
            item_id: task.source_item_id,
            status: "claimed".to_string(),
            workspace_path: Some(worktree_data.path),
            bundle_generated: bundle_data.bundle.task_id == worktree_data.task_id,
        });
    }

    finish_report(action, report)
}

pub fn run_cli(args: &[String]) -> Result<()> {
    let params = parse_args(args)?;
    let root = params.root.clone().unwrap_or_else(|| ".".to_string());
    let result = prepare_next(Path::new(&root), params);
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn parse_args(args: &[String]) -> Result<RunnerPrepareParams> {
    let mut params = RunnerPrepareParams {
        root: None,
        worker: None,
        claimant: None,
        max_tasks: None,
        dry_run: None,
        verification_command: Vec::new(),
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--root" => {
                index += 1;
                params.root = Some(required_arg(args, index, "--root")?.to_string());
            }
            "--worker" => {
                index += 1;
                params.worker = Some(required_arg(args, index, "--worker")?.to_string());
            }
            "--claimant" => {
                index += 1;
                params.claimant = Some(required_arg(args, index, "--claimant")?.to_string());
            }
            "--max-tasks" => {
                index += 1;
                params.max_tasks = Some(required_arg(args, index, "--max-tasks")?.parse()?);
            }
            "--dry-run" => params.dry_run = Some(true),
            "--verify" => {
                index += 1;
                params
                    .verification_command
                    .push(required_arg(args, index, "--verify")?.to_string());
            }
            unknown => anyhow::bail!("unknown runner option `{unknown}`"),
        }
        index += 1;
    }
    Ok(params)
}

fn required_arg<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("{name} requires a value"))
}

fn record_event(
    default_root: &Path,
    root: &str,
    task_id: &str,
    event_type: &str,
    summary: &str,
) -> Result<(), ActionResult<RunnerReportData>> {
    tasks::record_task_event(
        default_root,
        Some(root),
        NewTaskEvent {
            task_id: task_id.to_string(),
            sequence: None,
            event_type: event_type.to_string(),
            summary: summary.to_string(),
            payload: None,
        },
    )
    .map(|_| ())
    .map_err(|error| {
        ActionResult::failed(
            "runner_prepare_next",
            "Could not record runner event.",
            error,
        )
    })
}

fn finish_report(action: &str, report: RunnerReportData) -> ActionResult<RunnerReportData> {
    let status = if report.prepared > 0 {
        ActionStatus::Completed
    } else {
        ActionStatus::Skipped
    };
    ActionResult {
        action: action.to_string(),
        status,
        summary: format!(
            "Runner prepared {} task(s), claimed {} task(s).",
            report.prepared, report.claimed
        ),
        next_action: Some("Wire a worker adapter to execute prepared tasks.".to_string()),
        data: Some(report),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dispatch, models::RootParams, project};
    use std::{fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn prepares_one_queued_task() {
        let project = project_with_backlog();
        let dispatched = dispatch::dispatch_next_work(project.path(), RootParams { root: None });
        assert!(matches!(dispatched.status, ActionStatus::Completed));

        let result = prepare_next(
            project.path(),
            RunnerPrepareParams {
                root: None,
                worker: Some("coder".to_string()),
                claimant: Some("runner-test".to_string()),
                max_tasks: Some(1),
                dry_run: None,
                verification_command: vec!["make".to_string(), "check".to_string()],
            },
        );
        let data = result.data.expect("runner data");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.claimed, 1);
        assert_eq!(data.prepared, 1);
        assert_eq!(data.tasks[0].item_id, "PROJ-001");
        assert!(data.tasks[0].workspace_path.is_some());
        assert!(data.tasks[0].bundle_generated);
    }

    #[test]
    fn stops_cleanly_when_no_tasks_are_queued() {
        let project = project_with_backlog();

        let result = prepare_next(
            project.path(),
            RunnerPrepareParams {
                root: None,
                worker: None,
                claimant: None,
                max_tasks: Some(1),
                dry_run: None,
                verification_command: Vec::new(),
            },
        );
        let data = result.data.expect("runner data");

        assert!(matches!(result.status, ActionStatus::Skipped));
        assert_eq!(data.stopped_reason, "no_queued_tasks");
        assert_eq!(data.prepared, 0);
    }

    fn project_with_backlog() -> TempDir {
        let project = TempDir::new().expect("temp dir");
        git(&project, &["init"]);
        git(&project, &["config", "user.name", "Platypus Test"]);
        git(
            &project,
            &["config", "user.email", "platypus@example.invalid"],
        );
        fs::write(project.path().join("README.md"), "# Test\n").expect("readme");
        git(&project, &["add", "README.md"]);
        git(&project, &["commit", "-m", "Initial commit"]);
        project::init_project(
            project.path(),
            crate::models::InitProjectParams {
                root: None,
                project_name: Some("Test".to_string()),
                overwrite: None,
            },
        );
        fs::write(
            project.path().join("backlog/items/PROJ-001.md"),
            r#"---
id: PROJ-001
title: Prepare task
priority: P0
type: foundation
area: execution
epic: general
depends_on: []
suggested_worker: coder
owned_surfaces:
- src/runner.rs
---

# PROJ-001 Prepare task

## Goal

Prepare one task.

## Implementation Contract

Claim, create worktree, and generate bundle.

## Acceptance

- Task is prepared.
"#,
        )
        .expect("item");
        project
    }

    fn git(project: &TempDir, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(project.path())
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
