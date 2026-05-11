//! Deterministic in-memory `ProjectState` implementation for contract tests.
//!
//! This backend is intentionally non-durable. It exists to prove that the
//! `ProjectState` contract is not secretly coupled to SQLite behavior.

use super::*;
use crate::assignments::validation::{clean_changed_files, validate_changed_files};
use crate::execution_mode;
use crate::models::{ExternalRef, TaskBundle};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Debug)]
pub struct MemoryProjectState {
    root: PathBuf,
    inner: Mutex<MemoryInner>,
}

#[derive(Debug, Default)]
struct MemoryInner {
    clock: u64,
    task_seq: u64,
    assignment_seq: u64,
    event_seq: i64,
    evidence_seq: u64,
    finding_seq: u64,
    lease_seq: u64,
    tasks: BTreeMap<String, TaskSnapshot>,
    assignments: BTreeMap<String, AssignmentSnapshot>,
    events: Vec<ProjectEventSnapshot>,
    worker_events: Vec<WorkerEventSnapshot>,
    evidence: BTreeMap<String, EvidenceSnapshot>,
    findings: BTreeMap<String, FindingSnapshot>,
    leases: BTreeMap<String, LeaseSnapshot>,
    integrations: BTreeMap<String, IntegrationSnapshot>,
}

impl MemoryProjectState {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            inner: Mutex::new(MemoryInner::default()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn lock(&self) -> StateResult<std::sync::MutexGuard<'_, MemoryInner>> {
        self.inner
            .lock()
            .map_err(|_| ProjectStateError::backend("memory state lock is poisoned"))
    }
}

impl MemoryInner {
    fn now(&mut self) -> String {
        self.clock += 1;
        format!("memory-time-{:06}", self.clock)
    }

    fn next_task_id(&mut self) -> String {
        self.task_seq += 1;
        format!("MEM-TASK-{:03}", self.task_seq)
    }

    fn next_assignment_id(&mut self) -> String {
        self.assignment_seq += 1;
        format!("MEM-ASN-{:03}", self.assignment_seq)
    }

    fn next_evidence_id(&mut self) -> String {
        self.evidence_seq += 1;
        format!("EVD-{:03}", self.evidence_seq)
    }

    fn next_finding_id(
        &mut self,
        source_item_id: Option<&str>,
        source_task_id: Option<&str>,
    ) -> String {
        self.finding_seq += 1;
        let prefix = source_item_id.or(source_task_id).unwrap_or("FIND");
        format!("{}-F{:03}", safe_id_prefix(prefix), self.finding_seq)
    }

    fn next_lease_id(&mut self) -> String {
        self.lease_seq += 1;
        format!("LEASE-{:03}", self.lease_seq)
    }

    fn push_event(
        &mut self,
        scope: &str,
        task_id: Option<String>,
        event_type: &str,
        summary: String,
        payload: Option<BTreeMap<String, Value>>,
    ) {
        self.event_seq += 1;
        let created_at = self.now();
        self.events.push(ProjectEventSnapshot {
            cursor: format!("memory:event:{}", self.event_seq),
            sequence: self.event_seq,
            scope: scope.to_string(),
            task_id,
            event_type: event_type.to_string(),
            summary,
            payload,
            created_at,
        });
    }
}

impl ProjectState for MemoryProjectState {
    fn describe_backend(&self) -> StateResult<BackendInfo> {
        Ok(BackendInfo {
            name: "memory".to_string(),
            version: None,
            capabilities: BackendCapabilities {
                durable: false,
                transactional_lifecycle: true,
                stable_event_replay: true,
                leases: true,
                shared_coordination: false,
                migrations: false,
                offline: true,
            },
        })
    }

