use platypus_mcp::{
    backlog,
    models::{CreateBacklogItemParams, InitProjectParams},
    project,
    state::{
        sqlite::SqliteProjectState, AcquireLeaseCommand, AppendWorkerEventCommand,
        ApprovalResponse, AssignmentLifecycleState, CompleteExecutionCommand, EvidenceQuery,
        IntegrationStrategy, LeaseScope, MemoryProjectState, PrepareAssignmentCommand,
        ProjectState, RecordEvidenceCommand, RecordFindingCommand, ReplayEventsQuery,
        StartExecutionCommand, TaskLifecycleState, UpdateFindingDispositionCommand,
        ValidateFindingsQuery,
    },
};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path, process::Command};
use tempfile::TempDir;

#[test]
fn project_state_contract_runs_against_memory_backend() {
    let project = TempDir::new().expect("temp dir");
    let state = MemoryProjectState::new(project.path());

    exercise_lifecycle_contract(&state);
    exercise_evidence_findings_and_reconcile_contract(&state);
    exercise_lease_contract(&state);
}

#[test]
fn project_state_contract_runs_against_sqlite_backend() {
    let project = sqlite_project();
    let state = SqliteProjectState::open(project.path(), None).expect("state");

    exercise_lifecycle_contract(&state);
    exercise_evidence_findings_and_reconcile_contract(&state);
    exercise_lease_contract(&state);
}

fn exercise_lifecycle_contract(state: &impl ProjectState) {
    let dispatched = state
        .dispatch_work(platypus_mcp::state::DispatchWorkCommand {
            summary: Some("contract dispatch".to_string()),
            preferred_worker: Some("coder".to_string()),
            source_item_id: None,
        })
        .expect("dispatch");
    assert_eq!(dispatched.task.state, TaskLifecycleState::Queued);

    let assignment = state
        .prepare_assignment(PrepareAssignmentCommand {
            task_id: Some(dispatched.task.id.clone()),
            worker: Some("coder".to_string()),
            claimant: "contract".to_string(),
            execution_mode: None,
            base_ref: None,
            verification_command: vec!["make check".to_string()],
        })
        .expect("assignment");
    assert_eq!(assignment.state, AssignmentLifecycleState::Prepared);

    let running = state
        .start_execution(StartExecutionCommand {
            assignment_id: assignment.id.clone(),
            worker_session: Some("session-1".to_string()),
        })
        .expect("running");
    assert_eq!(running.state, AssignmentLifecycleState::Running);

    let event = state
        .append_worker_event(AppendWorkerEventCommand {
            assignment_id: assignment.id.clone(),
            event_type: "worker_progress".to_string(),
            summary: "contract progress".to_string(),
            payload: BTreeMap::from([("step".to_string(), Value::String("one".to_string()))]),
        })
        .expect("worker event");
    assert!(event.sequence > 0);

    let completed = state
        .complete_execution(CompleteExecutionCommand {
            assignment_id: assignment.id,
            status: "completed".to_string(),
            summary: "contract complete".to_string(),
            changed_files: vec!["README.md".to_string()],
            verification_status: Some("passed".to_string()),
        })
        .expect("complete");
    assert_eq!(completed.state, AssignmentLifecycleState::Completed);

    let task = state
        .inspect_task(platypus_mcp::state::TaskQuery {
            task_id: dispatched.task.id,
        })
        .expect("task");
    assert_eq!(task.state, TaskLifecycleState::Completed);

    let replay = state
        .replay_events(ReplayEventsQuery {
            task_id: None,
            scope: None,
            limit: Some(20),
        })
        .expect("events");
    assert!(!replay.events.is_empty());
}

fn exercise_evidence_findings_and_reconcile_contract(state: &impl ProjectState) {
    let dispatched = state
        .dispatch_work(platypus_mcp::state::DispatchWorkCommand {
            summary: None,
            preferred_worker: Some("verifier".to_string()),
            source_item_id: None,
        })
        .expect("dispatch");
    let assignment = state
        .prepare_assignment(PrepareAssignmentCommand {
            task_id: Some(dispatched.task.id.clone()),
            worker: Some("verifier".to_string()),
            claimant: "contract".to_string(),
            execution_mode: None,
            base_ref: None,
            verification_command: Vec::new(),
        })
        .expect("assignment");
    state
        .start_execution(StartExecutionCommand {
            assignment_id: assignment.id.clone(),
            worker_session: None,
        })
        .expect("running");
    state
        .complete_execution(CompleteExecutionCommand {
            assignment_id: assignment.id,
            status: "completed".to_string(),
            summary: "done".to_string(),
            changed_files: Vec::new(),
            verification_status: Some("passed".to_string()),
        })
        .expect("complete");

    let evidence = state
        .record_evidence(RecordEvidenceCommand {
            id: None,
            source_item_id: Some(dispatched.task.source_item_id.clone()),
            source_task_id: Some(dispatched.task.id.clone()),
            kind: "verification".to_string(),
            summary: "make check passed".to_string(),
            refs: vec!["log:1".to_string()],
            metadata: BTreeMap::new(),
        })
        .expect("evidence");
    let listed = state
        .list_evidence(EvidenceQuery {
            source_item_id: None,
            source_task_id: Some(dispatched.task.id.clone()),
            kind: Some("verification".to_string()),
            limit: None,
        })
        .expect("listed evidence");
    assert_eq!(listed.evidence.len(), 1);

    let finding = state
        .record_finding(RecordFindingCommand {
            id: None,
            source_item_id: Some(dispatched.task.source_item_id),
            source_task_id: Some(dispatched.task.id),
            source_finding_ref: None,
            title: "Contract finding".to_string(),
            summary: "Must be dispositioned.".to_string(),
            severity: Some("medium".to_string()),
            required: Some(true),
            evidence_refs: vec![format!("evidence:{}", evidence.id)],
            metadata: BTreeMap::new(),
        })
        .expect("finding");
    assert!(
        !state
            .validate_findings(ValidateFindingsQuery::default())
            .expect("validation")
            .ok
    );

    state
        .update_finding_disposition(UpdateFindingDispositionCommand {
            finding_id: finding.id,
            status: "resolved".to_string(),
            owner: Some("contract".to_string()),
            disposition_reason: Some("covered".to_string()),
            evidence_refs: Vec::new(),
            metadata: BTreeMap::new(),
        })
        .expect("disposition");
    assert!(
        state
            .validate_findings(ValidateFindingsQuery::default())
            .expect("validation")
            .ok
    );

    let reconciled = state
        .reconcile_project(platypus_mcp::state::ReconcileProjectQuery {
            include_closed_items: true,
        })
        .expect("reconcile");
    assert!(reconciled.completed_tasks >= 1);
}

