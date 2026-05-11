mod closure;
mod create;
mod draft;
mod epic;
mod filesystem;
mod parse;
mod plan;
mod status;
mod types;
mod validate;

pub use create::create_backlog_item;
pub use draft::{
    draft_backlog_items, draft_backlog_items_from_sample, draft_backlog_items_sampling_prompt,
};
pub use epic::{create_epic, list_epics};
pub use plan::{
    draft_task_plan, draft_task_plan_from_sample, draft_task_plan_sampling_prompt,
    inspect_task_plan, list_task_plans, validate_task_plan, write_task_plan,
};
pub use status::{inspect_backlog_inventory, inspect_status, list_backlog};
pub use validate::validate_backlog;

pub(crate) use closure::closed_item_ids;

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
            external_refs: item.frontmatter.external_refs.clone(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ActionStatus, CreateBacklogItemParams, CreateEpicParams, DraftBacklogItemsParams,
        PlannedTask, RootParams, TaskPlanDesign, TaskPlanFile, TaskPlanItemParams,
        TaskPlanQueryParams, TaskPlanRequirement, WriteTaskPlanParams,
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
                suggested_worker: Some("coder".to_string()),
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
                suggested_worker: None,
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
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
            "## Implementation Contract\n\nImplement the requested change: Scaffold frontend."
        ));
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
                suggested_worker: None,
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
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
            "## Implementation Contract\n\nImplement the requested change: Document recovery flow."
        ));

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
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
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
        assert!(missing_from_omitted_error.contains("title"));
        assert!(missing_from_omitted_error.contains("goal"));
        assert!(missing_from_omitted_error.contains("implementation_contract|contract"));
        assert!(missing_from_omitted_error.contains("acceptance"));

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
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
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
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
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
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
                goal: String::new(),
                implementation_contract: None,
                contract: None,
                acceptance: Vec::new(),
                notes: None,
            },
        );
        let missing_error = missing.error.unwrap();
        assert!(missing_error.contains("title"));
        assert!(missing_error.contains("goal"));
        assert!(missing_error.contains("implementation_contract|contract"));
        assert!(missing_error.contains("acceptance"));
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
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
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
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
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
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
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
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
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
                suggested_worker: Some("coder".to_string()),
                owned_surfaces: Vec::new(),
                external_refs: Vec::new(),
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

    #[test]
    fn draft_backlog_items_skips_without_sampling() {
        let result = draft_backlog_items(DraftBacklogItemsParams {
            goal: "Build MCP tools".to_string(),
            suggested_worker: Some("coder".to_string()),
            owned_surfaces: vec!["src".to_string()],
            verification_command: vec!["cargo".to_string(), "test".to_string()],
        });

        assert!(matches!(result.status, ActionStatus::Skipped));
        let drafts = result.data.unwrap().drafts;
        assert!(drafts.is_empty());
    }

    #[test]
    fn sampled_task_plan_writes_and_validates_strict_task_plan() {
        let temp = project_fixture();
        write_item(temp.path(), "PROJ-001", "Task planning", "P1", &[]);

        let drafted = draft_task_plan(
            temp.path(),
            crate::models::DraftTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
            },
        );
        assert!(matches!(drafted.status, ActionStatus::Completed));
        let starter = drafted.data.expect("starter plan").plan;
        assert_eq!(starter.item_id, "PROJ-001");
        assert_eq!(starter.mode, "standard");
        assert_eq!(starter.tasks[0].id, "PROJ-001-T001");
        assert!(starter.tasks[0].verification.is_empty());

        let sampled = draft_task_plan_from_sample(
            temp.path(),
            &crate::models::DraftTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
            },
            r#"{
              "plan": {
                "item_id": "PROJ-001",
                "version": 1,
                "mode": "standard",
                "requirements": [
                  { "id": "R1", "text": "Deliver the backlog item." }
                ],
                "design": {
                  "summary": "Implement a focused task planning slice.",
                  "owned_surfaces": ["README.md"],
                  "notes": null
                },
                "tasks": [
                  {
                    "id": "PROJ-001-T01",
                    "title": "Implement task planning",
                    "goal": "Deliver concrete task planning behavior.",
                    "requirement_refs": ["R1"],
                    "depends_on": [],
                    "owned_surfaces": ["README.md"],
                    "suggested_worker": "coder",
                    "verification": ["make check"],
                    "acceptance": ["Task planning behavior is concrete."],
                    "notes": null
                  }
                ]
              }
            }"#,
        );
        assert!(matches!(sampled.status, ActionStatus::Completed));
        let plan = sampled.data.unwrap().plan;
        assert_eq!(plan.tasks[0].id, "PROJ-001-T01");

        let written = write_task_plan(
            temp.path(),
            WriteTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
                plan,
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
    }

    #[test]
    fn write_task_plan_accepts_goal_mode_aliases_and_stores_canonical_mode() {
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
                        suggested_worker: Some("coder".to_string()),
                        verification: vec!["make check".to_string()],
                        acceptance: vec!["Direct task is complete.".to_string()],
                        notes: None,
                    }],
                },
                overwrite: None,
            },
        );

        assert!(matches!(written.status, ActionStatus::Completed));
        let inspected = inspect_task_plan(
            temp.path(),
            TaskPlanItemParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
            },
        );
        assert_eq!(inspected.data.expect("plan").plan.mode, "direct");
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
    suggested_worker: coder
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
    suggested_worker: coder
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

    #[test]
    fn sampled_backlog_items_reject_generic_fastapi_react_templates() {
        let params = DraftBacklogItemsParams {
            goal: "Build a simple FastAPI + React web app".to_string(),
            suggested_worker: None,
            owned_surfaces: Vec::new(),
            verification_command: Vec::new(),
        };
        let result = draft_backlog_items_from_sample(
            &params,
            r#"[
              {
                "candidate_id": "draft-1",
                "title": "Shape Build a simple FastAPI + React web app",
                "objective": "Clarify scope.",
                "type": "foundation",
                "area": "planning",
                "owned_surfaces": [],
                "suggested_worker": "coder",
                "verification_command": []
              }
            ]"#,
        );

        assert!(matches!(result.status, ActionStatus::Failed));
        assert!(result.error.unwrap().contains("generic placeholder"));
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
    suggested_worker: coder
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
    suggested_worker: coder
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
    suggested_worker: coder
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
            "---\nid: {id}\ntitle: {title}\npriority: {priority}\ntype: feature\narea: tooling\nepic: general\ndepends_on: {depends}\nsuggested_worker: coder\nowned_surfaces: []\n---\n\n# {id} {title}\n\n## Goal\n\nGoal.\n\n## Implementation Contract\n\nContract.\n\n## Acceptance\n\n- Done.\n"
        );
        fs::write(root.join(format!("backlog/items/{id}.md")), text).expect("item");
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
