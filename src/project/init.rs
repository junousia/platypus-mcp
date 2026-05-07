use super::paths::{checked_relative_path, resolve_root};
use crate::models::{
    ActionResult, InitProjectParams, ProjectScaffoldData, ScaffoldEntry, ScaffoldEntryStatus,
};
use std::{fs, path::Path};

pub fn init_project(
    default_root: &Path,
    params: InitProjectParams,
) -> ActionResult<ProjectScaffoldData> {
    let action = "init_project";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(action, "Could not initialize Platypus project.", error)
        }
    };
    let project_name = params
        .project_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            root.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("project")
        });
    let overwrite = params.overwrite.unwrap_or(false);
    let mut entries = Vec::new();

    for directory in [
        "backlog",
        "backlog/items",
        "backlog/epics",
        "backlog/templates",
    ] {
        if let Err(error) = ensure_directory(&root, directory, &mut entries) {
            return ActionResult::failed(action, "Could not initialize Platypus project.", error);
        }
    }

    for (relative, content) in scaffold_files(project_name) {
        if let Err(error) = write_file(&root, relative, &content, overwrite, &mut entries) {
            return ActionResult::failed(action, "Could not initialize Platypus project.", error);
        }
    }

    let created = entries
        .iter()
        .filter(|entry| matches!(entry.status, ScaffoldEntryStatus::Created))
        .count();
    let skipped = entries.len() - created;
    ActionResult::completed(
        action,
        format!(
            "Platypus project scaffold ready: {} created, {} skipped.",
            created, skipped
        ),
        ProjectScaffoldData {
            root: root.display().to_string(),
            created,
            skipped,
            entries,
        },
    )
}

fn ensure_directory(
    root: &Path,
    relative: &str,
    entries: &mut Vec<ScaffoldEntry>,
) -> std::result::Result<(), String> {
    let relative_path = checked_relative_path(relative)?;
    let path = root.join(relative_path);
    if path.exists() {
        if !path.is_dir() {
            return Err(format!("{} exists but is not a directory", path.display()));
        }
        entries.push(skipped_entry(root, &path, "directory"));
        return Ok(());
    }
    fs::create_dir(&path).map_err(|error| format!("{}: {}", path.display(), error))?;
    entries.push(created_entry(root, &path, "directory"));
    Ok(())
}

fn write_file(
    root: &Path,
    relative: &str,
    content: &str,
    overwrite: bool,
    entries: &mut Vec<ScaffoldEntry>,
) -> std::result::Result<(), String> {
    let relative_path = checked_relative_path(relative)?;
    let path = root.join(relative_path);
    if path.exists() && !overwrite {
        if !path.is_file() {
            return Err(format!("{} exists but is not a file", path.display()));
        }
        entries.push(skipped_entry(root, &path, "file"));
        return Ok(());
    }
    fs::write(&path, content).map_err(|error| format!("{}: {}", path.display(), error))?;
    entries.push(created_entry(root, &path, "file"));
    Ok(())
}

fn created_entry(root: &Path, path: &Path, kind: &str) -> ScaffoldEntry {
    scaffold_entry(root, path, kind, ScaffoldEntryStatus::Created)
}

fn skipped_entry(root: &Path, path: &Path, kind: &str) -> ScaffoldEntry {
    scaffold_entry(root, path, kind, ScaffoldEntryStatus::Skipped)
}

fn scaffold_entry(
    root: &Path,
    path: &Path,
    kind: &str,
    status: ScaffoldEntryStatus,
) -> ScaffoldEntry {
    let relative = path.strip_prefix(root).unwrap_or(path);
    ScaffoldEntry {
        path: relative.display().to_string(),
        kind: kind.to_string(),
        status,
    }
}

fn scaffold_files(project_name: &str) -> Vec<(&'static str, String)> {
    vec![
        ("platy.yaml", project_config(project_name)),
        ("WORKFLOW.md", workflow_doc()),
        ("backlog/README.md", backlog_readme()),
        ("backlog/epics/general.md", general_epic()),
        ("backlog/templates/item.md", item_template()),
        ("backlog/templates/epic.md", epic_template()),
    ]
}

fn project_config(project_name: &str) -> String {
    format!(
        "project:\n  name: {}\nbacklog:\n  id_prefix: PROJ\n  items: backlog/items\n  epics: backlog/epics\nworkflow:\n  integration:\n    merge_style: merge_commit\n    require_clean_manager_workspace: true\n    require_verification_evidence: true\n",
        yaml_string(project_name)
    )
}

fn workflow_doc() -> String {
    "# Workflow\n\nUse Platypus MCP tools to inspect, shape, validate, and execute project work.\n"
        .to_string()
}

fn backlog_readme() -> String {
    "# Backlog\n\nStructured Platypus backlog items live in `backlog/items/`.\nUse `list_backlog` and `next_safe_action` to compute the current queue from item metadata and Git closure trailers.\n".to_string()
}

fn general_epic() -> String {
    "---\nid: general\ntitle: General\nstatus: active\npriority: P1\narea: general\n---\n\n# General\n\nDefault project epic.\n".to_string()
}

fn item_template() -> String {
    "---\nid: PROJ-000\ntitle: Item title\npriority: P1\ntype: feature\narea: general\nepic: general\ndepends_on: []\nsuggested_worker: coder\nowned_surfaces: []\n---\n\n# PROJ-000 Item title\n\n## Goal\n\nDescribe the goal.\n\n## Implementation Contract\n\nDescribe the expected implementation boundaries.\n\n## Acceptance\n\n- Describe a verifiable acceptance criterion.\n".to_string()
}

fn epic_template() -> String {
    "---\nid: epic-id\ntitle: Epic title\nstatus: active\npriority: P1\narea: general\n---\n\n# Epic title\n\nDescribe the epic.\n".to_string()
}

fn yaml_string(value: &str) -> String {
    serde_yaml::to_string(value)
        .unwrap_or_else(|_| "\"project\"".to_string())
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ActionStatus;
    use tempfile::TempDir;

    #[test]
    fn init_project_creates_missing_scaffold() {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path().to_string_lossy().into_owned();

        let result = init_project(
            temp.path(),
            InitProjectParams {
                root: Some(root),
                project_name: Some("Example".to_string()),
                overwrite: None,
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let data = result.data.expect("data");
        assert_eq!(data.skipped, 0);
        assert!(temp.path().join("platy.yaml").is_file());
        let config = fs::read_to_string(temp.path().join("platy.yaml")).expect("config");
        assert!(config.contains("workflow:"));
        assert!(config.contains("merge_style: merge_commit"));
        assert!(temp.path().join("backlog/items").is_dir());
        assert!(!temp.path().join("backlog/index.md").exists());
        assert!(temp.path().join("backlog/epics/general.md").is_file());
    }

    #[test]
    fn init_project_skips_existing_files_without_overwrite() {
        let temp = TempDir::new().expect("temp dir");
        fs::write(temp.path().join("platy.yaml"), "custom: true\n").expect("config");
        let root = temp.path().to_string_lossy().into_owned();

        let result = init_project(
            temp.path(),
            InitProjectParams {
                root: Some(root),
                project_name: None,
                overwrite: Some(false),
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        assert_eq!(
            fs::read_to_string(temp.path().join("platy.yaml")).expect("config"),
            "custom: true\n"
        );
        assert!(result.data.expect("data").skipped > 0);
    }
}