    fn dispatch_work(&self, command: DispatchWorkCommand) -> StateResult<DispatchWorkOutcome> {
        let mut inner = self.lock()?;
        if let Some(lease) = inner.leases.values().find(|lease| {
            lease.scope == "project"
                && lease.target_id == "root"
                && matches!(lease.state, LeaseState::Active)
        }) {
            return Err(ProjectStateError::conflict(format!(
                "project is leased by `{}` until {}",
                lease.owner, lease.expires_at
            )));
        }
        let item_id = command
            .source_item_id
            .clone()
            .or_else(|| {
                command
                    .preferred_worker
                    .as_deref()
                    .map(|worker| format!("MEM-{worker}"))
            })
            .unwrap_or_else(|| "MEM-001".to_string());
        if inner.tasks.values().any(|task| {
            task.source_item_id == item_id
                && matches!(
                    task.state,
                    TaskLifecycleState::Queued
                        | TaskLifecycleState::Claimed
                        | TaskLifecycleState::Running
                )
        }) {
            return Err(ProjectStateError::conflict(format!(
                "active task already exists for backlog item `{item_id}`"
            )));
        }
        let now = inner.now();
        let task_id = inner.next_task_id();
        let title = format!("Memory task {item_id}");
        let worker = command
            .preferred_worker
            .or_else(|| Some("coder".to_string()));
        let task = TaskSnapshot {
            id: task_id.clone(),
            source_item_id: item_id.clone(),
            title: title.clone(),
            status: "queued".to_string(),
            state: TaskLifecycleState::Queued,
            worker: worker.clone(),
            claimed_by: None,
            claimed_at: None,
            started_at: None,
            finished_at: None,
            worker_workspace: None,
            created_at: now.clone(),
            updated_at: now,
        };
        inner.tasks.insert(task_id.clone(), task.clone());
        inner.push_event(
            "task",
            Some(task_id),
            "task_queued",
            command
                .summary
                .unwrap_or_else(|| format!("Queued {item_id} for external worker execution.")),
            None,
        );
        Ok(DispatchWorkOutcome {
            task,
            candidate: BacklogCandidateSnapshot {
                source: "memory".to_string(),
                item_id,
                title,
                priority: "P1".to_string(),
                area: "test".to_string(),
                item_type: "test".to_string(),
                suggested_worker: worker,
                owned_surfaces: Vec::new(),
                external_refs: Vec::<ExternalRef>::new(),
            },
        })
    }

    fn prepare_assignment(
        &self,
        command: PrepareAssignmentCommand,
    ) -> StateResult<AssignmentSnapshot> {
        let mut inner = self.lock()?;
        if command.claimant.trim().is_empty() {
            return Err(ProjectStateError::invalid_command("claimant is required"));
        }
        let task_id = if let Some(task_id) = command.task_id {
            task_id
        } else {
            inner
                .tasks
                .values()
                .find(|task| {
                    matches!(
                        task.state,
                        TaskLifecycleState::Queued | TaskLifecycleState::Claimed
                    ) && command
                        .worker
                        .as_deref()
                        .is_none_or(|worker| task.worker.as_deref() == Some(worker))
                })
                .map(|task| task.id.clone())
                .ok_or_else(|| {
                    ProjectStateError::not_found("no queued task is available for assignment")
                })?
        };
        if let Some(existing) = inner
            .assignments
            .values()
            .find(|assignment| {
                assignment.task_id == task_id
                    && matches!(
                        assignment.state,
                        AssignmentLifecycleState::Prepared | AssignmentLifecycleState::Running
                    )
            })
            .cloned()
        {
            return Ok(AssignmentSnapshot {
                reused_existing: true,
                ..existing
            });
        }
        let now = inner.now();
        let assignment_id = inner.next_assignment_id();
        let worktree_path = format!("{}/.platy/memory-worktrees/{task_id}", self.root.display());
        let task = {
            let task = inner
                .tasks
                .get_mut(&task_id)
                .ok_or_else(|| ProjectStateError::not_found("task was not found"))?;
            if !matches!(
                task.state,
                TaskLifecycleState::Queued | TaskLifecycleState::Claimed
            ) {
                return Err(ProjectStateError::conflict(format!(
                    "task `{}` is `{}`; only queued or claimed tasks can be prepared",
                    task.id, task.status
                )));
            }
            task.status = "claimed".to_string();
            task.state = TaskLifecycleState::Claimed;
            task.claimed_by = Some(command.claimant.clone());
            task.claimed_at = Some(now.clone());
            task.updated_at = now.clone();
            task.clone()
        };
        let bundle = TaskBundle {
            task_id: task.id.clone(),
            item_id: task.source_item_id.clone(),
            title: task.title.clone(),
            worker: command.worker.clone().or_else(|| task.worker.clone()),
            workspace_path: worktree_path.clone(),
            goal: task.title.clone(),
            implementation_contract: "Memory backend test assignment.".to_string(),
            acceptance: Vec::new(),
            dependencies: Vec::new(),
            owned_surfaces: vec![".".to_string()],
            verification_command: command.verification_command,
            execution_mode: execution_mode::normalize_assignment(command.execution_mode.as_deref())
                .map_err(ProjectStateError::invalid_command)?,
            completion_contract: crate::models::default_worker_completion_contract(),
            brief: "Memory backend test assignment.".to_string(),
        };
        let assignment = AssignmentSnapshot {
            id: assignment_id.clone(),
            task_id: task.id.clone(),
            worker: bundle.worker.clone(),
            state: AssignmentLifecycleState::Prepared,
            reused_existing: false,
            assigned_by: Some(command.claimant),
            worker_session: None,
            worktree_path,
            bundle,
            started_at: None,
            completed_at: None,
            result_status: None,
            summary: None,
            changed_files: Vec::new(),
            verification_status: None,
            created_at: now.clone(),
            updated_at: now,
        };
        inner.assignments.insert(assignment_id, assignment.clone());
        inner.push_event(
            "task",
            Some(task_id),
            "assignment_prepared",
            format!("Prepared memory assignment `{}`.", assignment.id),
            Some(BTreeMap::from([(
                "execution_mode".to_string(),
                Value::String(assignment.bundle.execution_mode.clone()),
            )])),
        );
        Ok(assignment)
    }

