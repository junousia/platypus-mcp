use crate::{
    models::{ActionResult, GenerateTaskBundleParams, TaskBundle, TaskBundleData, TaskRecord},
    state::{sqlite::SqliteProjectState, ProjectState, TaskQuery, TaskSnapshot},
};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

const SECTION_LIMIT: usize = 6_000;
const BRIEF_LIMIT: usize = 16_000;

#[derive(Debug, Deserialize)]
struct BundleFrontmatter {
    id: String,
    title: String,
    #[serde(default)]
    depends_on: Vec<String>,
    suggested_worker: Option<String>,
    #[serde(default)]
    owned_surfaces: Vec<String>,
}

struct ParsedBundleItem {
    frontmatter: BundleFrontmatter,
    goal: String,
    implementation_contract: String,
    acceptance: Vec<String>,
}

pub fn generate_task_bundle(
    default_root: &Path,
    params: GenerateTaskBundleParams,
) -> ActionResult<TaskBundleData> {
    let action = "generate_task_bundle";
    let task_id = params.task_id.trim();
    if task_id.is_empty() {
        return ActionResult::failed(
            action,
            "Could not generate task bundle.",
            "task_id is required",
        );
    }
    let state = match SqliteProjectState::open(default_root, params.root.as_deref()) {
        Ok(state) => state,
        Err(error) => {
            return ActionResult::failed(action, "Could not open task storage.", error.to_string())
        }
    };
    let root = state.root().to_path_buf();
    let task = match state.inspect_task(TaskQuery {
        task_id: task_id.to_string(),
    }) {
        Ok(task) => task_record(task),
        Err(crate::state::ProjectStateError::NotFound { .. }) => {
            return ActionResult::skipped(
                action,
                format!("Task `{task_id}` was not found."),
                "Dispatch work before generating a task bundle.",
            )
        }
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect task.", error.to_string())
        }
    };
    let workspace_path = match workspace_path(&root, &task) {
        Ok(path) => path,
        Err(BundleMissing::Skipped(summary, next_action)) => {
            return ActionResult::skipped(action, summary, &next_action)
        }
        Err(BundleMissing::Failed(error)) => {
            return ActionResult::failed(action, "Could not inspect task workspace.", error)
        }
    };
    let item = match parse_backlog_item_for_task(&root, &task.source_item_id) {
        Ok(item) => item,
        Err(BundleMissing::Skipped(summary, next_action)) => {
            return ActionResult::skipped(action, summary, &next_action)
        }
        Err(BundleMissing::Failed(error)) => {
            return ActionResult::failed(action, "Could not read backlog item.", error)
        }
    };
    let verification_command = clean_command(params.verification_command);
    let brief = bundle_brief(&task, &item, &workspace_path, &verification_command);
    let bundle = TaskBundle {
        task_id: task.id.clone(),
        item_id: item.frontmatter.id.clone(),
        title: item.frontmatter.title.clone(),
        worker: task
            .worker
            .clone()
            .or_else(|| item.frontmatter.suggested_worker.clone()),
        workspace_path: workspace_path.display().to_string(),
        goal: item.goal,
        implementation_contract: item.implementation_contract,
        acceptance: item.acceptance,
        dependencies: item.frontmatter.depends_on,
        owned_surfaces: item.frontmatter.owned_surfaces,
        verification_command,
        brief,
    };

    ActionResult::completed(
        action,
        format!("Generated task bundle for `{}`.", task.id),
        TaskBundleData {
            root: root.display().to_string(),
            bundle,
        },
    )
}

enum BundleMissing {
    Skipped(String, String),
    Failed(String),
}

fn task_record(task: TaskSnapshot) -> TaskRecord {
    let (workspace_path, workspace_branch, workspace_base_ref) = match task.worker_workspace {
        Some(workspace) => (
            Some(workspace.path),
            Some(workspace.branch),
            Some(workspace.base_ref),
        ),
        None => (None, None, None),
    };
    TaskRecord {
        id: task.id,
        source_item_id: task.source_item_id,
        title: task.title,
        status: task.status,
        worker: task.worker,
        claimed_by: task.claimed_by,
        claimed_at: task.claimed_at,
        started_at: task.started_at,
        finished_at: task.finished_at,
        workspace_path,
        workspace_branch,
        workspace_base_ref,
        created_at: task.created_at,
        updated_at: task.updated_at,
    }
}

