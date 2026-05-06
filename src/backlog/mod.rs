mod closure;
mod create;
mod draft;
mod filesystem;
mod parse;
mod status;
mod types;
mod validate;

pub use create::create_backlog_item;
pub use draft::draft_backlog_items;
pub use status::{inspect_status, list_backlog};
pub use validate::validate_backlog;

pub(crate) use closure::closed_item_ids;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ActionStatus, CreateBacklogItemParams, DraftBacklogItemsParams};
    use std::{fs, path::Path};
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
}
