use crate::{
    backlog,
    models::{
        ActionResult, ActionStatus, BacklogCandidate, BacklogListData, ClassifyPlanningNeedsParams,
        InspectWorkQueueParams, NextSafeActionData, NextSafeActionParams, PlanningClassification,
        PlanningClassificationData, TaskPlanQueryParams, WorkQueueData, WorkQueueItem,
        WorkQueuePlanState,
    },
    storage,
};
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub fn next_safe_action(
    default_root: &Path,
    params: NextSafeActionParams,
) -> ActionResult<NextSafeActionData> {
    let action = "next_safe_action";
    let storage = match storage::connect(default_root, params.root.as_deref()) {
        Ok(storage) => storage,
        Err(error) => {
            return ActionResult::failed(
                action,
                "Could not inspect next safe action.",
                error.to_string(),
            )
        }
    };
    let root = storage.storage.root.display().to_string();
    let closed_item_ids = backlog::closed_item_ids(&storage.storage.root);

    if let Ok(Some(assignment)) =
        active_assignment(&storage.connection, "running", &closed_item_ids)
    {
        return completed(
            action,
            &root,
            "record_worker_progress",
            format!(
                "Worker assignment `{}` is running for task `{}`.",
                assignment.id, assignment.task_id
            ),
            "Record progress while the worker is active, or complete the worker task when the result is ready.",
            [
                ("root", root.as_str()),
                ("assignment_id", assignment.id.as_str()),
                ("event_type", "worker_progress"),
                ("summary", "Describe the worker progress."),
            ],
        );
    }
    if let Ok(Some(assignment)) =
        active_assignment(&storage.connection, "prepared", &closed_item_ids)
    {
        return completed(
            action,
            &root,
            "start_worker_task",
            format!(
                "Worker assignment `{}` is prepared for task `{}`.",
                assignment.id, assignment.task_id
            ),
            "Start the prepared assignment before recording progress or completion.",
            [
                ("root", root.as_str()),
                ("assignment_id", assignment.id.as_str()),
                ("worker_session", "external-worker-session"),
                ("summary", ""),
            ],
        );
    }
    if let Ok(Some(task)) = queued_task(&storage.connection, &closed_item_ids) {
        return completed(
            action,
            &root,
            "prepare_worker_handoff",
            format!("Task `{}` is queued for worker execution.", task.id),
            "Prepare a single handoff object so the host can assign the worktree and bundle to a worker.",
            [
                ("root", root.as_str()),
                ("task_id", task.id.as_str()),
                ("worker", task.worker.as_deref().unwrap_or("")),
                ("claimant", "external-worker"),
            ],
        );
    }
    if let Ok(Some(task)) = claimed_task(&storage.connection, &closed_item_ids) {
        return completed(
            action,
            &root,
            "prepare_worker_handoff",
            format!(
                "Task `{}` is already claimed and needs a worker handoff.",
                task.id
            ),
            "Prepare a worker assignment for the claimed task before dispatching more backlog work.",
            [
                ("root", root.as_str()),
                ("task_id", task.id.as_str()),
                ("worker", task.worker.as_deref().unwrap_or("")),
                ("claimant", "external-worker"),
            ],
        );
    }
    if let Ok(Some(task)) = completed_task_without_verification(&storage.connection) {
        return completed(
            action,
            &root,
            "record_verification_evidence",
            format!("Completed task `{}` has no verification evidence.", task.id),
            "Record verification evidence before treating the work as reconciled.",
            [
                ("root", root.as_str()),
                ("source_item_id", task.source_item_id.as_str()),
                ("source_task_id", task.id.as_str()),
                ("summary", "Describe the verification result."),
            ],
        );
    }
    if let Ok(Some(task)) = completed_task_without_integration(&storage.connection) {
        return completed(
            action,
            &root,
            "integrate_worker_result",
            format!("Completed task `{}` has not been integrated.", task.id),
            "Integrate the completed verified worker result before reconciliation.",
            [("root", root.as_str()), ("task_id", task.id.as_str())],
        );
    }

    let backlog = backlog::list_backlog(default_root, Some(root.as_str()), Some(1));
    if let ActionResult {
        data: Some(BacklogListData { candidates, .. }),
        ..
    } = backlog
    {
        if let Some(candidate) = candidates.first() {
            return completed(
                action,
                &root,
                "dispatch_next_work",
                format!("Backlog item `{}` is runnable.", candidate.item_id),
                "Dispatch the next runnable backlog item to create a durable task.",
                [("root", root.as_str()), ("summary", "")],
            );
        }
    }

    completed(
        action,
        &root,
        "create_backlog_item",
        "No queued task or runnable backlog item was found.".to_string(),
        "Create or draft a backlog item before dispatching work.",
        [
            ("root", root.as_str()),
            ("summary", "Describe the work item."),
        ],
    )
}