    fn start_execution(&self, command: StartExecutionCommand) -> StateResult<AssignmentSnapshot> {
        let mut inner = self.lock()?;
        let now = inner.now();
        let task_id = {
            let assignment = inner
                .assignments
                .get_mut(&command.assignment_id)
                .ok_or_else(|| ProjectStateError::not_found("worker assignment was not found"))?;
            if !matches!(assignment.state, AssignmentLifecycleState::Prepared) {
                return Err(ProjectStateError::conflict(format!(
                    "worker assignment `{}` is not prepared",
                    assignment.id
                )));
            }
            assignment.state = AssignmentLifecycleState::Running;
            assignment.started_at = Some(now.clone());
            assignment.worker_session = command.worker_session;
            assignment.updated_at = now.clone();
            assignment.task_id.clone()
        };
        let task = inner
            .tasks
            .get_mut(&task_id)
            .ok_or_else(|| ProjectStateError::not_found("task was not found"))?;
        task.status = "running".to_string();
        task.state = TaskLifecycleState::Running;
        task.started_at = Some(now.clone());
        task.updated_at = now;
        inner.push_event(
            "task",
            Some(task_id),
            "worker_started",
            format!("Started memory assignment `{}`.", command.assignment_id),
            None,
        );
        inner
            .assignments
            .get(&command.assignment_id)
            .cloned()
            .ok_or_else(|| ProjectStateError::not_found("worker assignment was not found"))
    }

    fn append_worker_event(
        &self,
        command: AppendWorkerEventCommand,
    ) -> StateResult<WorkerEventSnapshot> {
        let mut inner = self.lock()?;
        let assignment = inner
            .assignments
            .get(&command.assignment_id)
            .ok_or_else(|| ProjectStateError::not_found("worker assignment was not found"))?;
        if !matches!(
            assignment.state,
            AssignmentLifecycleState::Prepared | AssignmentLifecycleState::Running
        ) {
            return Err(ProjectStateError::conflict(
                "worker assignment is not prepared or running",
            ));
        }
        let sequence = inner
            .worker_events
            .iter()
            .filter(|event| event.assignment_id == command.assignment_id)
            .count() as i64
            + 1;
        let event = WorkerEventSnapshot {
            assignment_id: command.assignment_id,
            task_id: assignment.task_id.clone(),
            sequence,
            event_type: command.event_type,
            summary: command.summary,
            payload: command.payload,
            created_at: inner.now(),
        };
        inner.push_event(
            "task",
            Some(event.task_id.clone()),
            &event.event_type,
            event.summary.clone(),
            Some(event.payload.clone()),
        );
        inner.worker_events.push(event.clone());
        Ok(event)
    }

