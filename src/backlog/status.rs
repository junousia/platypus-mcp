use super::{
    closure::closed_item_ids, filesystem::resolve_root, types::ParsedBacklogItem,
    validate::validate_backlog_at_root,
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
    let closed_ids = closed_item_ids(&root);
    let candidates = if validation.ok {
        runnable_backlog_candidates(&validation.items, &closed_ids, limit.unwrap_or(10))
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
    let closed_ids = closed_item_ids(&root);
    let candidates =
        runnable_backlog_candidates(&validation.items, &closed_ids, limit.unwrap_or(10));
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

fn runnable_backlog_candidates(
    items: &[ParsedBacklogItem],
    closed_ids: &BTreeSet<String>,
    limit: usize,
) -> Vec<BacklogCandidate> {
    let mut candidates: Vec<&ParsedBacklogItem> = items
        .iter()
        .filter(|item| !closed_ids.contains(&item.frontmatter.id))
        .filter(|item| {
            item.frontmatter
                .depends_on
                .iter()
                .all(|dependency| closed_ids.contains(dependency))
        })
        .collect();
    candidates.sort_by_key(|item| {
        (
            priority_rank(&item.frontmatter.priority),
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
            priority: item.frontmatter.priority.clone(),
            area: item.frontmatter.area.clone(),
            suggested_worker: item.frontmatter.suggested_worker.clone(),
            owned_surfaces: item.frontmatter.owned_surfaces.clone(),
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
