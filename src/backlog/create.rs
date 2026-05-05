use super::{
    filesystem::{ensure_child_dir, resolve_root},
    types::{BacklogItemFrontmatterOut, ParsedBacklogItem, VALID_PRIORITIES, VALID_TYPES},
    validate::{valid_item_id, validate_backlog_at_root},
};
use crate::models::{ActionResult, CreateBacklogItemParams, CreatedBacklogItemData};
use std::{fs, path::Path};

pub fn create_backlog_item(
    default_root: &Path,
    params: CreateBacklogItemParams,
) -> ActionResult<CreatedBacklogItemData> {
    let action = "create_backlog_item";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not create backlog item.", error),
    };
    let items_dir = root.join("backlog").join("items");
    if let Err(error) = ensure_child_dir(&root, &items_dir) {
        return ActionResult::failed(action, "Could not create backlog item.", error);
    }
    let validation = validate_backlog_at_root(&root, true);
    let item_id = match params.id.as_deref() {
        Some(id) => normalize_item_id(id),
        None => allocate_item_id(
            &validation.items,
            params.id_prefix.as_deref().unwrap_or("PROJ"),
        ),
    };
    if !valid_item_id(&item_id) {
        return ActionResult::failed(
            action,
            "Could not create backlog item.",
            format!("invalid backlog id `{}`; expected AREA-000", item_id),
        );
    }
    let item_path = items_dir.join(format!("{}.md", item_id));
    if item_path.exists() {
        return ActionResult::failed(
            action,
            format!("Could not create backlog item {}.", item_id),
            "backlog item already exists",
        );
    }
    let epic = clean_optional(params.epic.clone()).unwrap_or_else(|| "general".to_string());
    if !validation.epic_ids.is_empty() && !validation.epic_ids.contains(&epic) {
        return ActionResult::failed(
            action,
            format!("Could not create backlog item {}.", item_id),
            format!("unknown epic `{}`", epic),
        );
    }
    let priority = clean_optional(params.priority.clone()).unwrap_or_else(|| "P1".to_string());
    let item_type =
        clean_optional(params.item_type.clone()).unwrap_or_else(|| "feature".to_string());
    if !VALID_PRIORITIES.contains(&priority.as_str()) {
        return ActionResult::failed(action, "Could not create backlog item.", "invalid priority");
    }
    if !VALID_TYPES.contains(&item_type.as_str()) {
        return ActionResult::failed(action, "Could not create backlog item.", "invalid type");
    }
    let contract = params
        .implementation_contract
        .as_ref()
        .or(params.contract.as_ref())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let acceptance = clean_vec(params.acceptance.clone());
    if params.title.trim().is_empty()
        || params.goal.trim().is_empty()
        || contract.is_none()
        || acceptance.is_empty()
    {
        return ActionResult::failed(
            action,
            "Could not create backlog item.",
            "title, goal, implementation_contract/contract, and acceptance are required",
        );
    }
    let text = match backlog_item_text(&item_id, &params, &priority, &item_type, &epic, &acceptance)
    {
        Ok(text) => text,
        Err(error) => return ActionResult::failed(action, "Could not create backlog item.", error),
    };
    if let Err(error) = fs::write(&item_path, text) {
        return ActionResult::failed(
            action,
            format!("Could not create backlog item {}.", item_id),
            error.to_string(),
        );
    }
    ActionResult::completed(
        action,
        format!("Created backlog item {}.", item_id),
        CreatedBacklogItemData {
            item_id,
            path: item_path.display().to_string(),
            created: true,
        },
    )
}

fn normalize_item_id(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn allocate_item_id(items: &[ParsedBacklogItem], preferred_prefix: &str) -> String {
    let prefix = preferred_prefix
        .trim()
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase();
    let prefix = if prefix.is_empty() {
        "PROJ"
    } else {
        prefix.as_str()
    };
    let mut max_number = 0_u32;
    for item in items {
        if let Some(number) = item
            .frontmatter
            .id
            .strip_prefix(prefix)
            .and_then(|suffix| suffix.strip_prefix('-'))
            .and_then(|number| number.parse::<u32>().ok())
        {
            max_number = max_number.max(number);
        }
    }
    format!("{}-{:03}", prefix, max_number + 1)
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn clean_vec(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn backlog_item_text(
    item_id: &str,
    params: &CreateBacklogItemParams,
    priority: &str,
    item_type: &str,
    epic: &str,
    acceptance: &[String],
) -> std::result::Result<String, String> {
    let contract = params
        .implementation_contract
        .as_deref()
        .or(params.contract.as_deref())
        .unwrap_or("")
        .trim();
    let frontmatter = BacklogItemFrontmatterOut {
        id: item_id.to_string(),
        title: params.title.trim().to_string(),
        status: "todo".to_string(),
        priority: priority.to_string(),
        item_type: item_type.to_string(),
        area: clean_optional(params.area.clone()).unwrap_or_else(|| "tooling".to_string()),
        epic: epic.to_string(),
        depends_on: clean_vec(params.depends_on.clone()),
        blocks: clean_vec(params.blocks.clone()),
        owner_role: clean_optional(params.owner_role.clone()),
        assigned_worker: clean_optional(params.assigned_worker.clone()),
        assignment_reason: clean_optional(params.assignment_reason.clone()),
        suggested_worker: clean_optional(params.suggested_worker.clone())
            .or_else(|| Some("coder".to_string())),
        required_capability_tags: clean_vec(params.required_capability_tags.clone()),
        complexity_tier: clean_optional(params.complexity_tier.clone()),
        owned_surfaces: clean_vec(params.owned_surfaces.clone()),
        github_issue: None,
        github_project_item: None,
        prs: Vec::new(),
        source_item_id: clean_optional(params.source_item_id.clone()),
        source_task_id: clean_optional(params.source_task_id.clone()),
        source_finding_ref: clean_optional(params.source_finding_ref.clone()),
    };
    let yaml = serde_yaml::to_string(&frontmatter).map_err(|error| error.to_string())?;
    let mut body = format!(
        "---\n{}---\n\n# {} {}\n\n## Goal\n\n{}\n\n## Implementation Contract\n\n{}\n\n## Acceptance\n\n{}\n",
        yaml,
        item_id,
        params.title.trim(),
        params.goal.trim(),
        contract,
        acceptance
            .iter()
            .map(|criterion| format!("- {}", criterion))
            .collect::<Vec<_>>()
            .join("\n")
    );
    if let Some(notes) = params
        .notes
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        body.push_str(&format!("\n## Notes\n\n{}\n", notes));
    }
    Ok(body)
}