    fn complete_execution(
        &self,
        command: CompleteExecutionCommand,
    ) -> StateResult<AssignmentSnapshot> {
        if command.assignment_id.trim().is_empty() {
            return Err(ProjectStateError::invalid_command(
                "assignment_id is required",
            ));
        }
        if command.summary.trim().is_empty() {
            return Err(ProjectStateError::invalid_command("summary is required"));
        }
        if command.status == "completed" && command.verification_status.is_none() {
            return Err(ProjectStateError::invalid_command(
                "verification_status is required when status is completed; use passed, failed, skipped, or not_run",
            ));
        }
        let changed_files = clean_changed_files(command.changed_files)
            .map_err(ProjectStateError::invalid_command)?;
        let mut inner = self.lock()?;
        let now = inner.now();
        let task_id = {
            let assignment = inner
                .assignments
                .get_mut(&command.assignment_id)
                .ok_or_else(|| ProjectStateError::not_found("worker assignment was not found"))?;
            if !matches!(assignment.state, AssignmentLifecycleState::Running) {
                return Err(ProjectStateError::conflict(format!(
                    "worker assignment `{}` is not running",
                    assignment.id
                )));
            }
            validate_changed_files(&assignment.bundle.owned_surfaces, &changed_files)
                .map_err(ProjectStateError::invalid_command)?;
            assignment.state = if command.status == "completed" {
                AssignmentLifecycleState::Completed
            } else if command.status == "cancelled" {
                AssignmentLifecycleState::Cancelled
            } else if command.status == "failed" {
                AssignmentLifecycleState::Failed
            } else {
                return Err(ProjectStateError::invalid_command(
                    "status must be completed, failed, or cancelled",
                ));
            };
            assignment.completed_at = Some(now.clone());
            assignment.result_status = Some(command.status.clone());
            assignment.summary = Some(command.summary);
            assignment.changed_files = changed_files.clone();
            assignment.verification_status = command.verification_status;
            assignment.updated_at = now.clone();
            assignment.task_id.clone()
        };
        let task = inner
            .tasks
            .get_mut(&task_id)
            .ok_or_else(|| ProjectStateError::not_found("task was not found"))?;
        task.status = command.status.clone();
        task.state = if command.status == "completed" {
            TaskLifecycleState::Completed
        } else if command.status == "cancelled" {
            TaskLifecycleState::Cancelled
        } else {
            TaskLifecycleState::Failed
        };
        task.finished_at = Some(now.clone());
        task.updated_at = now;
        inner.push_event(
            "task",
            Some(task_id),
            "worker_result",
            format!(
                "Worker assignment `{}` finished with `{}`.",
                command.assignment_id, command.status
            ),
            None,
        );
        inner
            .assignments
            .get(&command.assignment_id)
            .cloned()
            .ok_or_else(|| ProjectStateError::not_found("worker assignment was not found"))
    }

    fn resolve_approval(&self, _command: ResolveApprovalCommand) -> StateResult<ApprovalSnapshot> {
        Err(ProjectStateError::unsupported(
            "resolve_approval",
            "memory backend does not create approval requests yet",
        ))
    }

    fn acquire_lease(&self, command: AcquireLeaseCommand) -> StateResult<LeaseSnapshot> {
        let mut inner = self.lock()?;
        if let Some(existing) = inner.leases.values().find(|lease| {
            lease.scope == lease_scope(&command.scope)
                && lease.target_id == command.target_id
                && matches!(lease.state, LeaseState::Active)
        }) {
            return Err(ProjectStateError::conflict(format!(
                "{} `{}` is leased by `{}` until {}",
                existing.scope, existing.target_id, existing.owner, existing.expires_at
            )));
        }
        let id = inner.next_lease_id();
        let lease = LeaseSnapshot {
            id: id.clone(),
            scope: lease_scope(&command.scope).to_string(),
            target_id: command.target_id,
            owner: command.owner,
            state: LeaseState::Active,
            metadata: command.metadata,
            expires_at: format!("memory-expiry-{}", command.ttl_seconds),
        };
        inner.leases.insert(id, lease.clone());
        Ok(lease)
    }

    fn integrate_result(
        &self,
        command: IntegrateResultCommand,
    ) -> StateResult<IntegrationSnapshot> {
        let mut inner = self.lock()?;
        let task = inner
            .tasks
            .get(&command.task_id)
            .ok_or_else(|| ProjectStateError::not_found("task was not found"))?
            .clone();
        let integrated_at = inner.now();
        let integration = IntegrationSnapshot {
            task_id: task.id.clone(),
            source_item_id: task.source_item_id,
            strategy: format!("{:?}", command.strategy).to_ascii_lowercase(),
            evidence_refs: command.evidence_refs,
            integrated_by: command.verifier,
            integrated_at,
        };
        inner.integrations.insert(task.id, integration.clone());
        Ok(integration)
    }

