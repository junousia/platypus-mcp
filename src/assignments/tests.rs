use super::*;
use crate::tasks::{self, create_task_record, inspect_task_events, NewTask};
use std::{collections::BTreeMap, fs, process::Command};
use tempfile::TempDir;

#[test]
fn prepares_starts_records_and_completes_assignment() {
    let project = project_with_backlog();
    let task = create_task_record(
        project.path(),
        None,
        NewTask {
            source_item_id: "PROJ-001".to_string(),
            title: "External assignment".to_string(),
            worker: Some("coder".to_string()),
        },
    )
    .expect("task");

    let prepared = prepare_worker_assignment(
        project.path(),
        PrepareWorkerAssignmentParams {
            root: None,
            task_id: Some(task.id.clone()),
            worker: Some("coder".to_string()),
            claimant: Some("parent-agent".to_string()),
            base_ref: None,
            verification_command: vec!["make".to_string(), "check".to_string()],
        },
    );
    let assignment = prepared.data.expect("assignment data").assignment;
    assert!(matches!(prepared.status, ActionStatus::Completed));
    assert_eq!(assignment.status, "prepared");
    assert!(Path::new(&assignment.worktree_path).is_dir());
    assert!(assignment.bundle.brief.contains("External assignment"));

    let started = start_worker_execution(
        project.path(),
        StartWorkerExecutionParams {
            root: None,
            assignment_id: assignment.id.clone(),
            worker_session: Some("subagent-1".to_string()),
        },
    );
    let started_assignment = started.data.expect("started data").assignment;
    assert!(matches!(started.status, ActionStatus::Completed));
    assert_eq!(started_assignment.status, "running");
    assert_eq!(
        started_assignment.worker_session.as_deref(),
        Some("subagent-1")
    );

    let progress = record_worker_event(
        project.path(),
        RecordWorkerEventParams {
            root: None,
            assignment_id: assignment.id.clone(),
            event_type: "worker_progress".to_string(),
            summary: "README updated.".to_string(),
            payload: BTreeMap::new(),
        },
    );
    assert!(matches!(progress.status, ActionStatus::Completed));
    assert_eq!(
        progress.data.expect("event data").event.event_type,
        "worker_progress"
    );

    let completed = complete_worker_execution(
        project.path(),
        CompleteWorkerExecutionParams {
            root: None,
            assignment_id: assignment.id.clone(),
            status: "completed".to_string(),
            summary: "Implemented README change.".to_string(),
            changed_files: vec!["README.md".to_string()],
            verification_status: Some("passed".to_string()),
        },
    );
    let completed_assignment = completed.data.expect("complete data").assignment;
    assert!(matches!(completed.status, ActionStatus::Completed));
    assert_eq!(completed_assignment.status, "completed");
    assert_eq!(completed_assignment.changed_files, vec!["README.md"]);
    assert_eq!(
        completed_assignment.result_status.as_deref(),
        Some("completed")
    );

    let task = tasks::get_task_by_id(project.path(), None, &task.id).expect("task");
    assert_eq!(task.status, "completed");

    let events = inspect_task_events(
        project.path(),
        crate::models::InspectTaskEventsParams {
            root: None,
            task_id: task.id,
            limit: None,
        },
    )
    .data
    .expect("events");
    let event_types = events
        .events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect::<Vec<_>>();
    assert!(event_types.contains(&"task_claimed"));
    assert!(event_types.contains(&"worker_assignment_prepared"));
    assert!(event_types.contains(&"worker_started"));
    assert!(event_types.contains(&"worker_progress"));
    assert!(event_types.contains(&"worker_result"));
}

#[test]
fn refuses_duplicate_active_assignment() {
    let project = project_with_backlog();
    let task = create_task_record(
        project.path(),
        None,
        NewTask {
            source_item_id: "PROJ-001".to_string(),
            title: "External assignment".to_string(),
            worker: Some("coder".to_string()),
        },
    )
    .expect("task");

    let first = prepare_worker_assignment(
        project.path(),
        PrepareWorkerAssignmentParams {
            root: None,
            task_id: Some(task.id.clone()),
            worker: None,
            claimant: None,
            base_ref: None,
            verification_command: Vec::new(),
        },
    );
    assert!(matches!(first.status, ActionStatus::Completed));

    let second = prepare_worker_assignment(
        project.path(),
        PrepareWorkerAssignmentParams {
            root: None,
            task_id: Some(task.id),
            worker: None,
            claimant: None,
            base_ref: None,
            verification_command: Vec::new(),
        },
    );
    assert!(matches!(second.status, ActionStatus::Skipped));
    assert_eq!(
        second.data.expect("existing assignment").assignment.status,
        "prepared"
    );
}