fn workspace_path(root: &Path, task: &TaskRecord) -> Result<PathBuf, BundleMissing> {
    let Some(path) = task.workspace_path.as_ref() else {
        return Err(BundleMissing::Skipped(
            format!("Task `{}` does not have a workspace.", task.id),
            "Create a task worktree before generating a bundle.".to_string(),
        ));
    };
    let canonical = fs::canonicalize(path).map_err(|error| {
        BundleMissing::Skipped(
            format!("Task `{}` workspace path is not available.", task.id),
            format!("Restore the workspace or recreate it: {error}"),
        )
    })?;
    let worktrees_root = fs::canonicalize(root.join(".platy").join("worktrees"))
        .map_err(|error| BundleMissing::Failed(error.to_string()))?;
    if !canonical.starts_with(&worktrees_root) {
        return Err(BundleMissing::Failed(format!(
            "{} escapes {}",
            canonical.display(),
            worktrees_root.display()
        )));
    }
    Ok(canonical)
}

fn parse_backlog_item_for_task(
    root: &Path,
    item_id: &str,
) -> Result<ParsedBundleItem, BundleMissing> {
    let path = root
        .join("backlog")
        .join("items")
        .join(format!("{item_id}.md"));
    let canonical = fs::canonicalize(&path).map_err(|error| {
        BundleMissing::Skipped(
            format!("Backlog item `{item_id}` was not found."),
            format!("Create or restore {}: {error}", path.display()),
        )
    })?;
    if !canonical.starts_with(root) {
        return Err(BundleMissing::Failed(format!(
            "{} escapes project root {}",
            canonical.display(),
            root.display()
        )));
    }
    let text =
        fs::read_to_string(&canonical).map_err(|error| BundleMissing::Failed(error.to_string()))?;
    let (frontmatter, body) = split_frontmatter(&text).map_err(BundleMissing::Failed)?;
    let frontmatter: BundleFrontmatter = serde_yaml::from_str(frontmatter)
        .map_err(|error| BundleMissing::Failed(error.to_string()))?;
    let goal = limit_text(&section(body, "Goal").ok_or_else(|| {
        BundleMissing::Failed(format!("{} missing Goal section", canonical.display()))
    })?);
    let implementation_contract =
        limit_text(&section(body, "Implementation Contract").ok_or_else(|| {
            BundleMissing::Failed(format!(
                "{} missing Implementation Contract section",
                canonical.display()
            ))
        })?);
    let acceptance_text = section(body, "Acceptance").ok_or_else(|| {
        BundleMissing::Failed(format!(
            "{} missing Acceptance section",
            canonical.display()
        ))
    })?;
    let acceptance = parse_acceptance(&acceptance_text);
    if acceptance.is_empty() {
        return Err(BundleMissing::Failed(format!(
            "{} Acceptance section is empty",
            canonical.display()
        )));
    }
    Ok(ParsedBundleItem {
        frontmatter,
        goal,
        implementation_contract,
        acceptance,
    })
}

fn split_frontmatter(text: &str) -> Result<(&str, &str), String> {
    let rest = text
        .strip_prefix("---\n")
        .ok_or_else(|| "missing YAML frontmatter".to_string())?;
    let marker = "\n---\n";
    let end = rest
        .find(marker)
        .ok_or_else(|| "unterminated YAML frontmatter".to_string())?;
    Ok((&rest[..end], &rest[end + marker.len()..]))
}

fn section(body: &str, heading: &str) -> Option<String> {
    let marker = format!("## {heading}");
    let mut lines = body.lines();
    for line in lines.by_ref() {
        if line.trim() == marker {
            break;
        }
    }
    let mut collected = Vec::new();
    for line in lines {
        if line.starts_with("## ") {
            break;
        }
        collected.push(line);
    }
    if collected.is_empty() {
        None
    } else {
        Some(collected.join("\n").trim().to_string())
    }
}

fn parse_acceptance(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .unwrap_or(line)
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .map(|line| limit_text(&line))
        .collect()
}