    fn record_workspace(&self, command: RecordWorkspaceCommand) -> StateResult<TaskSnapshot> {
        let mut inner = self.lock()?;
        let now = inner.now();
        let task = inner
            .tasks
            .get_mut(&command.task_id)
            .ok_or_else(|| ProjectStateError::not_found("task was not found"))?;
        task.worker_workspace = Some(WorkerWorkspaceSnapshot {
            path: command.path,
            branch: command.branch,
            base_ref: command.base_ref,
        });
        task.updated_at = now;
        Ok(task.clone())
    }

    fn clear_workspace(&self, command: ClearWorkspaceCommand) -> StateResult<TaskSnapshot> {
        let mut inner = self.lock()?;
        let now = inner.now();
        let task = inner
            .tasks
            .get_mut(&command.task_id)
            .ok_or_else(|| ProjectStateError::not_found("task was not found"))?;
        task.worker_workspace = None;
        task.updated_at = now;
        Ok(task.clone())
    }

    fn next_safe_action(&self, _query: NextSafeActionQuery) -> StateResult<SafeActionSnapshot> {
        let inner = self.lock()?;
        let summary = if let Some(assignment) = inner
            .assignments
            .values()
            .find(|assignment| matches!(assignment.state, AssignmentLifecycleState::Running))
        {
            format!("Worker assignment `{}` is running.", assignment.id)
        } else if let Some(task) = inner
            .tasks
            .values()
            .find(|task| matches!(task.state, TaskLifecycleState::Queued))
        {
            format!("Task `{}` is queued.", task.id)
        } else {
            "No memory-backed task needs action.".to_string()
        };
        Ok(SafeActionSnapshot {
            recommended_tool: "inspect_work_queue".to_string(),
            summary,
            reason: "Memory backend safe-action inspection is test scoped.".to_string(),
            params: BTreeMap::new(),
        })
    }

    fn inspect_task(&self, query: TaskQuery) -> StateResult<TaskSnapshot> {
        self.lock()?
            .tasks
            .get(&query.task_id)
            .cloned()
            .ok_or_else(|| ProjectStateError::not_found("task was not found"))
    }

    fn inspect_assignment(&self, query: AssignmentQuery) -> StateResult<AssignmentSnapshot> {
        self.lock()?
            .assignments
            .get(&query.assignment_id)
            .cloned()
            .ok_or_else(|| ProjectStateError::not_found("worker assignment was not found"))
    }

    fn replay_events(&self, query: ReplayEventsQuery) -> StateResult<EventReplaySnapshot> {
        let limit = query.limit.unwrap_or(50).clamp(1, 200);
        let mut events = self
            .lock()?
            .events
            .iter()
            .filter(|event| {
                query
                    .scope
                    .as_deref()
                    .is_none_or(|scope| event.scope == scope)
                    && query
                        .task_id
                        .as_deref()
                        .is_none_or(|task_id| event.task_id.as_deref() == Some(task_id))
            })
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        events.sort_by_key(|event| event.sequence);
        Ok(EventReplaySnapshot {
            cursor: events.last().map(|event| event.cursor.clone()),
            events,
        })
    }

    fn record_evidence(&self, command: RecordEvidenceCommand) -> StateResult<EvidenceSnapshot> {
        let mut inner = self.lock()?;
        let id = clean_optional(command.id).unwrap_or_else(|| inner.next_evidence_id());
        if inner.evidence.contains_key(&id) {
            return Err(ProjectStateError::conflict(format!(
                "evidence `{id}` already exists"
            )));
        }
        let evidence = EvidenceSnapshot {
            id: id.clone(),
            source_item_id: clean_optional(command.source_item_id),
            source_task_id: clean_optional(command.source_task_id),
            kind: command.kind.trim().to_string(),
            summary: command.summary.trim().to_string(),
            refs: clean_vec(command.refs),
            metadata: command.metadata,
            created_at: inner.now(),
        };
        inner.evidence.insert(id, evidence.clone());
        Ok(evidence)
    }

