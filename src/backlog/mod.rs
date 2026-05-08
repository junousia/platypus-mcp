mod closure;
mod create;
mod draft;
mod filesystem;
mod parse;
mod plan;
mod status;
mod types;
mod validate;

pub use create::create_backlog_item;
pub use draft::draft_backlog_items;
pub use plan::{
    draft_task_plan, inspect_task_plan, list_task_plans, validate_task_plan, write_task_plan,
};
pub use status::{inspect_status, list_backlog};
pub use validate::validate_backlog;

pub(crate) use closure::closed_item_ids;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ActionStatus, CreateBacklogItemParams, DraftBacklogItemsParams, TaskPlanQueryParams,
        WriteTaskPlanParams,
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
    fn draft_backlog_items_returns_three_candidates() {
        let result = draft_backlog_items(DraftBacklogItemsParams {
            goal: "Build MCP tools".to_string(),
            suggested_worker: Some("coder".to_string()),
            owned_surfaces: vec!["src".to_string()],
            verification_command: vec!["cargo".to_string(), "test".to_string()],
        });

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(result.data.unwrap().drafts.len(), 3);
    }

    #[test]
    fn drafts_writes_and_validates_strict_task_plan() {
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
        let plan = drafted.data.unwrap().plan;
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

        let plan = draft_task_plan(
            temp.path(),
            crate::models::DraftTaskPlanParams {
                root: Some(root_arg(temp.path())),
                item_id: "PROJ-001".to_string(),
            },
        )
        .data
        .expect("draft")
        .plan;

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
        assert!(error.contains("verification must not be empty"));
        assert!(error.contains("acceptance must not be empty"));
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