fn exercise_lease_contract(state: &impl ProjectState) {
    state
        .acquire_lease(AcquireLeaseCommand {
            scope: LeaseScope::Task,
            target_id: "task-1".to_string(),
            owner: "owner-a".to_string(),
            ttl_seconds: 60,
            metadata: BTreeMap::new(),
        })
        .expect("lease");
    let same_owner_conflict = state.acquire_lease(AcquireLeaseCommand {
        scope: LeaseScope::Task,
        target_id: "task-1".to_string(),
        owner: "owner-a".to_string(),
        ttl_seconds: 60,
        metadata: BTreeMap::new(),
    });
    assert!(same_owner_conflict.is_err());
    let conflict = state.acquire_lease(AcquireLeaseCommand {
        scope: LeaseScope::Task,
        target_id: "task-1".to_string(),
        owner: "owner-b".to_string(),
        ttl_seconds: 60,
        metadata: BTreeMap::new(),
    });
    assert!(conflict.is_err());

    let unsupported = state.resolve_approval(platypus_mcp::state::ResolveApprovalCommand {
        approval_id: "missing".to_string(),
        response: ApprovalResponse::Approved,
        responder: "contract".to_string(),
        reason: None,
    });
    assert!(unsupported.is_err());

    let integration = state.integrate_result(platypus_mcp::state::IntegrateResultCommand {
        task_id: "missing".to_string(),
        strategy: IntegrationStrategy::External,
        verifier: "contract".to_string(),
        evidence_refs: Vec::new(),
    });
    assert!(integration.is_err());
}

fn sqlite_project() -> TempDir {
    let project = TempDir::new().expect("temp dir");
    git(project.path(), &["init"]);
    git(project.path(), &["config", "user.name", "Platypus Test"]);
    git(
        project.path(),
        &["config", "user.email", "platypus@example.invalid"],
    );
    fs::write(project.path().join("README.md"), "# Contract\n").expect("readme");
    git(project.path(), &["add", "README.md"]);
    git(project.path(), &["commit", "-m", "Initial commit"]);

    project::init_project(
        project.path(),
        InitProjectParams {
            root: None,
            project_name: Some("Contract".to_string()),
            overwrite: None,
        },
    );
    backlog::create_backlog_item(
        project.path(),
        CreateBacklogItemParams {
            root: None,
            id: Some("PROJ-001".to_string()),
            id_prefix: None,
            title: "Contract item".to_string(),
            priority: Some("P1".to_string()),
            item_type: Some("test".to_string()),
            area: Some("contract".to_string()),
            epic: None,
            depends_on: Vec::new(),
            suggested_worker: Some("coder".to_string()),
            owned_surfaces: Vec::new(),
            external_refs: Vec::new(),
            goal: "Exercise ProjectState.".to_string(),
            implementation_contract: Some("Run contract tests.".to_string()),
            contract: None,
            acceptance: vec!["Contract passes.".to_string()],
            notes: None,
        },
    );
    backlog::create_backlog_item(
        project.path(),
        CreateBacklogItemParams {
            root: None,
            id: Some("PROJ-002".to_string()),
            id_prefix: None,
            title: "Verification item".to_string(),
            priority: Some("P1".to_string()),
            item_type: Some("test".to_string()),
            area: Some("contract".to_string()),
            epic: None,
            depends_on: Vec::new(),
            suggested_worker: Some("verifier".to_string()),
            owned_surfaces: Vec::new(),
            external_refs: Vec::new(),
            goal: "Exercise ProjectState evidence.".to_string(),
            implementation_contract: Some("Run evidence contract tests.".to_string()),
            contract: None,
            acceptance: vec!["Contract passes.".to_string()],
            notes: None,
        },
    );
    git(project.path(), &["add", "--all"]);
    git(
        project.path(),
        &["commit", "-m", "Add Platypus contract scaffold"],
    );
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
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}