fn clean_command(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn bundle_brief(
    task: &TaskRecord,
    item: &ParsedBundleItem,
    workspace_path: &Path,
    verification_command: &[String],
) -> String {
    let verify = if verification_command.is_empty() {
        "No verification command was provided. Report what can be verified safely.".to_string()
    } else {
        verification_command.join(" ")
    };
    let acceptance = item
        .acceptance
        .iter()
        .map(|criterion| format!("- {criterion}"))
        .collect::<Vec<_>>()
        .join("\n");
    let surfaces = if item.frontmatter.owned_surfaces.is_empty() {
        "- No owned surfaces declared.".to_string()
    } else {
        item.frontmatter
            .owned_surfaces
            .iter()
            .map(|surface| format!("- {surface}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    limit_brief(&format!(
        "# Task Bundle: {item_id}\n\n\
         Task: {task_id}\n\
         Title: {title}\n\
         Workspace: {workspace}\n\
         Worker: {worker}\n\n\
         ## Goal\n\n{goal}\n\n\
         ## Implementation Contract\n\n{contract}\n\n\
         ## Acceptance\n\n{acceptance}\n\n\
         ## Owned Surfaces\n\n{surfaces}\n\n\
         ## Verification\n\n{verify}\n",
        item_id = item.frontmatter.id.as_str(),
        task_id = task.id.as_str(),
        title = item.frontmatter.title.as_str(),
        workspace = workspace_path.display(),
        worker = task
            .worker
            .as_deref()
            .or(item.frontmatter.suggested_worker.as_deref())
            .unwrap_or("unspecified"),
        goal = item.goal.as_str(),
        contract = item.implementation_contract.as_str(),
    ))
}

fn limit_text(value: &str) -> String {
    let mut value = value.trim().to_string();
    if value.len() > SECTION_LIMIT {
        value.truncate(SECTION_LIMIT);
        value.push_str("...");
    }
    value
}

fn limit_brief(value: &str) -> String {
    let mut value = value.trim().to_string();
    if value.len() > BRIEF_LIMIT {
        value.truncate(BRIEF_LIMIT);
        value.push_str("...");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::WorktreeCreateParams,
        tasks::{create_task_record, NewTask},
        workspace::worktree_create,
    };
    use std::{fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn generates_bundle_for_task_workspace() {
        let project = project_with_backlog();
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Bundle task".to_string(),
                worker: Some("coder".to_string()),
            },
        )
        .expect("task");
        worktree_create(
            project.path(),
            WorktreeCreateParams {
                root: None,
                task_id: task.id.clone(),
                base_ref: None,
            },
        )
        .data
        .expect("worktree");

        let result = generate_task_bundle(
            project.path(),
            GenerateTaskBundleParams {
                root: None,
                task_id: task.id.clone(),
                verification_command: vec!["make".to_string(), "check".to_string()],
            },
        );
        let bundle = result.data.expect("bundle data").bundle;

        assert!(matches!(
            result.status,
            crate::models::ActionStatus::Completed
        ));
        assert_eq!(bundle.task_id, task.id);
        assert_eq!(bundle.item_id, "PROJ-001");
        assert_eq!(bundle.acceptance, vec!["Bundle exists."]);
        assert_eq!(bundle.dependencies, vec!["MCP-006"]);
        assert_eq!(bundle.owned_surfaces, vec!["src/bundle.rs"]);
        assert_eq!(bundle.verification_command, vec!["make", "check"]);
        assert!(bundle.brief.contains("## Goal"));
    }

    #[test]
    fn missing_task_returns_skipped_result() {
        let project = project_with_backlog();

        let result = generate_task_bundle(
            project.path(),
            GenerateTaskBundleParams {
                root: None,
                task_id: "missing".to_string(),
                verification_command: Vec::new(),
            },
        );

        assert!(matches!(
            result.status,
            crate::models::ActionStatus::Skipped
        ));
    }

    #[test]
    fn missing_workspace_returns_skipped_result() {
        let project = project_with_backlog();
        let task = create_task_record(
            project.path(),
            None,
            NewTask {
                source_item_id: "PROJ-001".to_string(),
                title: "Bundle task".to_string(),
                worker: None,
            },
        )
        .expect("task");

        let result = generate_task_bundle(
            project.path(),
            GenerateTaskBundleParams {
                root: None,
                task_id: task.id,
                verification_command: Vec::new(),
            },
        );

        assert!(matches!(
            result.status,
            crate::models::ActionStatus::Skipped
        ));
        assert!(result.summary.contains("does not have a workspace"));
    }

    fn project_with_backlog() -> TempDir {
        let project = TempDir::new().expect("temp dir");
        git(&project, &["init"]);
        git(&project, &["config", "user.name", "Platypus Test"]);
        git(
            &project,
            &["config", "user.email", "platypus@example.invalid"],
        );
        fs::write(project.path().join("README.md"), "# Test\n").expect("readme");
        git(&project, &["add", "README.md"]);
        git(&project, &["commit", "-m", "Initial commit"]);
        fs::create_dir_all(project.path().join("backlog/items")).expect("items");
        fs::write(
            project.path().join("backlog/items/PROJ-001.md"),
            r#"---
id: PROJ-001
title: Generate bundle
priority: P0
type: foundation
area: execution
epic: general
depends_on:
- MCP-006
suggested_worker: coder
owned_surfaces:
- src/bundle.rs
---

# PROJ-001 Generate bundle

## Goal

Create a task bundle.

## Implementation Contract

Read task state and backlog content.

## Acceptance

- Bundle exists.
"#,
        )
        .expect("item");
        project
    }

    fn git(project: &TempDir, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(project.path())
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
}
