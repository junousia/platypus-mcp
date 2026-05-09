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
        "backlog/plans",
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
    if let Err(error) = ensure_gitignore(&root, &mut entries) {
        return ActionResult::failed(action, "Could not initialize Platypus project.", error);
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
        ("AGENTS.md", agents_doc()),
        ("CLAUDE.md", claude_doc()),
        ("platy.yaml", project_config(project_name)),
        ("WORKFLOW.md", workflow_doc()),
        ("backlog/README.md", backlog_readme()),
        ("backlog/epics/general.md", general_epic()),
        ("backlog/templates/item.md", item_template()),
        ("backlog/templates/plan.yaml", plan_template()),
        ("backlog/templates/epic.md", epic_template()),
    ]
}

fn ensure_gitignore(
    root: &Path,
    entries: &mut Vec<ScaffoldEntry>,
) -> std::result::Result<(), String> {
    let relative_path = checked_relative_path(".gitignore")?;
    let path = root.join(relative_path);
    let rule = ".platy/";
    if !path.exists() {
        fs::write(&path, format!("# Platypus runtime state\n{rule}\n"))
            .map_err(|error| format!("{}: {}", path.display(), error))?;
        entries.push(created_entry(root, &path, "file"));
        return Ok(());
    }
    if !path.is_file() {
        return Err(format!("{} exists but is not a file", path.display()));
    }
    let content =
        fs::read_to_string(&path).map_err(|error| format!("{}: {}", path.display(), error))?;
    if gitignore_has_rule(&content, rule) {
        entries.push(skipped_entry(root, &path, "file"));
        return Ok(());
    }
    let mut updated = content;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.trim_end().is_empty() {
        updated.push('\n');
    }
    updated.push_str("# Platypus runtime state\n");
    updated.push_str(rule);
    updated.push('\n');
    fs::write(&path, updated).map_err(|error| format!("{}: {}", path.display(), error))?;
    entries.push(created_entry(root, &path, "file"));
    Ok(())
}

fn gitignore_has_rule(content: &str, rule: &str) -> bool {
    content.lines().any(|line| line.trim() == rule)
}

fn project_config(project_name: &str) -> String {
    format!(
        "project:\n  name: {}\nbacklog:\n  id_prefix: PROJ\n  items: backlog/items\n  epics: backlog/epics\nworkflow:\n  integration:\n    merge_style: merge_commit\n    require_clean_manager_workspace: true\n    require_verification_evidence: true\n",
        yaml_string(project_name)
    )
}

fn workflow_doc() -> String {
    r#"# Workflow

This project uses Platypus MCP for spec-driven development. Free-form goals
should become structured backlog items, non-trivial backlog items should become
reviewable task plans, and implementation should run through isolated task
worktrees before integration.

## Operating Loop

1. Inspect setup with `doctor_snapshot`, `inspect_status`, and
   `next_safe_action`.
2. Turn goals into declarative backlog items with `draft_backlog_items`,
   `create_backlog_item`, and `validate_backlog`.
3. Inspect the queue with `inspect_work_queue` and
   `classify_planning_needs`.
4. For standard or full work, create a strict task plan with
   `draft_task_plan`, `write_task_plan`, and `validate_task_plan`.
5. Dispatch and prepare work with `dispatch_next_work` and
   `prepare_worker_handoff`.
6. Run implementation in the assigned worktree, not in the manager workspace.
7. Record progress, verification evidence, findings, and final result before
   integrating.
8. Use `integrate_worker_result` and `reconcile_project` to close the loop.

## State Rules

- Backlog items describe intent, constraints, dependencies, owned surfaces, and
  acceptance criteria.
- Task plans describe requirements, design, and executable task slices.
- Runtime state belongs in Platypus state, task events, evidence, findings, and
  Git trailers, not in backlog markdown or task-plan YAML.
- When uncertain, inspect before mutating and return structured recovery
  guidance to the user.
"#
    .to_string()
}

fn backlog_readme() -> String {
    r#"# Backlog

Structured Platypus backlog items live in `backlog/items/`.
Reviewable task plans for non-trivial items live in `backlog/plans/`.

Use the Platypus MCP tools to keep planning reproducible:

- `draft_backlog_items` turns a product goal into candidate work.
- `create_backlog_item` writes accepted backlog items.
- `validate_backlog` checks item and epic schema.
- `inspect_work_queue` shows runnable items and task-plan requirements.
- `draft_task_plan`, `write_task_plan`, and `validate_task_plan` make
  non-trivial work executable.
- `next_safe_action` computes the next lifecycle step from current state.

Do not manually maintain queue indexes or runtime status in markdown. Queue
state is computed from backlog metadata, task-plan readiness, Platypus runtime
state, and Git closure trailers.
"#
    .to_string()
}

fn agents_doc() -> String {
    agent_guidance_doc("Agent Instructions")
}

fn claude_doc() -> String {
    agent_guidance_doc("Claude Instructions")
}