pub fn inspect_work_queue(
    default_root: &Path,
    params: InspectWorkQueueParams,
) -> ActionResult<WorkQueueData> {
    let action = "inspect_work_queue";
    let require_task_plan = params.require_task_plan.unwrap_or(false);
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
                "Could not inspect executable work queue.",
                error.unwrap_or(summary),
            )
        }
    };

    let items: Vec<WorkQueueItem> = candidates
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| {
            work_queue_item(default_root, &root, index + 1, candidate, require_task_plan)
        })
        .collect();

    let (recommended_tool, reason, params) = recommended_queue_action(&root, &items);
    let summary = if items.is_empty() {
        "No runnable backlog items.".to_string()
    } else {
        format!("{} runnable backlog item(s) inspected.", items.len())
    };
    let status = if items.is_empty() {
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
            recommended_tool,
            summary,
            reason,
            params,
            items,
        }),
        error: None,
    }
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

#[derive(Debug)]
struct AssignmentHint {
    id: String,
    task_id: String,
    source_item_id: String,
}

#[derive(Debug)]
struct TaskHint {
    id: String,
    source_item_id: String,
    worker: Option<String>,
}

fn active_assignment(
    connection: &rusqlite::Connection,
    status: &str,
    closed_item_ids: &BTreeSet<String>,
) -> rusqlite::Result<Option<AssignmentHint>> {
    let mut statement = connection.prepare(
        r#"
        SELECT worker_assignments.id, worker_assignments.task_id, tasks.source_item_id
        FROM worker_assignments
        JOIN tasks ON tasks.id = worker_assignments.task_id
        WHERE worker_assignments.status = ?1
        ORDER BY worker_assignments.updated_at ASC, worker_assignments.id ASC
        "#,
    )?;
    let rows = statement.query_map([status], |row| {
        Ok(AssignmentHint {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            source_item_id: row.get("source_item_id")?,
        })
    })?;
    for row in rows {
        let assignment = row?;
        if !closed_item_ids.contains(&assignment.source_item_id) {
            return Ok(Some(assignment));
        }
    }
    Ok(None)
}

fn queued_task(
    connection: &rusqlite::Connection,
    closed_item_ids: &BTreeSet<String>,
) -> rusqlite::Result<Option<TaskHint>> {
    task_with_status(connection, "queued", "created_at", closed_item_ids)
}

fn claimed_task(
    connection: &rusqlite::Connection,
    closed_item_ids: &BTreeSet<String>,
) -> rusqlite::Result<Option<TaskHint>> {
    task_with_status(connection, "claimed", "updated_at", closed_item_ids)
}

fn task_with_status(
    connection: &rusqlite::Connection,
    status: &str,
    order_column: &str,
    closed_item_ids: &BTreeSet<String>,
) -> rusqlite::Result<Option<TaskHint>> {
    let query = format!(
        r#"
        SELECT id, source_item_id, worker
        FROM tasks
        WHERE status = ?1
        ORDER BY {order_column} ASC, id ASC
        "#
    );
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map([status], row_to_task_hint)?;
    for row in rows {
        let task = row?;
        if !closed_item_ids.contains(&task.source_item_id) {
            return Ok(Some(task));
        }
    }
    Ok(None)
}