#[test]
fn refuses_completion_before_start_and_unowned_files() {
    let project = project_with_backlog();
    let task = create_task_record(
        project.path(),
        None,
        NewTask {
            source_item_id: "PROJ-001".to_string(),
            title: "External assignment".to_string(),
            worker: Some("coder".to_string()),
        },
    )
    .expect("task");
    let assignment = prepare_worker_assignment(
        project.path(),
        PrepareWorkerAssignmentParams {
            root: None,
            task_id: Some(task.id),
            worker: None,
            claimant: None,
            base_ref: None,
            verification_command: Vec::new(),
        },
    )
    .data
    .expect("assignment")
    .assignment;

    let premature = complete_worker_execution(
        project.path(),
        CompleteWorkerExecutionParams {
            root: None,
            assignment_id: assignment.id.clone(),
            status: "completed".to_string(),
            summary: "Done.".to_string(),
            changed_files: vec!["README.md".to_string()],
            verification_status: Some("passed".to_string()),
        },
    );
    assert!(matches!(premature.status, ActionStatus::Skipped));

    start_worker_execution(
        project.path(),
        StartWorkerExecutionParams {
            root: None,
            assignment_id: assignment.id.clone(),
            worker_session: None,
        },
    );

    let unowned = complete_worker_execution(
        project.path(),
        CompleteWorkerExecutionParams {
            root: None,
            assignment_id: assignment.id,
            status: "completed".to_string(),
            summary: "Done.".to_string(),
            changed_files: vec!["src/lib.rs".to_string()],
            verification_status: Some("passed".to_string()),
        },
    );
    assert!(matches!(unowned.status, ActionStatus::Failed));
    assert!(unowned
        .error
        .expect("error")
        .contains("outside owned surfaces"));
}

#[test]
fn requires_verification_status_for_completed_assignment() {
    let project = project_with_backlog();
    let task = create_task_record(
        project.path(),
        None,
        NewTask {
            source_item_id: "PROJ-001".to_string(),
            title: "External assignment".to_string(),
            worker: Some("coder".to_string()),
        },
    )
    .expect("task");
    let assignment = prepare_worker_assignment(
        project.path(),
        PrepareWorkerAssignmentParams {
            root: None,
            task_id: Some(task.id),
            worker: None,
            claimant: None,
            base_ref: None,
            verification_command: Vec::new(),
        },
    )
    .data
    .expect("assignment")
    .assignment;
    start_worker_execution(
        project.path(),
        StartWorkerExecutionParams {
            root: None,
            assignment_id: assignment.id.clone(),
            worker_session: None,
        },
    );

    let result = complete_worker_execution(
        project.path(),
        CompleteWorkerExecutionParams {
            root: None,
            assignment_id: assignment.id,
            status: "completed".to_string(),
            summary: "Done.".to_string(),
            changed_files: vec!["README.md".to_string()],
            verification_status: None,
        },
    );

    assert!(matches!(result.status, ActionStatus::Failed));
    assert!(result
        .error
        .expect("error")
        .contains("verification_status is required"));
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
    fs::create_dir_all(project.path().join("backlog/items")).expect("items");
    fs::write(
        project.path().join("backlog/items/PROJ-001.md"),
        r#"---
id: PROJ-001
title: External assignment
priority: P0
type: foundation
area: execution
epic: general
depends_on: []
suggested_worker: coder
owned_surfaces:
- README.md
---

# PROJ-001 External assignment

## Goal

Verify external worker handoff.

## Implementation Contract

Edit only the assigned owned surface.

## Acceptance

- Assignment lifecycle completes.
"#,
    )
    .expect("item");
    git(&project, &["add", "backlog"]);
    git(&project, &["commit", "-m", "Add backlog fixture"]);
    project
}

fn git(project: &TempDir, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(project.path())
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
