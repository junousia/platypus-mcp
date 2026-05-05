use super::{
    filesystem::resolve_root, types::ParsedBacklogItem, validate::validate_backlog_at_root,
};
use crate::models::{
    ActionResult, ActionStatus, BacklogCandidate, BacklogListData, ProjectStatusData,
};
use std::{collections::BTreeSet, path::Path};

pub fn inspect_status(
    default_root: &Path,
    root: Option<&str>,
    limit: Option<usize>,
) -> ActionResult<ProjectStatusData> {
    let action = "inspect_status";
    let root = match resolve_root(default_root, root) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not inspect project.", error),
    };
    let validation = validate_backlog_at_root(&root, false);
    let candidates = if validation.ok {
        runnable_backlog_candidates(&validation.items, limit.unwrap_or(10))
    } else {
        Vec::new()
    };
    let data = ProjectStatusData {
        root: root.display().to_string(),
        platy_yaml: root.join("platy.yaml").is_file(),
        backlog_dir: root.join("backlog").is_dir(),
        git_metadata: root.join(".git").exists(),
        backlog_items: validation.items.len(),
        runnable_backlog_items: candidates.len(),
        tasks_supported: true,
        findings_supported: true,
    };
    ActionResult::completed(action, "Project status inspected.", data)
}

pub fn list_backlog(
    default_root: &Path,
    root: Option<&str>,
    limit: Option<usize>,
) -> ActionResult<BacklogListData> {
    let action = "list_backlog";
    let root = match resolve_root(default_root, root) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not list backlog.", error),
    };
    let validation = validate_backlog_at_root(&root, true);
    if !validation.ok {
        return ActionResult::failed(
            action,
            "Backlog is not valid enough to list runnable work.",
            validation.errors.join("\n"),
        );
    }
    let candidates = runnable_backlog_candidates(&validation.items, limit.unwrap_or(10));
    if candidates.is_empty() {
        return ActionResult {
            action: action.to_string(),
            status: ActionStatus::Skipped,
            summary: "No runnable backlog items.".to_string(),
            next_action: Some("Create or unblock backlog items.".to_string()),
            data: Some(BacklogListData {
                root: root.display().to_string(),
                candidates,
            }),
            error: None,
        };
    }
    ActionResult::completed(
        action,
        format!("{} runnable backlog item(s).", candidates.len()),
        BacklogListData {
            root: root.display().to_string(),
            candidates,
        },
    )
}

fn runnable_backlog_candidates(items: &[ParsedBacklogItem], limit: usize) -> Vec<BacklogCandidate> {
    let done_ids: BTreeSet<&str> = items
        .iter()
        .filter(|item| item.frontmatter.status == "done")
        .map(|item| item.frontmatter.id.as_str())
        .collect();
    let mut candidates: Vec<&ParsedBacklogItem> = items
        .iter()
        .filter(|item| matches!(item.frontmatter.status.as_str(), "todo" | "ready"))
        .filter(|item| {
            item.frontmatter
                .depends_on
                .iter()
                .all(|dependency| done_ids.contains(dependency.as_str()))
        })
        .collect();
    candidates.sort_by_key(|item| {
        (
            priority_rank(&item.frontmatter.priority),
            status_rank(&item.frontmatter.status),
            item.frontmatter.id.clone(),
        )
    });
    candidates
        .into_iter()
        .take(limit.min(100))
        .map(|item| BacklogCandidate {
            source: "backlog".to_string(),
            item_id: item.frontmatter.id.clone(),
            title: item.frontmatter.title.clone(),
            status: item.frontmatter.status.clone(),
            priority: item.frontmatter.priority.clone(),
            area: item.frontmatter.area.clone(),
            suggested_worker: item.frontmatter.suggested_worker.clone(),
        })
        .collect()
}

fn priority_rank(priority: &str) -> u8 {
    match priority {
        "P0" => 0,
        "P1" => 1,
        "P2" => 2,
        _ => 9,
    }
}

fn status_rank(status: &str) -> u8 {
    match status {
        "ready" => 0,
        "todo" => 1,
        _ => 9,
    }
}
