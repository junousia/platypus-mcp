mod closure;
mod create;
mod epic;
mod filesystem;
mod graph;
mod parse;
mod plan;
mod status;
mod types;
mod update;
mod validate;

pub use create::{create_backlog_item, create_backlog_items, quick_create_backlog_item};
pub use epic::{create_epic, list_epics};
pub use graph::inspect_dependency_graph;
pub use plan::{inspect_task_plan, list_task_plans, validate_task_plan, write_task_plan};
pub use status::{inspect_backlog_inventory, inspect_status, list_backlog};
pub use update::update_backlog_item;
pub use validate::validate_backlog;

pub(crate) use closure::{closed_item_ids, closure_sources};

#[derive(Debug, Clone, Default)]
pub(crate) struct BacklogItemExecutionPolicy {
    pub execution_path: Option<String>,
    pub planning_gate: Option<String>,
}

pub(crate) fn backlog_item_execution_policy(
    default_root: &std::path::Path,
    root: Option<&str>,
    item_id: &str,
) -> Result<BacklogItemExecutionPolicy, String> {
    let root = filesystem::resolve_root(default_root, root)?;
    let validation = validate::validate_backlog_at_root(&root, true);
    if !validation.ok {
        return Err(validation.errors.join("\n"));
    }
    let item = validation
        .items
        .iter()
        .find(|item| item.frontmatter.id == item_id)
        .ok_or_else(|| format!("backlog item `{item_id}` was not found"))?;
    Ok(BacklogItemExecutionPolicy {
        execution_path: item.frontmatter.execution_path.clone(),
        planning_gate: item.frontmatter.planning_gate.clone(),
    })
}

pub(crate) fn resolve_backlog_root(
    default_root: &std::path::Path,
    root: Option<&str>,
) -> Result<std::path::PathBuf, String> {
    filesystem::resolve_root(default_root, root)
}

pub(crate) fn external_ref_keys(
    root: &std::path::Path,
) -> Result<std::collections::BTreeSet<crate::integrations::ExternalRefKey>, String> {
    let validation = validate::validate_backlog_at_root(root, true);
    if !validation.ok {
        return Err(validation.errors.join("\n"));
    }
    Ok(validation
        .items
        .iter()
        .flat_map(|item| item.frontmatter.external_refs.iter())
        .map(crate::integrations::ExternalRefKey::from_ref)
        .collect())
}

#[derive(Debug, Clone)]
pub(crate) struct BacklogItemSnapshot {
    pub id: String,
    pub title: String,
    pub path: std::path::PathBuf,
    pub sections: Vec<String>,
    pub external_refs: Vec<crate::models::ExternalRef>,
}

