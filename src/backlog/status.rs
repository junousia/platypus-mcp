use super::{
    closure::closed_item_ids, filesystem::resolve_root, types::ParsedBacklogItem,
    validate::validate_backlog_at_root,
};
use crate::models::{
    ActionResult, ActionStatus, BacklogCandidate, BacklogInventoryData, BacklogInventoryItem,
    BacklogListData, ProjectStatusData,
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

pub fn inspect_backlog_inventory(
    default_root: &Path,
    root: Option<&str>,
    limit: Option<usize>,
) -> ActionResult<BacklogInventoryData> {
    let action = "inspect_backlog_inventory";
    let root = match resolve_root(default_root, root) {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(action, "Could not inspect backlog inventory.", error)
        }
    };
    let validation = validate_backlog_at_root(&root, true);
    if !validation.ok {
        return ActionResult::failed(
            action,
            "Backlog is not valid enough to inspect inventory.",
            validation.errors.join("\n"),
        );
    }
    let closed_ids = closed_item_ids(&root);
    let mut items = backlog_inventory_items(&validation.items, &closed_ids);
    let total = items.len();
    let runnable = items.iter().filter(|item| item.runnable).count();
    let closed = items.iter().filter(|item| item.closed).count();
    let blocked = items
        .iter()
        .filter(|item| !item.closed && !item.open_dependencies.is_empty())
        .count();

    let truncated = if let Some(limit) = limit {
        items.truncate(limit);
        items.len() < total
    } else {
        false
    };
    let returned = items.len();

    ActionResult::completed(
        action,
        format!("Backlog inventory inspected: {total} item(s), {runnable} runnable."),
        BacklogInventoryData {
            root: root.display().to_string(),
            items,
            total,
            returned,
            truncated,
            runnable,
            closed,
            blocked,
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
            item_type: item.frontmatter.item_type.clone(),
            area: item.frontmatter.area.clone(),
            suggested_worker: item.frontmatter.suggested_worker.clone(),
            owned_surfaces: item.frontmatter.owned_surfaces.clone(),
            external_refs: item.frontmatter.external_refs.clone(),
        })
        .collect()
}

fn backlog_inventory_items(
    items: &[ParsedBacklogItem],
    closed_ids: &BTreeSet<String>,
) -> Vec<BacklogInventoryItem> {
    let mut inventory: Vec<_> = items
        .iter()
        .map(|item| {
            let id = &item.frontmatter.id;
            let closed = closed_ids.contains(id);
            let open_dependencies = item
                .frontmatter
                .depends_on
                .iter()
                .filter(|dependency| !closed_ids.contains(*dependency))
                .cloned()
                .collect::<Vec<_>>();
            let runnable = !closed && open_dependencies.is_empty();
            let reason = if closed {
                "closed by reachable Platypus-Closes Git trailer".to_string()
            } else if !open_dependencies.is_empty() {
                format!(
                    "blocked by open dependencies: {}",
                    open_dependencies.join(", ")
                )
            } else {
                "runnable".to_string()
            };

            BacklogInventoryItem {
                item_id: id.clone(),
                title: item.frontmatter.title.clone(),
                priority: item.frontmatter.priority.clone(),
                item_type: item.frontmatter.item_type.clone(),
                area: item.frontmatter.area.clone(),
                suggested_worker: item.frontmatter.suggested_worker.clone(),
                owned_surfaces: item.frontmatter.owned_surfaces.clone(),
                depends_on: item.frontmatter.depends_on.clone(),
                open_dependencies,
                closed,
                runnable,
                reason,
            }
        })
        .collect();
    inventory.sort_by_key(|item| {
        (
            inventory_state_rank(item),
            priority_rank(&item.priority),
            item.item_id.clone(),
        )
    });
    inventory
}

fn inventory_state_rank(item: &BacklogInventoryItem) -> u8 {
    if item.runnable {
        0
    } else if !item.open_dependencies.is_empty() {
        1
    } else if item.closed {
        2
    } else {
        3
    }
}

fn priority_rank(priority: &str) -> u8 {
    match priority {
        "P0" => 0,
        "P1" => 1,
        "P2" => 2,
        _ => 9,
    }
}