fn agent_guidance_doc(title: &str) -> String {
    format!(
        r#"# {title}

This repository uses Platypus MCP as its spec-driven development control
surface. When the MCP server is available, prefer Platypus tools over ad hoc
file edits or informal task tracking.

## Default Flow

1. Inspect first: `doctor_snapshot`, `inspect_status`, and `next_safe_action`.
2. Convert user goals into backlog candidates with `draft_backlog_items`.
3. Persist approved work with `create_backlog_item` and validate with
   `validate_backlog`.
4. Use `inspect_work_queue` and `classify_planning_needs` before dispatch.
5. For standard or full work, create and validate `backlog/plans/<ITEM>.yaml`
   with `draft_task_plan`, `write_task_plan`, and `validate_task_plan`.
6. Dispatch through `dispatch_next_work` and prepare worker context with
   `prepare_worker_handoff`.
7. Implement inside the assigned worktree, record progress and evidence, then
   integrate through Platypus.

## Rules

- Keep backlog items declarative; do not add runtime status, task attempts, PR
  metadata, or closure fields.
- Keep task plans focused on requirements, design, and executable task slices;
  do not store implementation diary or completion state in plan YAML.
- Use `record_finding` for limitations and required follow-up work.
- Use `record_verification_evidence` before claiming verified completion.
- Use `reconcile_project` when state is unclear.
"#
    )
}

fn general_epic() -> String {
    "---\nid: general\ntitle: General\nstatus: active\npriority: P1\narea: general\n---\n\n# General\n\nDefault project epic.\n".to_string()
}

fn item_template() -> String {
    "---\nid: PROJ-000\ntitle: Item title\npriority: P1\ntype: feature\narea: general\nepic: general\ndepends_on: []\nsuggested_worker: coder\nowned_surfaces: []\n---\n\n# PROJ-000 Item title\n\n## Goal\n\nDescribe the goal.\n\n## Implementation Contract\n\nDescribe the expected implementation boundaries.\n\n## Acceptance\n\n- Describe a verifiable acceptance criterion.\n".to_string()
}

fn plan_template() -> String {
    "item_id: PROJ-000\nversion: 1\nmode: standard\nrequirements:\n  - id: R1\n    text: Describe a required outcome.\ndesign:\n  summary: Describe the implementation approach.\n  owned_surfaces:\n    - src/example.rs\n  notes: null\ntasks:\n  - id: PROJ-000-T01\n    title: Implement the first task\n    goal: Deliver one executable implementation slice.\n    requirement_refs:\n      - R1\n    depends_on: []\n    owned_surfaces:\n      - src/example.rs\n    suggested_worker: coder\n    verification:\n      - make check\n    acceptance:\n      - The task is complete and verified.\n    notes: null\n".to_string()
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
        assert!(temp.path().join("AGENTS.md").is_file());
        assert!(temp.path().join("CLAUDE.md").is_file());
        let gitignore = fs::read_to_string(temp.path().join(".gitignore")).expect("gitignore");
        assert!(gitignore.contains(".platy/"));
        let config = fs::read_to_string(temp.path().join("platy.yaml")).expect("config");
        assert!(config.contains("workflow:"));
        assert!(config.contains("merge_style: merge_commit"));
        let agents = fs::read_to_string(temp.path().join("AGENTS.md")).expect("agents");
        assert!(agents.contains("spec-driven development"));
        assert!(agents.contains("next_safe_action"));
        assert!(agents.contains("draft_task_plan"));
        let claude = fs::read_to_string(temp.path().join("CLAUDE.md")).expect("claude");
        assert!(claude.contains("spec-driven development"));
        assert!(claude.contains("next_safe_action"));
        assert!(claude.contains("draft_task_plan"));
        assert_eq!(
            agents.replace("Agent Instructions", "Shared Instructions"),
            claude.replace("Claude Instructions", "Shared Instructions")
        );
        let workflow = fs::read_to_string(temp.path().join("WORKFLOW.md")).expect("workflow");
        assert!(workflow.contains("spec-driven development"));
        assert!(workflow.contains("dispatch_next_work"));
        let backlog_readme =
            fs::read_to_string(temp.path().join("backlog/README.md")).expect("backlog readme");
        assert!(backlog_readme.contains("inspect_work_queue"));
        assert!(backlog_readme.contains("task-plan requirements"));
        assert!(temp.path().join("backlog/items").is_dir());
        assert!(temp.path().join("backlog/plans").is_dir());
        assert!(!temp.path().join("backlog/index.md").exists());
        assert!(temp.path().join("backlog/epics/general.md").is_file());
        assert!(temp.path().join("backlog/templates/plan.yaml").is_file());
    }

    #[test]
    fn init_project_skips_existing_files_without_overwrite() {
        let temp = TempDir::new().expect("temp dir");
        fs::write(temp.path().join("platy.yaml"), "custom: true\n").expect("config");
        fs::write(temp.path().join("CLAUDE.md"), "# Custom Claude\n").expect("claude");
        fs::write(temp.path().join(".gitignore"), "custom-ignore\n").expect("gitignore");
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
        assert_eq!(
            fs::read_to_string(temp.path().join("CLAUDE.md")).expect("claude"),
            "# Custom Claude\n"
        );
        let gitignore = fs::read_to_string(temp.path().join(".gitignore")).expect("gitignore");
        assert!(gitignore.contains("custom-ignore"));
        assert!(gitignore.contains(".platy/"));
        assert!(result.data.expect("data").skipped > 0);
    }

    #[test]
    fn init_project_merges_gitignore_even_with_overwrite() {
        let temp = TempDir::new().expect("temp dir");
        fs::write(temp.path().join(".gitignore"), "target/\n.env\n").expect("gitignore");
        let root = temp.path().to_string_lossy().into_owned();

        let result = init_project(
            temp.path(),
            InitProjectParams {
                root: Some(root),
                project_name: None,
                overwrite: Some(true),
            },
        );

        assert!(matches!(result.status, ActionStatus::Completed));
        let gitignore = fs::read_to_string(temp.path().join(".gitignore")).expect("gitignore");
        assert!(gitignore.contains("target/"));
        assert!(gitignore.contains(".env"));
        assert!(gitignore.contains(".platy/"));
    }
}