pub(crate) fn backlog_item_snapshot(
    default_root: &std::path::Path,
    root: Option<&str>,
    item_id: &str,
) -> Result<(std::path::PathBuf, BacklogItemSnapshot), String> {
    let root = filesystem::resolve_root(default_root, root)?;
    let validation = validate::validate_backlog_at_root(&root, true);
    if !validation.ok {
        return Err(validation.errors.join("\n"));
    }
    let item = validation
        .items
        .iter()
        .find(|item| item.frontmatter.id == item_id)
        .ok_or_else(|| format!("backlog item `{item_id}` was not found"))?;
    Ok((
        root,
        BacklogItemSnapshot {
            id: item.frontmatter.id.clone(),
            title: item.frontmatter.title.clone(),
            path: item.path.clone(),
            sections: item.sections.iter().cloned().collect(),
            external_refs: item.frontmatter.external_refs.clone(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ActionStatus, ApprovalRespondParams, CreateBacklogItemParams, CreateBacklogItemsEntry,
        CreateBacklogItemsParams, CreateEpicParams, PlannedTask, QuickCreateBacklogItemParams,
        RequestPlanningApprovalParams, RootParams, TaskPlanDesign, TaskPlanFile,
        TaskPlanQueryParams, TaskPlanRequirement, UpdateBacklogItemParams, WriteTaskPlanParams,
    };
    use std::{fs, path::Path, process::Command};
    use tempfile::TempDir;

    #[test]
    fn validates_and_lists_runnable_backlog_items() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "First task", "P1", &[]);
        write_item(temp.path(), "PROJ-002", "Second task", "P0", &["PROJ-001"]);

        let root = root_arg(temp.path());
        let validation = validate_backlog(temp.path(), Some(root.as_str()), true);
        assert!(matches!(validation.status, ActionStatus::Completed));
        assert_eq!(validation.data.unwrap().item_count, 2);

        let listed = list_backlog(temp.path(), Some(root.as_str()), Some(10));
        let data = listed.data.unwrap();
        assert_eq!(data.candidates.len(), 1);
        assert_eq!(data.candidates[0].item_id, "PROJ-001");
    }

    #[test]
    fn inspect_status_reports_project_readiness_without_local_worker_config() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "First task", "P1", &[]);

        let status = inspect_status(temp.path(), Some(root_arg(temp.path()).as_str()), Some(10));
        let data = status.data.expect("status");

        assert_eq!(data.runnable_backlog_items, 1);
        assert!(data.tasks_supported);
        assert!(data.findings_supported);
    }

    #[test]
    fn validate_backlog_rejects_unknown_frontmatter_fields() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "First task", "P1", &[]);
        let path = temp.path().join("backlog/items/PROJ-001.md");
        let mut text = fs::read_to_string(&path).expect("item");
        text = text.replacen("owned_surfaces: []", "owned_surfaces: []\nstatus: done", 1);
        fs::write(path, text).expect("item");

        let validation = validate_backlog(temp.path(), Some(root_arg(temp.path()).as_str()), true);

        assert!(matches!(validation.status, ActionStatus::Failed));
        assert!(validation
            .error
            .as_deref()
            .unwrap_or("")
            .contains("unknown field `status`"));
    }

    #[test]
    fn validate_backlog_rejects_invalid_external_refs() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "First task", "P1", &[]);
        let path = temp.path().join("backlog/items/PROJ-001.md");
        let mut text = fs::read_to_string(&path).expect("item");
        text = text.replacen(
            "owned_surfaces: []",
            "owned_surfaces: []\nexternal_refs:\n- provider: github\n  kind: issue\n  id: owner/repo#1",
            1,
        );
        fs::write(path, text).expect("item");

        let validation = validate_backlog(temp.path(), Some(root_arg(temp.path()).as_str()), true);

        assert!(matches!(validation.status, ActionStatus::Failed));
        assert!(validation
            .error
            .as_deref()
            .unwrap_or("")
            .contains("external_refs require url or locator"));
    }

    #[test]
    fn validate_backlog_next_action_matches_direct_worker_and_mixed_queues() {
        let direct = project_fixture();
        write_item(direct.path(), "PROJ-001", "Direct work", "P1", &[]);
        let direct_validation =
            validate_backlog(direct.path(), Some(root_arg(direct.path()).as_str()), true);
        let direct_next = direct_validation.next_action.as_deref().unwrap_or("");
        assert!(matches!(direct_validation.status, ActionStatus::Completed));
        assert!(direct_next.contains("prepare_work"));
        assert!(direct_next.contains("complete_backlog_item"));
        assert!(!direct_next.contains("Commit backlog artifacts before dispatching"));

        let worker = project_fixture();
        write_item(worker.path(), "PROJ-001", "Worker work", "P1", &[]);
        set_item_policy(worker.path(), "PROJ-001", "worker_handoff", "task_plan");
        let worker_blocked =
            validate_backlog(worker.path(), Some(root_arg(worker.path()).as_str()), true);
        let worker_blocked_next = worker_blocked.next_action.as_deref().unwrap_or("");
        assert!(matches!(worker_blocked.status, ActionStatus::Completed));
        assert!(worker_blocked_next.contains("need valid task plans"));
        assert!(worker_blocked_next.contains("write_task_plan"));
        assert!(!worker_blocked_next.contains("commit_planning_artifacts"));
        let written = write_task_plan(
            worker.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(worker.path())),
                item_id: "PROJ-001".to_string(),
                plan: valid_task_plan("PROJ-001"),
                overwrite: None,
            },
        );
        assert!(matches!(written.status, ActionStatus::Completed));
        let worker_validation =
            validate_backlog(worker.path(), Some(root_arg(worker.path()).as_str()), true);
        let worker_next = worker_validation.next_action.as_deref().unwrap_or("");
        assert!(matches!(worker_validation.status, ActionStatus::Completed));
        assert!(worker_next.contains("commit_planning_artifacts"));
        assert!(worker_next.contains("worker worktrees"));

        let mixed = project_fixture();
        write_item(mixed.path(), "PROJ-001", "Direct work", "P1", &[]);
        write_item(mixed.path(), "PROJ-002", "Worker work", "P1", &[]);
        set_item_policy(mixed.path(), "PROJ-002", "worker_handoff", "task_plan");
        let written = write_task_plan(
            mixed.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(mixed.path())),
                item_id: "PROJ-002".to_string(),
                plan: valid_task_plan("PROJ-002"),
                overwrite: None,
            },
        );
        assert!(matches!(written.status, ActionStatus::Completed));
        let mixed_validation =
            validate_backlog(mixed.path(), Some(root_arg(mixed.path()).as_str()), true);
        let mixed_next = mixed_validation.next_action.as_deref().unwrap_or("");
        assert!(matches!(mixed_validation.status, ActionStatus::Completed));
        assert!(mixed_next.contains("Direct-ready"));
        assert!(mixed_next.contains("Worker-handoff"));
        assert!(mixed_next.contains("commit_planning_artifacts"));
    }

    #[test]
    fn validate_backlog_next_action_respects_planning_approval_gate() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Approved worker work", "P1", &[]);
        set_item_policy(
            temp.path(),
            "PROJ-001",
            "worker_handoff",
            "approved_task_plan",
        );
        let written = write_task_plan(
            temp.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                plan: valid_task_plan("PROJ-001"),
                overwrite: None,
            },
        );
        assert!(matches!(written.status, ActionStatus::Completed));

        let approval_needed =
            validate_backlog(temp.path(), Some(root_arg(temp.path()).as_str()), true);
        let approval_needed_next = approval_needed.next_action.as_deref().unwrap_or("");
        assert!(matches!(approval_needed.status, ActionStatus::Completed));
        assert!(approval_needed_next.contains("need planning approval"));
        assert!(approval_needed_next.contains("request_planning_approval"));
        assert!(!approval_needed_next.contains("commit_planning_artifacts"));

        let requested = crate::approvals::request_planning_approval(
            temp.path(),
            RequestPlanningApprovalParams {
                root: Some(root_arg(temp.path())),
                item_ids: vec!["PROJ-001".to_string()],
                requested_by: Some("manager".to_string()),
                summary: Some("Review the task plan before dispatch.".to_string()),
            },
        );
        assert!(matches!(requested.status, ActionStatus::Completed));
        let approval = requested.data.expect("approval").approval;

        let approved = crate::approvals::approval_respond(
            temp.path(),
            ApprovalRespondParams {
                root: Some(root_arg(temp.path())),
                approval_id: approval.id,
                decision: "approve".to_string(),
                responder: Some("user".to_string()),
                reason: Some("Reviewed.".to_string()),
            },
        );
        assert!(matches!(approved.status, ActionStatus::Completed));

        let ready = validate_backlog(temp.path(), Some(root_arg(temp.path()).as_str()), true);
        let ready_next = ready.next_action.as_deref().unwrap_or("");
        assert!(matches!(ready.status, ActionStatus::Completed));
        assert!(ready_next.contains("commit_planning_artifacts"));
        assert!(ready_next.contains("worker worktrees"));
    }

    #[test]
    fn list_backlog_excludes_items_closed_by_split_trailer_paragraphs() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "First task", "P1", &[]);
        write_item(temp.path(), "PROJ-002", "Second task", "P0", &["PROJ-001"]);
        git(temp.path(), &["init"]);
        git(temp.path(), &["config", "user.name", "Platypus Test"]);
        git(
            temp.path(),
            &["config", "user.email", "platypus@example.invalid"],
        );
        git(temp.path(), &["add", "--all"]);
        git(
            temp.path(),
            &[
                "commit",
                "-m",
                "Complete first task",
                "-m",
                "Platypus-Closes: PROJ-001",
                "-m",
                "Platypus-Verification: make check",
            ],
        );

        let root = root_arg(temp.path());
        let listed = list_backlog(temp.path(), Some(root.as_str()), Some(10));
        let data = listed.data.unwrap();

        assert_eq!(data.candidates.len(), 1);
        assert_eq!(data.candidates[0].item_id, "PROJ-002");
    }

    #[test]
    fn backlog_inventory_explains_closed_and_blocked_items() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "First task", "P1", &[]);
        write_item(temp.path(), "PROJ-002", "Second task", "P1", &["PROJ-001"]);
        write_item(temp.path(), "PROJ-003", "Third task", "P2", &["PROJ-002"]);
        git(temp.path(), &["init"]);
        git(temp.path(), &["config", "user.name", "Platypus Test"]);
        git(
            temp.path(),
            &["config", "user.email", "platypus@example.invalid"],
        );
        git(temp.path(), &["add", "--all"]);
        git(
            temp.path(),
            &[
                "commit",
                "-m",
                "Complete first task",
                "-m",
                "Platypus-Closes: PROJ-001",
                "-m",
                "Platypus-Verification: make check",
            ],
        );

        let inventory =
            inspect_backlog_inventory(temp.path(), Some(root_arg(temp.path()).as_str()), Some(10));
        let data = inventory.data.expect("inventory data");

        assert_eq!(data.total, 3);
        assert_eq!(data.returned, 3);
        assert!(!data.truncated);
        assert_eq!(data.closed, 1);
        assert_eq!(data.runnable, 1);
        assert_eq!(data.blocked, 1);
        assert_eq!(data.items[0].item_id, "PROJ-002");
        let closed = data
            .items
            .iter()
            .find(|item| item.item_id == "PROJ-001")
            .expect("closed item");
        assert!(closed.closed);
        assert!(closed.reason.contains("Platypus-Closes"));
        let runnable = data
            .items
            .iter()
            .find(|item| item.item_id == "PROJ-002")
            .expect("runnable item");
        assert!(runnable.runnable);
        let blocked = data
            .items
            .iter()
            .find(|item| item.item_id == "PROJ-003")
            .expect("blocked item");
        assert_eq!(blocked.open_dependencies, vec!["PROJ-002"]);
        assert!(blocked.reason.contains("PROJ-002"));

        let limited =
            inspect_backlog_inventory(temp.path(), Some(root_arg(temp.path()).as_str()), Some(2));
        let limited = limited.data.expect("limited inventory data");
        assert_eq!(limited.total, 3);
        assert_eq!(limited.returned, 2);
        assert!(limited.truncated);
    }

    #[test]
    fn creates_and_lists_epics() {
        let temp = project_fixture();

        let created = create_epic(
            temp.path(),
            CreateEpicParams {
                root: Some(root_arg(temp.path())),
                id: "webapp".to_string(),
                title: "Web Application".to_string(),
                status: None,
                priority: Some("P0".to_string()),
                area: None,
                description: Some("Web application work.".to_string()),
            },
        );
        assert!(matches!(created.status, ActionStatus::Completed));
        let data = created.data.expect("created epic");
        assert_eq!(data.epic.id, "webapp");
        assert_eq!(data.epic.status, "active");
        assert_eq!(data.epic.priority, "P0");
        assert_eq!(data.epic.area, "webapp");
        assert!(temp.path().join("backlog/epics/webapp.md").is_file());

        let listed = list_epics(
            temp.path(),
            RootParams {
                root: Some(root_arg(temp.path())),
            },
        );
        assert!(matches!(listed.status, ActionStatus::Completed));
        let listed = listed.data.expect("listed epics");
        assert_eq!(listed.returned, 2);
        assert_eq!(listed.epics[0].id, "general");
        assert_eq!(listed.epics[1].id, "webapp");

        let validation = validate_backlog(temp.path(), Some(root_arg(temp.path()).as_str()), true);
        assert!(matches!(validation.status, ActionStatus::Completed));
    }

    #[test]
    fn create_epic_rejects_invalid_values_with_recovery_guidance() {
        let temp = project_fixture();

        let invalid_id = create_epic(
            temp.path(),
            CreateEpicParams {
                root: Some(root_arg(temp.path())),
                id: "../webapp".to_string(),
                title: "Web Application".to_string(),
                status: None,
                priority: None,
                area: None,
                description: None,
            },
        );
        assert!(matches!(invalid_id.status, ActionStatus::Failed));
        assert!(invalid_id
            .next_action
            .as_deref()
            .unwrap_or("")
            .contains("webapp"));
        assert!(!temp.path().join("backlog/epics/../webapp.md").exists());

        let duplicate = create_epic(
            temp.path(),
            CreateEpicParams {
                root: Some(root_arg(temp.path())),
                id: "general".to_string(),
                title: "Duplicate".to_string(),
                status: None,
                priority: None,
                area: None,
                description: None,
            },
        );
        assert!(matches!(duplicate.status, ActionStatus::Failed));
        assert!(duplicate
            .error
            .as_deref()
            .unwrap_or("")
            .contains("backlog/epics/general.md"));

        let invalid_status = create_epic(
            temp.path(),
            CreateEpicParams {
                root: Some(root_arg(temp.path())),
                id: "webapp".to_string(),
                title: "Web Application".to_string(),
                status: Some("open".to_string()),
                priority: None,
                area: None,
                description: None,
            },
        );
        assert!(invalid_status
            .error
            .as_deref()
            .unwrap_or("")
            .contains("active, archived"));

        let invalid_priority = create_epic(
            temp.path(),
            CreateEpicParams {
                root: Some(root_arg(temp.path())),
                id: "webapp".to_string(),
                title: "Web Application".to_string(),
                status: None,
                priority: Some("high".to_string()),
                area: None,
                description: None,
            },
        );
        assert!(invalid_priority
            .error
            .as_deref()
            .unwrap_or("")
            .contains("P0, P1, P2"));
    }

    #[cfg(unix)]
    #[test]
    fn create_epic_rejects_preexisting_symlink_target() {
        let temp = project_fixture();
        let outside = TempDir::new().expect("outside temp dir");
        let outside_target = outside.path().join("outside.md");
        std::os::unix::fs::symlink(&outside_target, temp.path().join("backlog/epics/webapp.md"))
            .expect("symlink");

        let result = create_epic(
            temp.path(),
            CreateEpicParams {
                root: Some(root_arg(temp.path())),
                id: "webapp".to_string(),
                title: "Web Application".to_string(),
                status: None,
                priority: None,
                area: None,
                description: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.error.as_deref().unwrap_or("").contains("symlink"));
        assert!(!outside_target.exists());
    }

    #[test]
    fn create_backlog_item_allocates_next_id_and_writes_valid_markdown() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Existing", "P1", &[]);

        let result = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: None,
                id_prefix: None,
                title: "Create feature".to_string(),
                priority: Some("P1".to_string()),
                item_type: Some("feature".to_string()),
                area: Some("tooling".to_string()),
                epic: Some("general".to_string()),
                depends_on: vec!["PROJ-001".to_string()],
                owned_surfaces: vec!["src".to_string()],
                external_refs: vec![crate::models::ExternalRef {
                    provider: "github".to_string(),
                    kind: "issue".to_string(),
                    id: "owner/repo#1".to_string(),
                    url: Some("https://github.com/owner/repo/issues/1".to_string()),
                    locator: None,
                    imported_at: None,
                    source_hash: Some("sha256:test".to_string()),
                }],
                execution_path: None,
                planning_gate: None,
                goal: "Create the feature.".to_string(),
                implementation_contract: Some("Implement the scoped change.".to_string()),
                contract: None,
                acceptance: vec!["Feature works.".to_string()],
                notes: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let data = result.data.unwrap();
        assert_eq!(data.item_id, "PROJ-002");
        assert!(data.validation.ok);
        assert_eq!(data.validation.item_count, 2);
        assert!(temp.path().join("backlog/items/PROJ-002.md").is_file());

        let root = root_arg(temp.path());
        let validation = validate_backlog(temp.path(), Some(root.as_str()), true);
        assert!(matches!(validation.status, ActionStatus::Completed));
        let parsed = validate::validate_backlog_at_root(temp.path(), true);
        let created = parsed
            .items
            .iter()
            .find(|item| item.frontmatter.id == "PROJ-002")
            .expect("created item");
        assert_eq!(created.frontmatter.external_refs[0].id, "owner/repo#1");
    }

    #[test]
    fn create_backlog_item_blocks_invalid_existing_backlog_without_new_file() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Existing", "P1", &[]);
        let existing_path = temp.path().join("backlog/items/PROJ-001.md");
        let mut existing = fs::read_to_string(&existing_path).expect("existing item");
        existing = existing.replacen("owned_surfaces: []", "owned_surfaces: []\nstatus: done", 1);
        fs::write(&existing_path, existing).expect("corrupt item");

        let result = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-002".to_string()),
                id_prefix: None,
                title: "New item".to_string(),
                priority: None,
                item_type: None,
                area: None,
                epic: None,
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Create a new item.".to_string(),
                implementation_contract: None,
                contract: None,
                acceptance: Vec::new(),
                notes: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("existing backlog has"));
        assert!(!temp.path().join("backlog/items/PROJ-002.md").exists());
    }

    #[test]
    fn create_backlog_item_derives_fields_from_minimal_goal_or_title() {
        let temp = project_fixture();

        let goal_only = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-001".to_string()),
                id_prefix: None,
                title: String::new(),
                priority: None,
                item_type: None,
                area: None,
                epic: None,
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Scaffold frontend".to_string(),
                implementation_contract: None,
                contract: None,
                acceptance: Vec::new(),
                notes: None,
            },
        );
        assert!(matches!(goal_only.status, ActionStatus::Completed));
        let goal_text =
            fs::read_to_string(temp.path().join("backlog/items/PROJ-001.md")).expect("goal item");
        assert!(goal_text.contains("title: Scaffold frontend"));
        assert!(goal_text.contains(
            "## Implementation Contract\n\n_Not specified. Add a real implementation contract"
        ));
        assert!(!goal_text.contains("No implementation contract was provided"));
        assert!(!goal_text.contains("Implement the requested change: Scaffold frontend."));
        assert!(goal_text
            .contains("- Scaffold frontend is implemented and verification notes are recorded."));

        let title_only = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-002".to_string()),
                id_prefix: None,
                title: "Document recovery flow".to_string(),
                priority: None,
                item_type: None,
                area: None,
                epic: None,
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: String::new(),
                implementation_contract: None,
                contract: None,
                acceptance: Vec::new(),
                notes: None,
            },
        );
        assert!(matches!(title_only.status, ActionStatus::Completed));
        let title_text =
            fs::read_to_string(temp.path().join("backlog/items/PROJ-002.md")).expect("title item");
        assert!(title_text.contains("## Goal\n\nDocument recovery flow."));
        assert!(title_text.contains(
            "## Implementation Contract\n\n_Not specified. Add a real implementation contract"
        ));
        assert!(!title_text.contains("No implementation contract was provided"));
        assert!(!title_text.contains("Implement the requested change: Document recovery flow."));

        let validation = validate_backlog(temp.path(), Some(root_arg(temp.path()).as_str()), true);
        assert!(matches!(validation.status, ActionStatus::Completed));
    }

    #[test]
    fn create_backlog_item_accepts_common_type_aliases() {
        let temp = project_fixture();

        let result = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-001".to_string()),
                id_prefix: None,
                title: "Seed backlog".to_string(),
                priority: Some("P1".to_string()),
                item_type: Some("backlog".to_string()),
                area: Some("planning".to_string()),
                epic: Some("general".to_string()),
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Seed initial backlog items.".to_string(),
                implementation_contract: Some("Create valid backlog items.".to_string()),
                contract: None,
                acceptance: vec!["Backlog validates.".to_string()],
                notes: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let parsed = validate::validate_backlog_at_root(temp.path(), true);
        let created = parsed
            .items
            .iter()
            .find(|item| item.frontmatter.id == "PROJ-001")
            .expect("created item");
        assert_eq!(created.frontmatter.item_type, "feature");
    }

    #[test]
    fn create_backlog_item_reports_schema_guidance() {
        let temp = project_fixture();

        let omitted_required_fields: CreateBacklogItemParams =
            serde_json::from_value(serde_json::json!({
                "root": root_arg(temp.path()),
                "id": "PROJ-001"
            }))
            .expect("missing required fields deserialize to validation defaults");
        let missing_from_omitted = create_backlog_item(temp.path(), omitted_required_fields);
        let missing_from_omitted_error = missing_from_omitted.error.unwrap();
        let missing_from_omitted_fields = missing_from_omitted_error
            .split('.')
            .next()
            .expect("missing field sentence");
        assert!(missing_from_omitted_fields.contains("title|goal"));
        assert!(!missing_from_omitted_fields.contains("implementation_contract|contract"));
        assert!(!missing_from_omitted_fields.contains("acceptance"));

        let invalid_priority = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-001".to_string()),
                id_prefix: None,
                title: "Invalid priority".to_string(),
                priority: Some("high".to_string()),
                item_type: Some("feature".to_string()),
                area: Some("general".to_string()),
                epic: Some("general".to_string()),
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Goal.".to_string(),
                implementation_contract: Some("Contract.".to_string()),
                contract: None,
                acceptance: vec!["Done.".to_string()],
                notes: None,
            },
        );
        assert!(invalid_priority
            .error
            .as_deref()
            .unwrap_or("")
            .contains("P0, P1, P2"));
        assert!(invalid_priority
            .next_action
            .as_deref()
            .unwrap_or("")
            .contains("Set priority to one of"));

        let invalid_type = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-001".to_string()),
                id_prefix: None,
                title: "Invalid type".to_string(),
                priority: Some("P1".to_string()),
                item_type: Some("bug".to_string()),
                area: Some("general".to_string()),
                epic: Some("general".to_string()),
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Goal.".to_string(),
                implementation_contract: Some("Contract.".to_string()),
                contract: None,
                acceptance: vec!["Done.".to_string()],
                notes: None,
            },
        );
        let invalid_type_error = invalid_type.error.unwrap();
        assert!(invalid_type_error.contains("foundation"));
        assert!(invalid_type_error.contains("docs"));

        let missing = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-001".to_string()),
                id_prefix: None,
                title: " ".to_string(),
                priority: Some("P1".to_string()),
                item_type: Some("feature".to_string()),
                area: Some("general".to_string()),
                epic: Some("general".to_string()),
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: String::new(),
                implementation_contract: None,
                contract: None,
                acceptance: Vec::new(),
                notes: None,
            },
        );
        let missing_error = missing.error.unwrap();
        let missing_fields = missing_error
            .split('.')
            .next()
            .expect("missing field sentence");
        assert!(missing_fields.contains("title|goal"));
        assert!(!missing_fields.contains("implementation_contract|contract"));
        assert!(!missing_fields.contains("acceptance"));
    }

    #[test]
    fn create_backlog_item_rejects_conflicting_contract_aliases() {
        let temp = project_fixture();

        let result = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-001".to_string()),
                id_prefix: None,
                title: "Conflicting contract".to_string(),
                priority: Some("P1".to_string()),
                item_type: Some("feature".to_string()),
                area: Some("general".to_string()),
                epic: Some("general".to_string()),
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Create a conflicting request.".to_string(),
                implementation_contract: Some("Primary contract.".to_string()),
                contract: Some("Alias contract.".to_string()),
                acceptance: vec!["Done.".to_string()],
                notes: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("provide only one of implementation_contract or contract"));
        assert!(!temp.path().join("backlog/items/PROJ-001.md").exists());
    }

    #[test]
    fn create_backlog_item_reports_actionable_recovery_guidance() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Existing", "P1", &[]);

        let duplicate = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-001".to_string()),
                id_prefix: None,
                title: "Duplicate".to_string(),
                priority: Some("P1".to_string()),
                item_type: Some("feature".to_string()),
                area: Some("general".to_string()),
                epic: Some("general".to_string()),
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Goal.".to_string(),
                implementation_contract: Some("Contract.".to_string()),
                contract: None,
                acceptance: vec!["Done.".to_string()],
                notes: None,
            },
        );
        assert!(matches!(duplicate.status, ActionStatus::Failed));
        assert!(duplicate
            .error
            .as_deref()
            .unwrap_or("")
            .contains("backlog/items/PROJ-001.md"));
        assert!(duplicate
            .next_action
            .as_deref()
            .unwrap_or("")
            .contains("Omit id"));

        let unknown_epic = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-002".to_string()),
                id_prefix: None,
                title: "Unknown epic".to_string(),
                priority: Some("P1".to_string()),
                item_type: Some("feature".to_string()),
                area: Some("general".to_string()),
                epic: Some("webapp".to_string()),
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Goal.".to_string(),
                implementation_contract: Some("Contract.".to_string()),
                contract: None,
                acceptance: vec!["Done.".to_string()],
                notes: None,
            },
        );
        assert!(matches!(unknown_epic.status, ActionStatus::Failed));
        assert!(unknown_epic
            .error
            .as_deref()
            .unwrap_or("")
            .contains("unknown epic `webapp`; existing epics: general"));
        assert!(unknown_epic
            .next_action
            .as_deref()
            .unwrap_or("")
            .contains("backlog/epics/webapp.md"));

        let path_shaped_epic = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-002".to_string()),
                id_prefix: None,
                title: "Path shaped epic".to_string(),
                priority: Some("P1".to_string()),
                item_type: Some("feature".to_string()),
                area: Some("general".to_string()),
                epic: Some("../../README".to_string()),
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Goal.".to_string(),
                implementation_contract: Some("Contract.".to_string()),
                contract: None,
                acceptance: vec!["Done.".to_string()],
                notes: None,
            },
        );
        let path_shaped_next = path_shaped_epic.next_action.as_deref().unwrap_or("");
        assert!(path_shaped_next.contains("list_epics: general"));
        assert!(!path_shaped_next.contains("backlog/epics/../../README.md"));

        let missing_dependency = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-002".to_string()),
                id_prefix: None,
                title: "Missing dependency".to_string(),
                priority: Some("P1".to_string()),
                item_type: Some("feature".to_string()),
                area: Some("general".to_string()),
                epic: Some("general".to_string()),
                depends_on: vec!["PROJ-999".to_string()],
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Goal.".to_string(),
                implementation_contract: Some("Contract.".to_string()),
                contract: None,
                acceptance: vec!["Done.".to_string()],
                notes: None,
            },
        );
        assert!(matches!(missing_dependency.status, ActionStatus::Failed));
        assert!(missing_dependency
            .error
            .as_deref()
            .unwrap_or("")
            .contains("unknown dependency `PROJ-999`"));
        assert!(missing_dependency
            .next_action
            .as_deref()
            .unwrap_or("")
            .contains("Create the missing dependency item first"));
    }

    #[test]
    fn create_backlog_item_reports_missing_directory_recovery() {
        let temp = TempDir::new().expect("temp dir");

        let result = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-001".to_string()),
                id_prefix: None,
                title: "Missing directory".to_string(),
                priority: Some("P1".to_string()),
                item_type: Some("feature".to_string()),
                area: Some("general".to_string()),
                epic: Some("general".to_string()),
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Goal.".to_string(),
                implementation_contract: Some("Contract.".to_string()),
                contract: None,
                acceptance: vec!["Done.".to_string()],
                notes: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .next_action
            .as_deref()
            .unwrap_or("")
            .contains("init_project"));
    }

    #[cfg(unix)]
    #[test]
    fn create_backlog_item_rejects_preexisting_symlink_target() {
        let temp = project_fixture();
        let outside = TempDir::new().expect("outside temp dir");
        let outside_target = outside.path().join("outside.md");
        std::os::unix::fs::symlink(
            &outside_target,
            temp.path().join("backlog/items/PROJ-001.md"),
        )
        .expect("symlink");

        let result = create_backlog_item(
            temp.path(),
            CreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-001".to_string()),
                id_prefix: None,
                title: "Symlink target".to_string(),
                priority: Some("P1".to_string()),
                item_type: Some("feature".to_string()),
                area: Some("general".to_string()),
                epic: Some("general".to_string()),
                depends_on: Vec::new(),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                execution_path: None,
                planning_gate: None,
                goal: "Goal.".to_string(),
                implementation_contract: Some("Contract.".to_string()),
                contract: None,
                acceptance: vec!["Done.".to_string()],
                notes: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.error.as_deref().unwrap_or("").contains("symlink"));
        assert!(!outside_target.exists());
    }

    #[test]
    fn create_backlog_items_creates_chain_atomically() {
        let temp = project_fixture();

        let result = create_backlog_items(
            temp.path(),
            CreateBacklogItemsParams {
                root: Some(root_arg(temp.path())),
                id_prefix: Some("WEB".to_string()),
                preview: false,
                detail: None,
                items: vec![
                    batch_entry("foundation", None, "Shape web foundation", vec![]),
                    batch_entry(
                        "implementation",
                        None,
                        "Implement web foundation",
                        vec!["foundation".to_string()],
                    ),
                ],
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let data = result.data.expect("batch data");
        assert_eq!(data.created, 2);
        assert!(data.validation.ok);
        assert_eq!(data.validation.item_count, 2);
        assert_eq!(data.items[0].client_key.as_deref(), Some("foundation"));
        assert_eq!(data.items[0].item_id, "WEB-001");
        assert_eq!(data.items[1].item_id, "WEB-002");
        assert_eq!(data.items[1].depends_on, vec!["WEB-001"]);
        let next_action = result.next_action.as_deref().expect("next action");
        assert!(next_action.contains("inspect_work_queue"));
        assert!(next_action.contains("write_task_plan"));
        assert!(next_action.contains("worker handoff"));
        assert!(temp.path().join("backlog/items/WEB-001.md").is_file());
        assert!(temp.path().join("backlog/items/WEB-002.md").is_file());

        let validation = validate_backlog(temp.path(), Some(root_arg(temp.path()).as_str()), true);
        assert!(matches!(validation.status, ActionStatus::Completed));
    }

    #[test]
    fn create_backlog_items_previews_without_writes() {
        let temp = project_fixture();

        let result = create_backlog_items(
            temp.path(),
            CreateBacklogItemsParams {
                root: Some(root_arg(temp.path())),
                id_prefix: Some("WEB".to_string()),
                preview: true,
                detail: None,
                items: vec![
                    batch_entry("shape", None, "Shape web foundation", vec![]),
                    batch_entry(
                        "build",
                        None,
                        "Build web foundation",
                        vec!["shape".to_string()],
                    ),
                ],
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let data = result.data.expect("preview data");
        assert!(data.preview);
        assert_eq!(data.created, 0);
        assert_eq!(data.validation.item_count, 0);
        assert_eq!(data.items[0].item_id, "WEB-001");
        assert_eq!(data.items[1].item_id, "WEB-002");
        assert_eq!(data.items[1].depends_on, vec!["WEB-001"]);
        assert!(!data.items[0].created);
        assert_eq!(data.items[0].preview.title, "Shape web foundation");
        assert!(data.items[0].preview.markdown_included);
        assert!(data.items[0]
            .preview
            .markdown
            .as_deref()
            .unwrap_or("")
            .contains("# WEB-001 Shape web foundation"));
        assert!(!temp.path().join("backlog/items/WEB-001.md").exists());
        assert!(!temp.path().join("backlog/items/WEB-002.md").exists());
    }

    #[test]
    fn create_backlog_items_uses_empty_contract_when_omitted() {
        let temp = project_fixture();

        let result = create_backlog_items(
            temp.path(),
            CreateBacklogItemsParams {
                root: Some(root_arg(temp.path())),
                id_prefix: Some("WEB".to_string()),
                preview: true,
                detail: None,
                items: vec![CreateBacklogItemsEntry {
                    client_key: Some("minimal".to_string()),
                    depends_on_keys: Vec::new(),
                    id: None,
                    id_prefix: None,
                    title: String::new(),
                    priority: None,
                    item_type: None,
                    area: None,
                    epic: None,
                    depends_on: Vec::new(),
                    owned_surfaces: Vec::new(),
                    external_refs: Vec::new(),
                    execution_path: None,
                    planning_gate: None,
                    goal: "Scaffold frontend".to_string(),
                    implementation_contract: None,
                    contract: None,
                    acceptance: Vec::new(),
                    notes: None,
                }],
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let data = result.data.expect("preview data");
        assert_eq!(data.items[0].preview.title, "Scaffold frontend");
        assert_eq!(data.items[0].preview.implementation_contract, "");
        assert!(data.items[0].preview.markdown_included);
        let markdown = data.items[0].preview.markdown.as_deref().unwrap_or("");
        assert!(markdown.contains(
            "## Implementation Contract\n\n_Not specified. Add a real implementation contract"
        ));
        assert!(!markdown.contains("Implement the requested change: Scaffold frontend."));
        assert!(!temp.path().join("backlog/items/WEB-001.md").exists());
    }

    #[test]
    fn create_backlog_items_rejects_conflicting_contract_aliases_without_writes() {
        let temp = project_fixture();

        let result = create_backlog_items(
            temp.path(),
            CreateBacklogItemsParams {
                root: Some(root_arg(temp.path())),
                id_prefix: None,
                preview: false,
                detail: None,
                items: vec![CreateBacklogItemsEntry {
                    implementation_contract: Some("Primary contract.".to_string()),
                    contract: Some("Alias contract.".to_string()),
                    ..batch_entry("bad", Some("PROJ-001"), "Conflicting contract", vec![])
                }],
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("provide only one of implementation_contract or contract"));
        assert!(!temp.path().join("backlog/items/PROJ-001.md").exists());
    }

    #[test]
    fn quick_create_backlog_item_expands_to_canonical_create_path() {
        let temp = project_fixture();

        let result = quick_create_backlog_item(
            temp.path(),
            QuickCreateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                id: Some("PROJ-001".to_string()),
                id_prefix: None,
                title: "Add quick item".to_string(),
                goal: "Add a quick backlog item.".to_string(),
                priority: Some("P2".to_string()),
                item_type: Some("docs".to_string()),
                area: Some("backlog".to_string()),
                epic: Some("general".to_string()),
                depends_on: Vec::new(),
                owned_surfaces: vec!["docs/tools.md".to_string()],
                acceptance: vec!["The quick item is written.".to_string()],
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let data = result.data.expect("quick create data");
        assert_eq!(data.item_id, "PROJ-001");
        let text = fs::read_to_string(temp.path().join("backlog/items/PROJ-001.md"))
            .expect("created item");
        assert!(text.contains("priority: P2"));
        assert!(text.contains("type: docs"));
        assert!(text.contains("- docs/tools.md"));
        assert!(text.contains("- The quick item is written."));
    }

    #[test]
    fn create_backlog_items_blocks_invalid_existing_backlog_without_writes() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Existing", "P1", &[]);
        let existing_path = temp.path().join("backlog/items/PROJ-001.md");
        let mut existing = fs::read_to_string(&existing_path).expect("existing item");
        existing = existing.replacen("owned_surfaces: []", "owned_surfaces: []\nstatus: done", 1);
        fs::write(&existing_path, existing).expect("corrupt item");

        let result = create_backlog_items(
            temp.path(),
            CreateBacklogItemsParams {
                root: Some(root_arg(temp.path())),
                id_prefix: None,
                preview: false,
                detail: None,
                items: vec![batch_entry("new", Some("PROJ-002"), "New item", vec![])],
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("existing backlog has"));
        assert!(!temp.path().join("backlog/items/PROJ-002.md").exists());
    }

    #[test]
    fn create_backlog_items_supports_explicit_and_auto_ids() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Existing", "P1", &[]);

        let result = create_backlog_items(
            temp.path(),
            CreateBacklogItemsParams {
                root: Some(root_arg(temp.path())),
                id_prefix: None,
                preview: false,
                detail: None,
                items: vec![
                    batch_entry("explicit", Some("PROJ-010"), "Explicit item", vec![]),
                    batch_entry("auto", None, "Auto item", vec!["explicit".to_string()]),
                ],
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let data = result.data.expect("batch data");
        assert_eq!(data.items[0].item_id, "PROJ-010");
        assert_eq!(data.items[1].item_id, "PROJ-011");
        assert_eq!(data.items[1].depends_on, vec!["PROJ-010"]);
    }

    #[test]
    fn create_backlog_items_rolls_back_unknown_epic() {
        let temp = project_fixture();

        let result = create_backlog_items(
            temp.path(),
            CreateBacklogItemsParams {
                root: Some(root_arg(temp.path())),
                id_prefix: None,
                preview: false,
                detail: None,
                items: vec![
                    batch_entry("ok", None, "Would be valid", vec![]),
                    CreateBacklogItemsEntry {
                        epic: Some("missing".to_string()),
                        ..batch_entry("bad", None, "Bad epic", vec![])
                    },
                ],
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.error.as_deref().unwrap_or("").contains("item[1]"));
        assert!(result.error.as_deref().unwrap_or("").contains("bad"));
        assert!(!temp.path().join("backlog/items/PROJ-001.md").exists());
        assert!(!temp.path().join("backlog/items/PROJ-002.md").exists());
    }

    #[test]
    fn create_backlog_items_rolls_back_duplicate_id() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Existing", "P1", &[]);

        let result = create_backlog_items(
            temp.path(),
            CreateBacklogItemsParams {
                root: Some(root_arg(temp.path())),
                id_prefix: None,
                preview: false,
                detail: None,
                items: vec![
                    batch_entry("ok", Some("PROJ-002"), "Would be valid", vec![]),
                    batch_entry("duplicate", Some("PROJ-001"), "Duplicate", vec![]),
                ],
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("duplicate backlog id `PROJ-001`"));
        assert!(!temp.path().join("backlog/items/PROJ-002.md").exists());
    }

    #[test]
    fn create_backlog_items_rejects_invalid_dependency_key_without_writes() {
        let temp = project_fixture();

        let result = create_backlog_items(
            temp.path(),
            CreateBacklogItemsParams {
                root: Some(root_arg(temp.path())),
                id_prefix: None,
                preview: false,
                detail: None,
                items: vec![batch_entry(
                    "implementation",
                    None,
                    "Implement web foundation",
                    vec!["missing-key".to_string()],
                )],
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("unknown dependency client_key `missing-key`"));
        assert!(!temp.path().join("backlog/items/PROJ-001.md").exists());
    }

    #[test]
    fn create_backlog_items_rejects_dependency_key_cycles_without_writes() {
        let temp = project_fixture();

        let result = create_backlog_items(
            temp.path(),
            CreateBacklogItemsParams {
                root: Some(root_arg(temp.path())),
                id_prefix: None,
                preview: false,
                detail: None,
                items: vec![
                    batch_entry(
                        "shape",
                        None,
                        "Shape web foundation",
                        vec!["build".to_string()],
                    ),
                    batch_entry(
                        "build",
                        None,
                        "Build web foundation",
                        vec!["shape".to_string()],
                    ),
                ],
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("cyclic batch dependency"));
        assert!(!temp.path().join("backlog/items/PROJ-001.md").exists());
        assert!(!temp.path().join("backlog/items/PROJ-002.md").exists());
    }

    #[test]
    fn create_backlog_items_rejects_explicit_id_cycles_without_writes() {
        let temp = project_fixture();

        let mut first = batch_entry("first", Some("PROJ-001"), "First", vec![]);
        first.depends_on = vec!["PROJ-002".to_string()];
        let mut second = batch_entry("second", Some("PROJ-002"), "Second", vec![]);
        second.depends_on = vec!["PROJ-001".to_string()];

        let result = create_backlog_items(
            temp.path(),
            CreateBacklogItemsParams {
                root: Some(root_arg(temp.path())),
                id_prefix: None,
                preview: false,
                detail: None,
                items: vec![first, second],
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("cyclic batch dependency"));
        assert!(!temp.path().join("backlog/items/PROJ-001.md").exists());
        assert!(!temp.path().join("backlog/items/PROJ-002.md").exists());
    }

    #[test]
    fn write_task_plan_writes_and_validates_strict_task_plan() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Task planning", "P1", &[]);

        let written = write_task_plan(
            temp.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                plan: TaskPlanFile {
                    item_id: "PROJ-001".to_string(),
                    version: 1,
                    mode: "standard".to_string(),
                    requirements: vec![TaskPlanRequirement {
                        id: "R1".to_string(),
                        text: "Deliver the backlog item.".to_string(),
                    }],
                    design: TaskPlanDesign {
                        summary: "Implement a focused task planning slice.".to_string(),
                        owned_surfaces: vec!["README.md".to_string()],
                        notes: None,
                    },
                    tasks: vec![PlannedTask {
                        id: "PROJ-001-T001".to_string(),
                        title: "Implement task planning".to_string(),
                        goal: "Deliver concrete task planning behavior.".to_string(),
                        requirement_refs: vec!["R1".to_string()],
                        depends_on: Vec::new(),
                        owned_surfaces: vec!["README.md".to_string()],
                        verification: vec!["make check".to_string()],
                        acceptance: vec!["Task planning behavior is concrete.".to_string()],
                        notes: None,
                    }],
                },
                overwrite: None,
            },
        );
        assert!(matches!(written.status, ActionStatus::Completed));

        let validation = validate_task_plan(
            temp.path(),
            TaskPlanQueryParams {
                root: Some(root_arg(temp.path())),
                item_id: Some("PROJ-001".to_string()),
                include_errors: Some(true),
            },
        );
        assert!(matches!(validation.status, ActionStatus::Completed));
        assert!(validation.data.unwrap().ok);
        let next_action = validation.next_action.as_deref().unwrap_or("");
        assert!(next_action.contains("prepare_work"));
        assert!(next_action.contains("complete_backlog_item"));
        assert!(!next_action.contains("Commit task-plan artifacts before dispatching"));
    }

    #[test]
    fn write_task_plan_next_action_protects_worker_handoff_artifacts() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Worker planning", "P1", &[]);
        set_item_policy(temp.path(), "PROJ-001", "worker_handoff", "task_plan");

        let written = write_task_plan(
            temp.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                plan: valid_task_plan("PROJ-001"),
                overwrite: None,
            },
        );
        assert!(matches!(written.status, ActionStatus::Completed));
        let next_action = written.next_action.as_deref().unwrap_or("");
        assert!(next_action.contains("commit_planning_artifacts"));
        assert!(next_action.contains("worker worktrees"));
    }

    #[test]
    fn validate_task_plan_normalizes_item_filter_for_next_action() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Worker planning", "P1", &[]);
        set_item_policy(temp.path(), "PROJ-001", "worker_handoff", "task_plan");

        let written = write_task_plan(
            temp.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                plan: valid_task_plan("PROJ-001"),
                overwrite: None,
            },
        );
        assert!(matches!(written.status, ActionStatus::Completed));

        let validation = validate_task_plan(
            temp.path(),
            TaskPlanQueryParams {
                root: Some(root_arg(temp.path())),
                item_id: Some("proj-001".to_string()),
                include_errors: Some(true),
            },
        );

        let next_action = validation.next_action.as_deref().unwrap_or("");
        assert!(matches!(validation.status, ActionStatus::Completed));
        assert!(next_action.contains("commit_planning_artifacts"));
        assert!(next_action.contains("worker worktrees"));
        assert!(!next_action.contains("Inspect the queue"));
    }

    #[test]
    fn write_task_plan_rejects_goal_workflow_mode_aliases() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Task planning", "P1", &[]);

        let written = write_task_plan(
            temp.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                plan: TaskPlanFile {
                    item_id: "PROJ-001".to_string(),
                    version: 1,
                    mode: "direct_scaffold".to_string(),
                    requirements: vec![TaskPlanRequirement {
                        id: "R1".to_string(),
                        text: "Deliver the backlog item.".to_string(),
                    }],
                    design: TaskPlanDesign {
                        summary: "Implement a focused direct task.".to_string(),
                        owned_surfaces: vec!["README.md".to_string()],
                        notes: None,
                    },
                    tasks: vec![PlannedTask {
                        id: "PROJ-001-T01".to_string(),
                        title: "Implement direct task".to_string(),
                        goal: "Deliver concrete direct behavior.".to_string(),
                        requirement_refs: vec!["R1".to_string()],
                        depends_on: Vec::new(),
                        owned_surfaces: vec!["README.md".to_string()],
                        verification: vec!["make check".to_string()],
                        acceptance: vec!["Direct task is complete.".to_string()],
                        notes: None,
                    }],
                },
                overwrite: None,
            },
        );

        assert!(matches!(written.status, ActionStatus::Failed));
        assert!(written
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("expected one of: minimal, standard, full"));
        assert!(!temp.path().join("backlog/plans/PROJ-001.yaml").exists());
    }

    #[test]
    fn write_task_plan_rejects_invalid_new_plan_without_file() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Task planning", "P1", &[]);
        let mut plan = valid_task_plan("PROJ-001");
        plan.tasks.clear();

        let result = write_task_plan(
            temp.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                plan,
                overwrite: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.error.as_deref().unwrap_or("").contains("tasks"));
        assert!(!temp.path().join("backlog/plans/PROJ-001.yaml").exists());
    }

    #[test]
    fn write_task_plan_failed_overwrite_preserves_existing_file() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Task planning", "P1", &[]);
        fs::create_dir_all(temp.path().join("backlog/plans")).expect("plans dir");
        let path = temp.path().join("backlog/plans/PROJ-001.yaml");
        let original = serde_yaml::to_string(&valid_task_plan("PROJ-001")).expect("plan yaml");
        fs::write(&path, &original).expect("existing plan");
        let mut replacement = valid_task_plan("PROJ-001");
        replacement.design.summary.clear();

        let result = write_task_plan(
            temp.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                plan: replacement,
                overwrite: Some(true),
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("design.summary"));
        assert_eq!(fs::read_to_string(&path).expect("existing plan"), original);
    }

    #[test]
    fn task_plan_yaml_defaults_missing_version_to_one() {
        let parsed: crate::models::TaskPlanFile = serde_yaml::from_str(
            r#"item_id: PROJ-001
mode: standard
requirements:
  - id: R1
    text: Requirement.
design:
  summary: Design.
  owned_surfaces:
    - README.md
tasks:
  - id: PROJ-001-T01
    title: Implement slice
    goal: Exercise default version.
    requirement_refs:
      - R1
    depends_on: []
    owned_surfaces:
      - README.md
    verification:
      - make check
    acceptance:
      - Slice is complete.
"#,
        )
        .expect("plan parses");

        assert_eq!(parsed.version, 1);
    }

    #[test]
    fn write_task_plan_defaults_empty_task_surfaces_from_design() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Task planning", "P1", &[]);

        let plan: crate::models::TaskPlanFile = serde_yaml::from_str(
            r#"item_id: PROJ-001
mode: standard
requirements:
  - id: R1
    text: Requirement.
design:
  summary: Design.
  owned_surfaces:
    - README.md
tasks:
  - id: PROJ-001-T01
    title: Implement slice
    goal: Exercise default owned surfaces.
    requirement_refs:
      - R1
    depends_on: []
    verification:
      - make check
    acceptance:
      - Slice is complete.
"#,
        )
        .expect("plan parses");

        let result = write_task_plan(
            temp.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                plan,
                overwrite: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let written =
            fs::read_to_string(temp.path().join("backlog/plans/PROJ-001.yaml")).expect("plan");
        assert!(written.contains("owned_surfaces:\n  - README.md"));
    }

    #[cfg(unix)]
    #[test]
    fn write_task_plan_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Task planning", "P1", &[]);
        fs::create_dir_all(temp.path().join("backlog/plans")).expect("plans dir");
        let outside = TempDir::new().expect("outside dir");
        symlink(
            outside.path().join("PROJ-001.yaml"),
            temp.path().join("backlog/plans/PROJ-001.yaml"),
        )
        .expect("symlink");

        let plan: crate::models::TaskPlanFile = serde_yaml::from_str(
            r#"item_id: PROJ-001
version: 1
mode: standard
requirements:
  - id: R1
    text: Requirement.
design:
  summary: Design.
  owned_surfaces:
    - README.md
tasks:
  - id: PROJ-001-T01
    title: Implement slice
    goal: Exercise symlink safety.
    requirement_refs:
      - R1
    depends_on: []
    owned_surfaces:
      - README.md
    verification:
      - make check
    acceptance:
      - Slice is complete.
"#,
        )
        .expect("plan");

        let result = write_task_plan(
            temp.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                plan,
                overwrite: Some(true),
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.error.unwrap().contains("is a symlink"));
    }

    #[test]
    fn validates_task_plan_semantics_and_rejects_unknown_yaml_fields() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Task planning", "P1", &[]);
        fs::create_dir_all(temp.path().join("backlog/plans")).expect("plans dir");
        fs::write(
            temp.path().join("backlog/plans/PROJ-001.yaml"),
            r#"item_id: PROJ-001
version: 1
mode: standard
status: done
requirements:
  - id: R1
    text: Requirement.
design:
  summary: Design.
tasks:
  - id: PROJ-001-T02
    title: Invalid dependency
    goal: Exercise validation.
    requirement_refs:
      - R2
    depends_on:
      - PROJ-001-T99
    owned_surfaces: []
    verification: []
    acceptance: []
"#,
        )
        .expect("plan");

        let validation = validate_task_plan(
            temp.path(),
            TaskPlanQueryParams {
                root: Some(root_arg(temp.path())),
                item_id: Some("PROJ-001".to_string()),
                include_errors: Some(true),
            },
        );
        assert!(matches!(validation.status, ActionStatus::Failed));
        let error = validation.error.unwrap();
        assert!(
            error.contains("unknown field `status`"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn validates_task_plan_dependencies_requirements_and_required_fields() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Task planning", "P1", &[]);
        fs::create_dir_all(temp.path().join("backlog/plans")).expect("plans dir");
        fs::write(
            temp.path().join("backlog/plans/PROJ-001.yaml"),
            r#"item_id: PROJ-001
version: 1
mode: standard
requirements:
  - id: R1
    text: Requirement.
design:
  summary: Design.
tasks:
  - id: PROJ-001-T02
    title: Invalid dependency
    goal: Exercise validation.
    requirement_refs:
      - R2
    depends_on:
      - PROJ-001-T99
    owned_surfaces: []
    verification: []
    acceptance: []
"#,
        )
        .expect("plan");

        let validation = validate_task_plan(
            temp.path(),
            TaskPlanQueryParams {
                root: Some(root_arg(temp.path())),
                item_id: Some("PROJ-001".to_string()),
                include_errors: Some(true),
            },
        );
        assert!(matches!(validation.status, ActionStatus::Failed));
        let error = validation.error.unwrap();
        assert!(error.contains("unknown requirement_ref `R2`"));
        assert!(error.contains("unknown dependency `PROJ-001-T99`"));
        assert!(error.contains("owned_surfaces must not be empty"));
        assert!(
            !error.contains("verification must not be empty"),
            "verification should be optional, got: {error}"
        );
        assert!(
            !error.contains("acceptance must not be empty"),
            "acceptance should be optional, got: {error}"
        );
    }

    #[test]
    fn validates_task_plan_rejects_circular_dependencies() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Task planning", "P1", &[]);
        fs::create_dir_all(temp.path().join("backlog/plans")).expect("plans dir");
        fs::write(
            temp.path().join("backlog/plans/PROJ-001.yaml"),
            r#"item_id: PROJ-001
version: 1
mode: standard
requirements:
  - id: R1
    text: Requirement.
design:
  summary: Design.
  owned_surfaces:
    - README.md
tasks:
  - id: PROJ-001-T01
    title: First
    goal: First task.
    requirement_refs:
      - R1
    depends_on:
      - PROJ-001-T02
    owned_surfaces:
      - README.md
    verification:
      - make check
    acceptance:
      - First done.
  - id: PROJ-001-T02
    title: Second
    goal: Second task.
    requirement_refs:
      - R1
    depends_on:
      - PROJ-001-T01
    owned_surfaces:
      - README.md
    verification:
      - make check
    acceptance:
      - Second done.
"#,
        )
        .expect("plan");

        let validation = validate_task_plan(
            temp.path(),
            TaskPlanQueryParams {
                root: Some(root_arg(temp.path())),
                item_id: Some("PROJ-001".to_string()),
                include_errors: Some(true),
            },
        );
        assert!(matches!(validation.status, ActionStatus::Failed));
        let error = validation.error.unwrap();
        assert!(error.contains("circular task dependency"), "{error}");
    }

    #[test]
    fn update_backlog_item_updates_fields_sections_and_validates() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Old title", "P1", &[]);

        let result = update_backlog_item(
            temp.path(),
            UpdateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                item_id: "proj-001".to_string(),
                title: Some("New title".to_string()),
                priority: Some("P0".to_string()),
                item_type: Some("docs".to_string()),
                area: Some("documentation".to_string()),
                epic: None,
                depends_on: Some(Vec::new()),
                owned_surfaces: Some(vec!["README.md".to_string()]),
                external_refs: None,
                execution_path: Some("worker_handoff".to_string()),
                planning_gate: Some("task_plan".to_string()),
                goal: Some("Update the backlog item safely.".to_string()),
                implementation_contract: Some("Patch the item through the MCP tool.".to_string()),
                contract: None,
                acceptance: Some(vec!["Updated item validates.".to_string()]),
                notes: Some("Reviewed from feedback.".to_string()),
                force_closed: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let data = result.data.expect("data");
        assert_eq!(data.item_id, "PROJ-001");
        assert!(data.changed_fields.contains(&"title".to_string()));
        assert!(data.changed_fields.contains(&"acceptance".to_string()));
        let text = fs::read_to_string(temp.path().join("backlog/items/PROJ-001.md")).unwrap();
        assert!(text.contains("title: New title"));
        assert!(text.contains("priority: P0"));
        assert!(text.contains("type: docs"));
        assert!(text.contains("execution_path: worker_handoff"));
        assert!(text.contains("planning_gate: task_plan"));
        assert!(text.contains("## Notes\n\nReviewed from feedback."));
        assert!(text.contains("- Updated item validates."));
    }

    #[test]
    fn update_backlog_item_rejects_invalid_update_and_rolls_back() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Stable title", "P1", &[]);
        let path = temp.path().join("backlog/items/PROJ-001.md");
        let before = fs::read_to_string(&path).expect("before");

        let result = update_backlog_item(
            temp.path(),
            UpdateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                title: Some("Changed title".to_string()),
                priority: Some("P9".to_string()),
                item_type: None,
                area: None,
                epic: None,
                depends_on: None,
                owned_surfaces: None,
                external_refs: None,
                execution_path: None,
                planning_gate: None,
                goal: None,
                implementation_contract: None,
                contract: None,
                acceptance: None,
                notes: None,
                force_closed: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("invalid priority"));
        assert_eq!(fs::read_to_string(path).expect("after"), before);
    }

    #[test]
    fn update_backlog_item_rolls_back_unknown_dependency() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Stable title", "P1", &[]);
        let path = temp.path().join("backlog/items/PROJ-001.md");
        let before = fs::read_to_string(&path).expect("before");

        let result = update_backlog_item(
            temp.path(),
            UpdateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                title: Some("Changed title".to_string()),
                priority: None,
                item_type: None,
                area: None,
                epic: None,
                depends_on: Some(vec!["PROJ-999".to_string()]),
                owned_surfaces: None,
                external_refs: None,
                execution_path: None,
                planning_gate: None,
                goal: None,
                implementation_contract: None,
                contract: None,
                acceptance: None,
                notes: None,
                force_closed: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("unknown dependency `PROJ-999`"));
        assert_eq!(fs::read_to_string(path).expect("after"), before);
    }

    #[test]
    fn update_backlog_item_protects_closed_items_unless_forced() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Closed title", "P1", &[]);
        git(temp.path(), &["init"]);
        git(temp.path(), &["config", "user.name", "Platypus Test"]);
        git(
            temp.path(),
            &["config", "user.email", "platypus@example.invalid"],
        );
        git(temp.path(), &["add", "--all"]);
        git(
            temp.path(),
            &[
                "commit",
                "-m",
                "Close item",
                "-m",
                "Platypus-Closes: PROJ-001",
            ],
        );

        let blocked = update_backlog_item(
            temp.path(),
            UpdateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                title: Some("Blocked update".to_string()),
                priority: None,
                item_type: None,
                area: None,
                epic: None,
                depends_on: None,
                owned_surfaces: None,
                external_refs: None,
                execution_path: None,
                planning_gate: None,
                goal: None,
                implementation_contract: None,
                contract: None,
                acceptance: None,
                notes: None,
                force_closed: None,
            },
        );
        assert!(matches!(blocked.status, ActionStatus::Failed));
        assert!(blocked
            .error
            .as_deref()
            .unwrap_or("")
            .contains("already closed"));

        let forced = update_backlog_item(
            temp.path(),
            UpdateBacklogItemParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                title: Some("Forced update".to_string()),
                priority: None,
                item_type: None,
                area: None,
                epic: None,
                depends_on: None,
                owned_surfaces: None,
                external_refs: None,
                execution_path: None,
                planning_gate: None,
                goal: None,
                implementation_contract: None,
                contract: None,
                acceptance: None,
                notes: None,
                force_closed: Some(true),
            },
        );
        assert!(matches!(forced.status, ActionStatus::Completed));
        assert!(forced.data.unwrap().closed);
    }

    fn project_fixture() -> TempDir {
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir_all(temp.path().join("backlog/items")).expect("items dir");
        fs::create_dir_all(temp.path().join("backlog/epics")).expect("epics dir");
        fs::write(temp.path().join("platy.yaml"), "project: test\n").expect("config");
        fs::write(
            temp.path().join("backlog/epics/general.md"),
            "---\nid: general\ntitle: General\nstatus: active\npriority: P1\narea: general\n---\n\n# General\n",
        )
        .expect("epic");
        temp
    }

    fn write_item(root: &Path, id: &str, title: &str, priority: &str, depends_on: &[&str]) {
        let depends = if depends_on.is_empty() {
            "[]".to_string()
        } else {
            format!(
                "\n{}",
                depends_on
                    .iter()
                    .map(|dependency| format!("  - {}", dependency))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        let text = format!(
            "---\nid: {id}\ntitle: {title}\npriority: {priority}\ntype: feature\narea: tooling\nepic: general\ndepends_on: {depends}\nowned_surfaces: []\n---\n\n# {id} {title}\n\n## Goal\n\nGoal.\n\n## Implementation Contract\n\nContract.\n\n## Acceptance\n\n- Done.\n"
        );
        fs::write(root.join(format!("backlog/items/{id}.md")), text).expect("item");
    }

    fn set_item_policy(root: &Path, id: &str, execution_path: &str, planning_gate: &str) {
        let path = root.join(format!("backlog/items/{id}.md"));
        let text = fs::read_to_string(&path).expect("item");
        let text = text.replacen(
            "owned_surfaces: []",
            &format!(
                "owned_surfaces: []\nexecution_path: {execution_path}\nplanning_gate: {planning_gate}"
            ),
            1,
        );
        fs::write(path, text).expect("item");
    }

    fn batch_entry(
        client_key: &str,
        id: Option<&str>,
        title: &str,
        depends_on_keys: Vec<String>,
    ) -> CreateBacklogItemsEntry {
        CreateBacklogItemsEntry {
            client_key: Some(client_key.to_string()),
            depends_on_keys,
            id: id.map(ToOwned::to_owned),
            id_prefix: None,
            title: title.to_string(),
            priority: Some("P1".to_string()),
            item_type: Some("feature".to_string()),
            area: Some("general".to_string()),
            epic: Some("general".to_string()),
            depends_on: Vec::new(),
            owned_surfaces: Vec::new(),
            external_refs: Vec::new(),
            execution_path: None,
            planning_gate: None,
            goal: format!("{title}."),
            implementation_contract: Some(format!("Implement {title}.")),
            contract: None,
            acceptance: vec![format!("{title} is complete.")],
            notes: None,
        }
    }

    fn valid_task_plan(item_id: &str) -> TaskPlanFile {
        TaskPlanFile {
            item_id: item_id.to_string(),
            version: 1,
            mode: "standard".to_string(),
            requirements: vec![TaskPlanRequirement {
                id: "R1".to_string(),
                text: "Deliver the backlog item.".to_string(),
            }],
            design: TaskPlanDesign {
                summary: "Implement a focused task.".to_string(),
                owned_surfaces: vec!["README.md".to_string()],
                notes: None,
            },
            tasks: vec![PlannedTask {
                id: format!("{item_id}-T01"),
                title: "Implement task".to_string(),
                goal: "Deliver concrete behavior.".to_string(),
                requirement_refs: vec!["R1".to_string()],
                depends_on: Vec::new(),
                owned_surfaces: vec!["README.md".to_string()],
                verification: vec!["make check".to_string()],
                acceptance: vec!["Task is complete.".to_string()],
                notes: None,
            }],
        }
    }

    fn root_arg(root: &Path) -> String {
        root.to_string_lossy().into_owned()
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
}
