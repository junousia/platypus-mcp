use crate::{
    assignments,
    models::{
        ActionResult, ActionStatus, CompleteWorkerExecutionParams, PrepareWorkerAssignmentParams,
        RecordWorkerEventParams, RunnerPrepareParams, RunnerReportData, RunnerTaskSummary,
        StartWorkerExecutionParams,
    },
    storage,
    tasks::{self, NewTaskEvent},
    workers::{WorkerAdapter, WorkerEvent, WorkerExitStatus, WorkerRequest},
};
use anyhow::Result;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
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
        let prepared = assignments::prepare_worker_assignment(
            default_root,
            PrepareWorkerAssignmentParams {
                root: Some(root.clone()),
                task_id: None,
                worker: params.worker.clone(),
                claimant: params
                    .claimant
                    .clone()
                    .or_else(|| Some("local-runner".to_string())),
                base_ref: None,
                verification_command: params.verification_command.clone(),
            },
        );
        let Some(prepared_data) = prepared.data else {
            report.stopped_reason = match prepared.status {
                ActionStatus::Skipped => "no_queued_tasks".to_string(),
                ActionStatus::Failed => "prepare_failed".to_string(),
                ActionStatus::Completed => "prepare_missing_data".to_string(),
            };
            return finish_report(action, report);
        };
        let assignment = prepared_data.assignment;
        report.claimed += 1;
        if let Err(error) = record_event(
            default_root,
            &root,
            &assignment.task_id,
            "runner_prepare_started",
            "Runner started task preparation.",
        ) {
            return error;
        }
        if let Err(error) = record_event(
            default_root,
            &root,
            &assignment.task_id,
            "task_bundle_generated",
            "Generated task bundle for worker execution.",
        ) {
            return error;
        }
        if let Err(error) = record_event(
            default_root,
            &root,
            &assignment.task_id,
            "runner_prepare_stopped",
            "Runner stopped after preparing task.",
        ) {
            return error;
        }
        report.prepared += 1;
        report.tasks.push(RunnerTaskSummary {
            assignment_id: Some(assignment.id),
            task_id: assignment.task_id,
            item_id: assignment.bundle.item_id,
            status: assignment.status,
            workspace_path: Some(assignment.worktree_path),
            bundle_generated: true,
        });
    }

    finish_report(action, report)
}

pub fn run_with_adapter(
    default_root: &Path,
    params: RunnerPrepareParams,
    adapter: &dyn WorkerAdapter,
) -> ActionResult<RunnerReportData> {
    let action = "runner_run_with_adapter";
    let prepared = prepare_next(default_root, params);
    let Some(mut report) = prepared.data else {
        return ActionResult::skipped(
            action,
            "No task was prepared for worker execution.",
            "Dispatch work before running a worker adapter.",
        );
    };
    if report.prepared == 0 {
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No prepared tasks to execute.".to_string(),
            next_action: Some("Dispatch work before running a worker adapter.".to_string()),
            data: Some(report),
            error: None,
        };
    }

    for task_summary in &mut report.tasks {
        let Some(assignment_id) = task_summary.assignment_id.clone() else {
            report.stopped_reason = "assignment_missing".to_string();
            return finish_report(action, report);
        };
        let started = assignments::start_worker_execution(
            default_root,
            StartWorkerExecutionParams {
                root: Some(report.root.clone()),
                assignment_id: assignment_id.clone(),
                worker_session: Some(format!("runner:{}", adapter.name())),
            },
        );
        let Some(started_data) = started.data else {
            report.stopped_reason = "adapter_start_failed".to_string();
            return finish_report(action, report);
        };
        let assignment = started_data.assignment;
        task_summary.status = assignment.status.clone();

        let mut emitted_events = Vec::new();
        let worker_result = adapter.run(
            WorkerRequest {
                task_id: assignment.task_id.clone(),
                item_id: assignment.bundle.item_id.clone(),
                title: assignment.bundle.title.clone(),
                workspace_path: assignment.bundle.workspace_path.clone(),
                brief: assignment.bundle.brief.clone(),
            },
            &mut |event| emitted_events.push(event),
        );
        for event in emitted_events {
            if let Err(error) =
                record_assignment_worker_event(default_root, &report.root, &assignment_id, event)
            {
                return error;
            }
        }
        let verification_status = worker_result.verification_status.or_else(|| {
            matches!(worker_result.status, WorkerExitStatus::Completed)
                .then(|| "not_run".to_string())
        });
        let completed = assignments::complete_worker_execution(
            default_root,
            CompleteWorkerExecutionParams {
                root: Some(report.root.clone()),
                assignment_id,
                status: worker_result.status.as_task_status().to_string(),
                summary: worker_result.summary,
                changed_files: worker_result.changed_files,
                verification_status,
            },
        );
        let Some(completed_data) = completed.data else {
            report.stopped_reason = "adapter_completion_failed".to_string();
            return finish_report(action, report);
        };
        task_summary.status = completed_data.assignment.status;
    }

    report.stopped_reason = "adapter_completed".to_string();
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
    record_event_payload(default_root, root, task_id, event_type, summary, None)
}