    fn list_evidence(&self, query: EvidenceQuery) -> StateResult<EvidenceListSnapshot> {
        let limit = query.limit.unwrap_or(50).clamp(1, 200);
        let evidence = self
            .lock()?
            .evidence
            .values()
            .filter(|evidence| {
                query
                    .source_item_id
                    .as_deref()
                    .is_none_or(|value| evidence.source_item_id.as_deref() == Some(value))
                    && query
                        .source_task_id
                        .as_deref()
                        .is_none_or(|value| evidence.source_task_id.as_deref() == Some(value))
                    && query
                        .kind
                        .as_deref()
                        .is_none_or(|value| evidence.kind == value)
            })
            .take(limit)
            .cloned()
            .collect();
        Ok(EvidenceListSnapshot { evidence })
    }

    fn record_finding(&self, command: RecordFindingCommand) -> StateResult<FindingSnapshot> {
        let mut inner = self.lock()?;
        let id = clean_optional(command.id).unwrap_or_else(|| {
            inner.next_finding_id(
                command.source_item_id.as_deref(),
                command.source_task_id.as_deref(),
            )
        });
        if inner.findings.contains_key(&id) {
            return Err(ProjectStateError::conflict(format!(
                "finding `{id}` already exists"
            )));
        }
        let now = inner.now();
        let finding = FindingSnapshot {
            id: id.clone(),
            source_item_id: clean_optional(command.source_item_id),
            source_task_id: clean_optional(command.source_task_id),
            source_finding_ref: clean_optional(command.source_finding_ref),
            title: command.title.trim().to_string(),
            status: "open".to_string(),
            severity: command.severity.unwrap_or_else(|| "medium".to_string()),
            required: command.required.unwrap_or(true),
            summary: command.summary.trim().to_string(),
            owner: None,
            disposition_reason: None,
            evidence_refs: command.evidence_refs,
            metadata: command.metadata,
            disposition: None,
            created_at: now.clone(),
            updated_at: now,
        };
        inner.findings.insert(id, finding.clone());
        Ok(finding)
    }

    fn list_findings(&self, query: FindingsQuery) -> StateResult<FindingsSnapshot> {
        let limit = query.limit.unwrap_or(20).clamp(1, 200);
        let findings = self
            .lock()?
            .findings
            .values()
            .filter(|finding| {
                query
                    .source_item_id
                    .as_deref()
                    .is_none_or(|value| finding.source_item_id.as_deref() == Some(value))
                    && query
                        .source_task_id
                        .as_deref()
                        .is_none_or(|value| finding.source_task_id.as_deref() == Some(value))
                    && query
                        .status
                        .as_deref()
                        .is_none_or(|value| finding.status == value)
            })
            .take(limit)
            .cloned()
            .collect();
        Ok(FindingsSnapshot { findings })
    }

    fn validate_findings(
        &self,
        query: ValidateFindingsQuery,
    ) -> StateResult<FindingsValidationSnapshot> {
        let unresolved_required = self
            .lock()?
            .findings
            .values()
            .filter(|finding| {
                finding.required
                    && !matches!(
                        finding.status.as_str(),
                        "resolved" | "rejected" | "duplicate"
                    )
                    && query
                        .source_item_id
                        .as_deref()
                        .is_none_or(|value| finding.source_item_id.as_deref() == Some(value))
                    && query
                        .source_task_id
                        .as_deref()
                        .is_none_or(|value| finding.source_task_id.as_deref() == Some(value))
            })
            .cloned()
            .collect::<Vec<_>>();
        Ok(FindingsValidationSnapshot {
            ok: unresolved_required.is_empty(),
            unresolved_required,
        })
    }

    fn update_finding_disposition(
        &self,
        command: UpdateFindingDispositionCommand,
    ) -> StateResult<FindingSnapshot> {
        let mut inner = self.lock()?;
        let now = inner.now();
        let finding = inner
            .findings
            .get_mut(&command.finding_id)
            .ok_or_else(|| ProjectStateError::not_found("finding was not found"))?;
        finding.status = command.status;
        finding.owner = clean_optional(command.owner);
        finding.disposition_reason = clean_optional(command.disposition_reason);
        finding.evidence_refs = command.evidence_refs;
        finding.metadata = command.metadata;
        finding.disposition =
            finding
                .disposition_reason
                .clone()
                .map(|reason| FindingDispositionSnapshot {
                    state: finding.status.clone(),
                    reason,
                    evidence_refs: finding.evidence_refs.clone(),
                });
        finding.updated_at = now;
        Ok(finding.clone())
    }

