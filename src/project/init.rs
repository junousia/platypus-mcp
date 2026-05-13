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
    if !path.exists() {
        fs::write(&path, default_gitignore())
            .map_err(|error| format!("{}: {}", path.display(), error))?;
        entries.push(created_entry(root, &path, "file"));
        return Ok(());
    }
    if !path.is_file() {
        return Err(format!("{} exists but is not a file", path.display()));
    }
    let content =
        fs::read_to_string(&path).map_err(|error| format!("{}: {}", path.display(), error))?;
    let missing_rules = default_gitignore_rules()
        .iter()
        .copied()
        .filter(|rule| !gitignore_has_rule(&content, rule))
        .collect::<Vec<_>>();
    if missing_rules.is_empty() {
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
    updated.push_str("# Platypus runtime and generated files\n");
    for rule in missing_rules {
        updated.push_str(rule);
        updated.push('\n');
    }
    fs::write(&path, updated).map_err(|error| format!("{}: {}", path.display(), error))?;
    entries.push(created_entry(root, &path, "file"));
    Ok(())
}

fn default_gitignore() -> String {
    let mut content = "# Platypus runtime and generated files\n".to_string();
    for rule in default_gitignore_rules() {
        content.push_str(rule);
        content.push('\n');
    }
    content
}

fn default_gitignore_rules() -> &'static [&'static str] {
    &[
        ".platy/",
        "__pycache__/",
        "*.py[cod]",
        ".venv/",
        "node_modules/",
        "dist/",
        "build/",
        ".env",
        ".env.*",
    ]
}

fn gitignore_has_rule(content: &str, rule: &str) -> bool {
    content.lines().any(|line| line.trim() == rule)
}