fn record_assignment_worker_event(
    default_root: &Path,
    root: &str,
    assignment_id: &str,
    event: WorkerEvent,
) -> Result<(), ActionResult<RunnerReportData>> {
    let recorded = assignments::record_worker_event(
        default_root,
        RecordWorkerEventParams {
            root: Some(root.to_string()),
            assignment_id: assignment_id.to_string(),
            event_type: event.event_type,
            summary: event.summary,
            payload: payload_map(event.payload),
        },
    );
    match recorded.status {
        ActionStatus::Completed => Ok(()),
        ActionStatus::Skipped | ActionStatus::Failed => Err(ActionResult::failed(
            "runner_run_with_adapter",
            "Could not record worker event.",
            recorded.error.unwrap_or(recorded.summary),
        )),
    }
}

fn payload_map(payload: Option<Value>) -> BTreeMap<String, Value> {
    match payload {
        Some(Value::Object(map)) => map.into_iter().collect(),
        Some(value) => {
            let mut map = Map::new();
            map.insert("value".to_string(), value);
            map.into_iter().collect()
        }
        None => BTreeMap::new(),
    }
}

fn record_event_payload(
    default_root: &Path,
    root: &str,
    task_id: &str,
    event_type: &str,
    summary: &str,
    payload: Option<serde_json::Value>,
) -> Result<(), ActionResult<RunnerReportData>> {
    tasks::record_task_event(
        default_root,
        Some(root),
        NewTaskEvent {
            task_id: task_id.to_string(),
            sequence: None,
            event_type: event_type.to_string(),
            summary: summary.to_string(),
            payload,
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
    use crate::workers::FakeWorkerAdapter;
    use crate::{
        dispatch,
        models::{InspectWorkerAssignmentParams, RootParams},
        project,
    };
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
        let assignment_id = data.tasks[0]
            .assignment_id
            .as_deref()
            .expect("assignment id");
        assert_eq!(data.tasks[0].item_id, "PROJ-001");
        assert!(data.tasks[0].workspace_path.is_some());
        assert!(data.tasks[0].bundle_generated);

        let inspected = assignments::inspect_worker_assignment(
            project.path(),
            InspectWorkerAssignmentParams {
                root: None,
                assignment_id: assignment_id.to_string(),
            },
        );
        let inspected = inspected.data.expect("assignment");
        assert_eq!(inspected.assignment.status, "prepared");
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

    #[test]
    fn fake_worker_completes_prepared_task_through_runner() {
        let project = project_with_backlog();
        let dispatched = dispatch::dispatch_next_work(project.path(), RootParams { root: None });
        let task_id = dispatched
            .data
            .as_ref()
            .expect("dispatch data")
            .task
            .id
            .clone();

        let result = run_with_adapter(
            project.path(),
            RunnerPrepareParams {
                root: None,
                worker: Some("coder".to_string()),
                claimant: Some("runner-test".to_string()),
                max_tasks: Some(1),
                dry_run: None,
                verification_command: Vec::new(),
            },
            &FakeWorkerAdapter::completed("fake"),
        );
        let data = result.data.expect("runner data");
        let assignment_id = data.tasks[0]
            .assignment_id
            .as_deref()
            .expect("assignment id")
            .to_string();
        let task = tasks::get_task_by_id(project.path(), None, &task_id).expect("task");

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(data.tasks[0].status, "completed");
        assert_eq!(task.status, "completed");
        assert!(task.finished_at.is_some());

        let inspected = assignments::inspect_worker_assignment(
            project.path(),
            InspectWorkerAssignmentParams {
                root: None,
                assignment_id,
            },
        );
        let inspected = inspected.data.expect("assignment");
        assert_eq!(inspected.assignment.status, "completed");
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
- README.md
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