    fn reconcile_project(&self, _query: ReconcileProjectQuery) -> StateResult<ReconcileSnapshot> {
        let inner = self.lock()?;
        let mut gaps = Vec::new();
        let completed_tasks = inner
            .tasks
            .values()
            .filter(|task| matches!(task.state, TaskLifecycleState::Completed))
            .cloned()
            .collect::<Vec<_>>();
        for evidence in inner.evidence.values().filter(|evidence| {
            evidence.source_task_id.is_some()
                && matches!(evidence.kind.as_str(), "verification" | "commit")
        }) {
            let Some(source_task_id) = evidence.source_task_id.as_deref() else {
                continue;
            };
            match inner.tasks.get(source_task_id) {
                Some(task) if matches!(task.state, TaskLifecycleState::Completed) => {}
                Some(task) => gaps.push(ReconcileGap {
                    kind: "evidence_for_incomplete_task".to_string(),
                    source_item_id: evidence
                        .source_item_id
                        .clone()
                        .or_else(|| Some(task.source_item_id.clone())),
                    source_task_id: evidence.source_task_id.clone(),
                    summary: format!(
                        "Evidence `{}` of kind `{}` is attached to task `{source_task_id}` while it is `{}`.",
                        evidence.id, evidence.kind, task.status
                    ),
                    next_action:
                        "Complete the worker lifecycle before relying on verification or integration evidence."
                            .to_string(),
                }),
                None => gaps.push(ReconcileGap {
                    kind: "orphaned_task_evidence".to_string(),
                    source_item_id: evidence.source_item_id.clone(),
                    source_task_id: evidence.source_task_id.clone(),
                    summary: format!(
                        "Evidence `{}` of kind `{}` references missing task `{source_task_id}`.",
                        evidence.id, evidence.kind
                    ),
                    next_action:
                        "Record evidence against a valid lifecycle task or replace the orphaned evidence."
                            .to_string(),
                }),
            }
        }
        for task in &completed_tasks {
            if !inner.evidence.values().any(|evidence| {
                evidence.source_task_id.as_deref() == Some(task.id.as_str())
                    && evidence.kind == "verification"
            }) {
                gaps.push(ReconcileGap {
                    kind: "missing_verification_evidence".to_string(),
                    source_item_id: Some(task.source_item_id.clone()),
                    source_task_id: Some(task.id.clone()),
                    summary: format!(
                        "Completed task `{}` for `{}` has no verification evidence.",
                        task.id, task.source_item_id
                    ),
                    next_action: "Record verification evidence or rerun verification.".to_string(),
                });
            }
        }
        let unresolved_required = inner
            .findings
            .values()
            .filter(|finding| {
                finding.required
                    && !matches!(
                        finding.status.as_str(),
                        "resolved" | "rejected" | "duplicate"
                    )
            })
            .count();
        Ok(ReconcileSnapshot {
            ok: gaps.is_empty() && unresolved_required == 0,
            closed_item_ids: BTreeSet::new(),
            completed_tasks: completed_tasks.len(),
            unresolved_required_findings: unresolved_required,
            gaps,
        })
    }
}

fn lease_scope(scope: &LeaseScope) -> &'static str {
    match scope {
        LeaseScope::Project => "project",
        LeaseScope::Task => "task",
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn clean_vec(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|value| clean_optional(Some(value)))
        .collect()
}

fn safe_id_prefix(value: &str) -> String {
    let prefix = value
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                Some(character.to_ascii_uppercase())
            } else if character == '-' || character == '_' {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>();
    let prefix = prefix.trim_matches('-');
    if prefix.is_empty() {
        "FIND".to_string()
    } else {
        prefix.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_backend_reports_test_scoped_capabilities() {
        let state = MemoryProjectState::new("/tmp/project");
        let info = state.describe_backend().expect("backend info");

        assert_eq!(info.name, "memory");
        assert!(!info.capabilities.durable);
        assert!(info.capabilities.transactional_lifecycle);
        assert!(info.capabilities.stable_event_replay);
        assert!(info.capabilities.offline);
    }
}