fn project_config(project_name: &str) -> String {
    format!(
        "project:\n  name: {}\nbacklog:\n  id_prefix: PROJ\n  items: backlog/items\n  epics: backlog/epics\nworkflow:\n  integration:\n    merge_style: merge_commit\n    require_clean_manager_workspace: true\n    require_verification_evidence: false\n  dispatch:\n    auto_commit_artifacts_default: false\n  execution:\n    default_path: direct_edit\n    direct_planning_gate: none\n    worker_planning_gate: task_plan\n",
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

Use the table below as the exact workflow. Read it top to bottom. The first
matching state wins. Do not skip the detecting tool and do not infer hidden
state from chat history.

| State | Detect with | Match condition | Required next action | Exit condition |
| --- | --- | --- | --- | --- |
| `unknown` | session start | state was not freshly inspected | call `inspect_session` | setup, project status, workflow config, and queue facts are known |
| `needs_scaffold` | `doctor_snapshot` | scaffold files are missing | call `init_project`, then `doctor_snapshot` | scaffold blockers are gone |
| `empty_backlog` | `inspect_work_queue` | no backlog items exist | host model chooses concrete items; call `create_backlog_items`, then `validate_backlog` | backlog validates and queue is inspected again |
| `dependency_blocked` | `inspect_work_queue` | `inventory.dependency_blocked_count > 0` and no runnable item is selected | call `inspect_item` on the first blocked item; close or create required dependencies | blocked dependencies are resolved |
| `plan_missing` | `inspect_work_queue` | an item recommends `write_task_plan` | host model writes an explicit plan with `write_task_plan`, then calls `validate_task_plan` | task plan validates cleanly |
| `direct_ready` | `inspect_work_queue` | `queue_state == "direct_ready"` | call `prepare_work`, edit the manager workspace, then call `complete_backlog_item` | direct completion evidence or closure commit exists |
| `worker_ready` | `inspect_work_queue` | `queue_state == "ready"` | call `prepare_work` for one item or `dispatch_ready_work` for a batch | `run_in_worktree` handoff exists |
| `worker_active` | `inspect_task` or `inspect_work_queue` | task is active or prepared | run the external worker in the assigned worktree; call `finish_work` | `finish_work.host_action` is returned |
| `pending_integration` | `inspect_work_queue` or `inspect_integration_gates` | `queue_state == "completed_pending_integration"` or gates are ready | call `inspect_integration_gates`, then `integrate_worker_result`, then `reconcile_project` | work is integrated or a specific blocker is reported |
| `failed_or_unclear` | any tool result | `status == "failed"` or `recovery_action` is present | follow `recovery_action`; if still unclear call `inspect_session` | a known state above matches |

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

- `create_backlog_item` writes one accepted backlog item.
- `create_backlog_items` atomically writes a related set and resolves
  `depends_on_keys`.
- `validate_backlog` checks item and epic schema.
- `inspect_queue_status` shows compact queue counts, top ready work, top
  blocked work, and active tasks.
- `inspect_work_queue` shows runnable items, full routing state, and task-plan
  requirements.
- `write_task_plan` and `validate_task_plan` make non-trivial work executable.
- `prepare_work` returns either direct manager-workspace guidance or a
  worker/worktree handoff.
- `complete_backlog_item` closes direct manager-workspace work.
- `finish_work` closes worker assignments and returns integration guidance.
- `inspect_work_queue` computes detailed queue state and the recommended next
  tool.

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

Use this exact workflow table. Read it top to bottom. The first matching state
wins. Do not skip the detecting tool and do not infer hidden state from chat
history.

| State | Detect with | Match condition | Required next action |
| --- | --- | --- | --- |
| `unknown` | session start | state was not freshly inspected | call `inspect_session` |
| `needs_scaffold` | `doctor_snapshot` | scaffold files are missing | call `init_project`, then `doctor_snapshot` |
| `empty_backlog` | `inspect_work_queue` | no backlog items exist | host model chooses concrete items; call `create_backlog_items`, then `validate_backlog` |
| `dependency_blocked` | `inspect_work_queue` | `inventory.dependency_blocked_count > 0` and no runnable item is selected | call `inspect_item`; close or create required dependencies |
| `plan_missing` | `inspect_work_queue` | an item recommends `write_task_plan` | call `write_task_plan`, then `validate_task_plan` |
| `direct_ready` | `inspect_work_queue` | `queue_state == "direct_ready"` | call `prepare_work`; treat the direct action as response-local guidance, edit manager workspace, then `complete_backlog_item` |
| `worker_ready` | `inspect_work_queue` | `queue_state == "ready"` | call `prepare_work` or `dispatch_ready_work` |
| `worker_active` | `inspect_task` or `inspect_work_queue` | task is active or prepared | run the external worker in the assigned worktree, then `finish_work` |
| `pending_integration` | `inspect_work_queue` or `inspect_integration_gates` | `queue_state == "completed_pending_integration"` or gates are ready | call `inspect_integration_gates`, then `integrate_worker_result`, then `reconcile_project` |
| `failed_or_unclear` | any tool result | `status == "failed"` or `recovery_action` is present | follow `recovery_action`; if still unclear call `inspect_session` |

## Tool Preload

If your MCP host supports tool discovery or schema preloading, load the common
planning group at session start:

`inspect_session`, `doctor_snapshot`, `inspect_status`, `inspect_workflow_config`,
`create_backlog_item`, `create_backlog_items`, `create_epic`,
`validate_backlog`, `list_backlog`, `inspect_backlog_inventory`,
`inspect_queue_status`, `inspect_work_queue`, `write_task_plan`, `validate_task_plan`,
`inspect_task_plan`, `request_planning_approval`, `approval_respond`.

Before dispatch, handoff, verification, or integration work, load the execution
group:

`inspect_queue_status`, `inspect_work_queue`, `prepare_work`, `dispatch_ready_work`,
`commit_planning_artifacts`, `generate_task_bundle`,
`inspect_task_events`, `events_replay`, `worktree_status`,
`inspect_worktree_changes`, `send_worker_guidance`, `start_worker_task`,
`record_worker_progress`, `complete_worker_task`, `complete_backlog_item`,
`finish_work`, `run_task_verification`, `record_verification_evidence`,
`record_finding`, `validate_findings`,
`inspect_integration_gates`, `integrate_worker_result`, `worktree_cleanup`,
`reconcile_project`.

Preloading is optional and host-specific. If the host cannot preload tool
schemas, continue normally and call tools as needed.

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
    "---\nid: PROJ-000\ntitle: Item title\npriority: P1\ntype: feature\narea: general\nepic: general\ndepends_on: []\nowned_surfaces: []\n---\n\n# PROJ-000 Item title\n\n## Goal\n\nDescribe the goal.\n\n## Implementation Contract\n\nDescribe the expected implementation boundaries.\n\n## Acceptance\n\n- Describe a verifiable acceptance criterion.\n".to_string()
}

fn plan_template() -> String {
    "item_id: PROJ-000\nversion: 1\nmode: standard\nrequirements:\n  - id: R1\n    text: Describe a required outcome.\ndesign:\n  summary: Describe the implementation approach.\n  owned_surfaces:\n    - src/example.rs\n  notes: null\ntasks:\n  - id: PROJ-000-T001\n    title: Implement the first task\n    goal: Deliver one executable implementation slice.\n    requirement_refs:\n      - R1\n    depends_on: []\n    owned_surfaces:\n      - src/example.rs\n    verification:\n      - make check\n    acceptance:\n      - The task is complete and verified.\n    notes: null\n".to_string()
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
        assert!(gitignore.contains("__pycache__/"));
        assert!(gitignore.contains("node_modules/"));
        assert!(gitignore.contains(".env"));
        let config = fs::read_to_string(temp.path().join("platy.yaml")).expect("config");
        assert!(config.contains("workflow:"));
        assert!(config.contains("merge_style: merge_commit"));
        let agents = fs::read_to_string(temp.path().join("AGENTS.md")).expect("agents");
        assert!(agents.contains("spec-driven development"));
        assert!(agents.contains("inspect_queue_status"));
        assert!(agents.contains("inspect_work_queue"));
        assert!(agents.contains("first matching state"));
        assert!(agents.contains("queue_state == \"direct_ready\""));
        assert!(agents.contains("completed_pending_integration"));
        assert!(agents.contains("write_task_plan"));
        assert!(!agents.contains("draft_task_plan"));
        assert!(agents.contains("Tool Preload"));
        assert!(agents.contains("planning group"));
        assert!(agents.contains("execution"));
        assert!(agents.contains("request_planning_approval"));
        assert!(agents.contains("prepare_work"));
        assert!(agents.contains("dispatch_ready_work"));
        assert!(agents.contains("complete_backlog_item"));
        assert!(agents.contains("finish_work"));
        let claude = fs::read_to_string(temp.path().join("CLAUDE.md")).expect("claude");
        assert!(claude.contains("spec-driven development"));
        assert!(claude.contains("inspect_queue_status"));
        assert!(claude.contains("inspect_work_queue"));
        assert!(claude.contains("first matching state"));
        assert!(claude.contains("queue_state == \"direct_ready\""));
        assert!(claude.contains("completed_pending_integration"));
        assert!(claude.contains("write_task_plan"));
        assert!(!claude.contains("draft_task_plan"));
        assert!(claude.contains("Tool Preload"));
        assert!(claude.contains("planning group"));
        assert!(claude.contains("execution"));
        assert!(claude.contains("request_planning_approval"));
        assert!(claude.contains("prepare_work"));
        assert!(claude.contains("dispatch_ready_work"));
        assert!(claude.contains("complete_backlog_item"));
        assert!(claude.contains("finish_work"));
        assert_eq!(
            agents.replace("Agent Instructions", "Shared Instructions"),
            claude.replace("Claude Instructions", "Shared Instructions")
        );
        let workflow = fs::read_to_string(temp.path().join("WORKFLOW.md")).expect("workflow");
        assert!(workflow.contains("matching state wins"));
        assert!(workflow.contains("queue_state == \"direct_ready\""));
        assert!(workflow.contains("completed_pending_integration"));
        assert!(workflow.contains("spec-driven development"));
        assert!(workflow.contains("dispatch_ready_work"));
        assert!(workflow.contains("complete_backlog_item"));
        let backlog_readme =
            fs::read_to_string(temp.path().join("backlog/README.md")).expect("backlog readme");
        assert!(backlog_readme.contains("inspect_queue_status"));
        assert!(backlog_readme.contains("inspect_work_queue"));
        assert!(backlog_readme.contains("task-plan"));
        assert!(backlog_readme.contains("complete_backlog_item"));
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
        assert!(gitignore.contains("__pycache__/"));
        assert!(gitignore.contains("node_modules/"));
    }
}