fn completed_task_without_verification(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<Option<TaskHint>> {
    connection
        .query_row(
            r#"
            SELECT tasks.id, tasks.source_item_id, tasks.worker
            FROM tasks
            WHERE tasks.status = 'completed'
              AND NOT EXISTS (
                SELECT 1 FROM evidence
                WHERE evidence.source_task_id = tasks.id
                  AND evidence.kind = 'verification'
              )
            ORDER BY tasks.finished_at ASC, tasks.id ASC
            LIMIT 1
            "#,
            [],
            row_to_task_hint,
        )
        .optional()
}

fn completed_task_without_integration(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<Option<TaskHint>> {
    connection
        .query_row(
            r#"
            SELECT t.id, t.source_item_id, t.worker
            FROM tasks t
            WHERE t.status = 'completed'
              AND EXISTS (
                SELECT 1
                FROM evidence verification
                WHERE verification.source_task_id = t.id
                  AND verification.kind = 'verification'
              )
              AND NOT EXISTS (
                SELECT 1
                FROM evidence integration
                WHERE integration.source_task_id = t.id
                  AND integration.kind = 'commit'
              )
            ORDER BY t.updated_at ASC, t.id ASC
            LIMIT 1
            "#,
            [],
            |row| {
                Ok(TaskHint {
                    id: row.get("id")?,
                    source_item_id: row.get("source_item_id")?,
                    worker: row.get("worker")?,
                })
            },
        )
        .optional()
}

fn row_to_task_hint(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskHint> {
    Ok(TaskHint {
        id: row.get("id")?,
        source_item_id: row.get("source_item_id")?,
        worker: row.get("worker")?,
    })
}

fn work_queue_item(
    default_root: &Path,
    root: &str,
    position: usize,
    candidate: BacklogCandidate,
    require_task_plan: bool,
) -> WorkQueueItem {
    let planning = classify_candidate(&candidate);
    let plan = task_plan_state(default_root, root, &candidate.item_id);
    let plan_valid = plan.status == "valid";
    let planning_required = require_task_plan && planning.required_mode != "direct";
    let ready_to_dispatch = !planning_required || plan_valid;
    let (recommended_tool, reason) = if ready_to_dispatch {
        (
            "dispatch_next_work".to_string(),
            "Backlog item is runnable.".to_string(),
        )
    } else if plan.status == "missing" {
        (
            "draft_task_plan".to_string(),
            "A task plan is required before dispatch.".to_string(),
        )
    } else {
        (
            "validate_task_plan".to_string(),
            "Task plan must be fixed before dispatch.".to_string(),
        )
    };
    WorkQueueItem {
        position,
        candidate,
        planning,
        plan,
        ready_to_dispatch,
        recommended_tool,
        reason,
    }
}

fn classify_candidate(candidate: &BacklogCandidate) -> PlanningClassification {
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

    if candidate.owned_surfaces.len() > 1 {
        mode = "standard";
        reasons.push("touches multiple owned surfaces".to_string());
    }
    if matches!(
        candidate.item_type.as_str(),
        "foundation" | "feature" | "safety" | "ux"
    ) {
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
        reasons.push("small low-risk item can be handled directly".to_string());
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

    let item_id = first.candidate.item_id.as_str();
    let mut params = map_params([("root", root)]);
    match first.recommended_tool.as_str() {
        "dispatch_next_work" => {
            params.insert("summary".to_string(), Value::String(String::new()));
        }
        "draft_task_plan" | "validate_task_plan" => {
            params.insert("item_id".to_string(), Value::String(item_id.to_string()));
        }
        _ => {}
    }
    (
        first.recommended_tool.clone(),
        format!("{} {}", item_id, first.reason),
        params,
    )
}

fn completed<const N: usize>(
    action: &str,
    root: &str,
    recommended_tool: &str,
    summary: String,
    reason: &str,
    params: [(&str, &str); N],
) -> ActionResult<NextSafeActionData> {
    ActionResult::completed(
        action,
        summary.clone(),
        NextSafeActionData {
            root: root.to_string(),
            recommended_tool: recommended_tool.to_string(),
            summary,
            reason: reason.to_string(),
            params: params
                .into_iter()
                .filter(|(_, value)| !value.is_empty())
                .map(|(key, value)| (key.to_string(), Value::String(value.to_string())))
                .collect::<BTreeMap<_, _>>(),
        },
    )
}

fn map_params<const N: usize>(params: [(&str, &str); N]) -> BTreeMap<String, Value> {
    params
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (key.to_string(), Value::String(value.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{create_task_record, NewTask};
    use std::{fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn recommends_preparing_queued_task() {
        let project = TempDir::new().expect("temp dir");
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

        assert_eq!(data.recommended_tool, "dispatch_next_work");
        assert!(data.summary.contains("PROJ-002"));
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

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(true),
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(data.recommended_tool, "dispatch_next_work");
        assert_eq!(data.items[0].planning.required_mode, "direct");
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
    fn inspects_work_queue_and_recommends_dispatch_with_valid_plan() {
        let project = backlog_project();
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

        let result = inspect_work_queue(
            project.path(),
            InspectWorkQueueParams {
                root: None,
                limit: Some(10),
                require_task_plan: Some(true),
            },
        );
        let data = result.data.expect("queue");

        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(data.recommended_tool, "dispatch_next_work");
        assert_eq!(data.items[0].plan.status, "valid");
        assert!(data.items[0].ready_to_dispatch);
        assert_eq!(data.items[0].plan.task_count, 1);
    }

    fn backlog_project() -> TempDir {
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
        project
    }

    fn init_git_with_closed_item(root: &Path, item_id: &str) {
        git(root, &["init"]);
        git(root, &["config", "user.name", "Platypus Test"]);
        git(root, &["config", "user.email", "platypus@example.invalid"]);
        fs::write(root.join("README.md"), "# Test\n").expect("readme");
        git(root, &["add", "README.md"]);
        git(
            root,
            &[
                "commit",
                "-m",
                &format!("Close item\n\nPlatypus-Closes: {item_id}\nPlatypus-Verification: test"),
            ],
        );
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
