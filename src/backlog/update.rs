use super::{
    closure::closed_item_ids,
    filesystem::resolve_root,
    types::{
        BacklogItemFrontmatter, BacklogItemFrontmatterOut, BacklogValidation, VALID_PRIORITIES,
        VALID_TYPES,
    },
    validate::{valid_item_id, validate_backlog_at_root},
};
use crate::{
    execution_policy,
    models::{
        ActionResult, ActionStatus, BacklogValidationData, UpdateBacklogItemParams,
        UpdatedBacklogItemData,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Debug)]
struct ParsedBody {
    sections: BTreeMap<String, String>,
    order: Vec<String>,
}

#[derive(Debug)]
struct UpdatedItem {
    frontmatter: BacklogItemFrontmatter,
    body: ParsedBody,
    changed_fields: Vec<String>,
    requested_updates: usize,
}

pub fn update_backlog_item(
    default_root: &Path,
    params: UpdateBacklogItemParams,
) -> ActionResult<UpdatedBacklogItemData> {
    let action = "update_backlog_item";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not update backlog item.", error),
    };
    let item_id = normalize_item_id(&params.item_id);
    if !valid_item_id(&item_id) {
        return failed_with_next(
            action,
            "Could not update backlog item.",
            format!(
                "invalid backlog item id `{}`; expected `PREFIX-NNN`",
                params.item_id
            ),
            "Use a canonical backlog item ID such as `PROJ-001`.",
        );
    }
    let validation = validate_backlog_at_root(&root, true);
    if !validation.ok {
        return failed_with_next(
            action,
            "Could not update backlog item.",
            format_existing_validation_error(&validation),
            "Run validate_backlog, fix existing backlog errors, then retry update_backlog_item.",
        );
    }
    let Some(item) = validation
        .items
        .iter()
        .find(|item| item.frontmatter.id == item_id)
        .cloned()
    else {
        return failed_with_next(
            action,
            format!("Could not update backlog item {item_id}."),
            format!("backlog item `{item_id}` was not found"),
            "Call inspect_work_queue or list_backlog to choose an existing backlog item ID.",
        );
    };
    let closed = closed_item_ids(&root).contains(&item_id);
    if closed && !params.force_closed.unwrap_or(false) {
        return failed_with_next(
            action,
            format!("Could not update backlog item {item_id}."),
            format!("backlog item `{item_id}` is already closed"),
            "Closed backlog items are protected. Create a follow-up item, or retry update_backlog_item with force_closed=true if this is intentional.",
        );
    }

    let original = match fs::read_to_string(&item.path) {
        Ok(text) => text,
        Err(error) => {
            return ActionResult::failed(
                action,
                format!("Could not update backlog item {item_id}."),
                error.to_string(),
            )
        }
    };
    let (_, body_text) = match split_frontmatter(&original) {
        Ok(parts) => parts,
        Err(error) => {
            return ActionResult::failed(
                action,
                format!("Could not update backlog item {item_id}."),
                error,
            )
        }
    };
    let updated = match apply_update(item.frontmatter.clone(), parse_body(body_text), &params) {
        Ok(updated) => updated,
        Err(error) => {
            return failed_with_next(
                action,
                "Could not update backlog item.",
                error,
                "Fix the update input and retry update_backlog_item.",
            )
        }
    };
    if updated.requested_updates == 0 {
        return failed_with_next(
            action,
            "Could not update backlog item.",
            "no update fields were provided",
            "Provide at least one field to update, such as title, priority, goal, implementation_contract, or acceptance.",
        );
    }
    if updated.changed_fields.is_empty() {
        let data = UpdatedBacklogItemData {
            root: root.display().to_string(),
            item_id,
            path: item.path.display().to_string(),
            closed,
            changed_fields: Vec::new(),
            validation: backlog_validation_data(&root, &validation, true),
        };
        return ActionResult::completed(
            action,
            "Backlog item already matched requested values.",
            data,
        );
    }

    let next_text = match render_item(&updated.frontmatter, &updated.body) {
        Ok(text) => text,
        Err(error) => {
            return ActionResult::failed(
                action,
                format!("Could not update backlog item {item_id}."),
                error,
            )
        }
    };
    let previous = original.into_bytes();
    if let Err(error) = fs::write(&item.path, next_text) {
        return ActionResult::failed(
            action,
            format!("Could not update backlog item {item_id}."),
            error.to_string(),
        );
    }
    let post_write = validate_backlog_at_root(&root, true);
    if !post_write.ok {
        let rollback = fs::write(&item.path, &previous);
        let mut error = format!(
            "backlog validation failed after update; rolled back {}. {}",
            item.path.display(),
            format_existing_validation_error(&post_write)
        );
        if let Err(rollback_error) = rollback {
            error.push_str(&format!("\nrollback failed: {rollback_error}"));
        }
        return failed_with_next(
            action,
            format!("Could not update backlog item {item_id}."),
            error,
            "Fix the update input or existing backlog state, then retry update_backlog_item.",
        );
    }

    ActionResult::completed(
        action,
        format!(
            "Updated backlog item {item_id}: {}.",
            updated.changed_fields.join(", ")
        ),
        UpdatedBacklogItemData {
            root: root.display().to_string(),
            item_id,
            path: item.path.display().to_string(),
            closed,
            changed_fields: updated.changed_fields,
            validation: backlog_validation_data(&root, &post_write, true),
        },
    )
}

fn apply_update(
    mut frontmatter: BacklogItemFrontmatter,
    mut body: ParsedBody,
    params: &UpdateBacklogItemParams,
) -> Result<UpdatedItem, String> {
    let mut changed_fields = Vec::new();
    let mut requested_updates = 0;

    update_string(
        &mut frontmatter.title,
        params.title.as_deref(),
        "title",
        true,
        &mut requested_updates,
        &mut changed_fields,
    )?;
    update_string(
        &mut frontmatter.priority,
        params.priority.as_deref(),
        "priority",
        true,
        &mut requested_updates,
        &mut changed_fields,
    )?;
    if params.priority.is_some() && !VALID_PRIORITIES.contains(&frontmatter.priority.as_str()) {
        return Err(format!(
            "invalid priority `{}`; expected one of: {}",
            frontmatter.priority,
            VALID_PRIORITIES.join(", ")
        ));
    }
    if let Some(item_type) = params.item_type.as_deref() {
        requested_updates += 1;
        let item_type = normalize_item_type(item_type);
        if !VALID_TYPES.contains(&item_type.as_str()) {
            return Err(format!(
                "invalid type `{}`; expected one of: {}",
                item_type,
                VALID_TYPES.join(", ")
            ));
        }
        if frontmatter.item_type != item_type {
            frontmatter.item_type = item_type;
            changed_fields.push("type".to_string());
        }
    }
    update_string(
        &mut frontmatter.area,
        params.area.as_deref(),
        "area",
        true,
        &mut requested_updates,
        &mut changed_fields,
    )?;
    update_string(
        &mut frontmatter.epic,
        params.epic.as_deref(),
        "epic",
        true,
        &mut requested_updates,
        &mut changed_fields,
    )?;
    if let Some(depends_on) = &params.depends_on {
        requested_updates += 1;
        let depends_on = clean_item_ids(depends_on);
        if frontmatter.depends_on != depends_on {
            frontmatter.depends_on = depends_on;
            changed_fields.push("depends_on".to_string());
        }
    }
    if let Some(owned_surfaces) = &params.owned_surfaces {
        requested_updates += 1;
        let owned_surfaces = clean_vec(owned_surfaces);
        if frontmatter.owned_surfaces != owned_surfaces {
            frontmatter.owned_surfaces = owned_surfaces;
            changed_fields.push("owned_surfaces".to_string());
        }
    }
    if let Some(external_refs) = &params.external_refs {
        requested_updates += 1;
        if frontmatter.external_refs != *external_refs {
            frontmatter.external_refs = external_refs.clone();
            changed_fields.push("external_refs".to_string());
        }
    }
    if let Some(execution_path) = params.execution_path.as_deref() {
        requested_updates += 1;
        let execution_path = execution_policy::normalize_execution_path(execution_path)
            .ok_or_else(|| {
                format!(
                    "invalid execution_path `{execution_path}`; expected direct_edit or worker_handoff"
                )
            })?
            .to_string();
        if frontmatter.execution_path.as_deref() != Some(execution_path.as_str()) {
            frontmatter.execution_path = Some(execution_path);
            changed_fields.push("execution_path".to_string());
        }
    }
    if let Some(planning_gate) = params.planning_gate.as_deref() {
        requested_updates += 1;
        let planning_gate = execution_policy::normalize_planning_gate(planning_gate)
            .ok_or_else(|| {
                format!(
                    "invalid planning_gate `{planning_gate}`; expected none, task_plan, or approved_task_plan"
                )
            })?
            .to_string();
        if frontmatter.planning_gate.as_deref() != Some(planning_gate.as_str()) {
            frontmatter.planning_gate = Some(planning_gate);
            changed_fields.push("planning_gate".to_string());
        }
    }
    update_section(
        &mut body,
        params.goal.as_deref(),
        "Goal",
        "goal",
        true,
        &mut requested_updates,
        &mut changed_fields,
    )?;
    if params.implementation_contract.is_some() && params.contract.is_some() {
        return Err(
            "provide only one of implementation_contract or contract in the same update"
                .to_string(),
        );
    }
    let contract = params
        .implementation_contract
        .as_deref()
        .or(params.contract.as_deref());
    update_section(
        &mut body,
        contract,
        "Implementation Contract",
        "implementation_contract",
        true,
        &mut requested_updates,
        &mut changed_fields,
    )?;
    if let Some(acceptance) = &params.acceptance {
        requested_updates += 1;
        let acceptance = clean_vec(acceptance);
        if acceptance.is_empty() {
            return Err("acceptance must contain at least one criterion".to_string());
        }
        let rendered = acceptance
            .iter()
            .map(|criterion| format!("- {criterion}"))
            .collect::<Vec<_>>()
            .join("\n");
        if body.sections.get("Acceptance").map(String::as_str) != Some(rendered.as_str()) {
            body.sections.insert("Acceptance".to_string(), rendered);
            ensure_section_order(&mut body.order, "Acceptance");
            changed_fields.push("acceptance".to_string());
        }
    }
    if let Some(notes) = params.notes.as_deref() {
        requested_updates += 1;
        let notes = notes.trim();
        if notes.is_empty() {
            if body.sections.remove("Notes").is_some() {
                body.order.retain(|section| section != "Notes");
                changed_fields.push("notes".to_string());
            }
        } else if body.sections.get("Notes").map(String::as_str) != Some(notes) {
            body.sections.insert("Notes".to_string(), notes.to_string());
            ensure_section_order(&mut body.order, "Notes");
            changed_fields.push("notes".to_string());
        }
    }

    Ok(UpdatedItem {
        frontmatter,
        body,
        changed_fields,
        requested_updates,
    })
}

fn update_string(
    target: &mut String,
    value: Option<&str>,
    field: &str,
    require_nonempty: bool,
    requested_updates: &mut usize,
    changed_fields: &mut Vec<String>,
) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    *requested_updates += 1;
    let value = value.trim();
    if require_nonempty && value.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if target != value {
        *target = value.to_string();
        changed_fields.push(field.to_string());
    }
    Ok(())
}

fn update_section(
    body: &mut ParsedBody,
    value: Option<&str>,
    section: &str,
    field: &str,
    require_nonempty: bool,
    requested_updates: &mut usize,
    changed_fields: &mut Vec<String>,
) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    *requested_updates += 1;
    let value = value.trim();
    if require_nonempty && value.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if body.sections.get(section).map(String::as_str) != Some(value) {
        body.sections.insert(section.to_string(), value.to_string());
        ensure_section_order(&mut body.order, section);
        changed_fields.push(field.to_string());
    }
    Ok(())
}

fn render_item(
    frontmatter: &BacklogItemFrontmatter,
    body: &ParsedBody,
) -> std::result::Result<String, String> {
    let frontmatter = BacklogItemFrontmatterOut {
        id: frontmatter.id.clone(),
        title: frontmatter.title.clone(),
        priority: frontmatter.priority.clone(),
        item_type: frontmatter.item_type.clone(),
        area: frontmatter.area.clone(),
        epic: frontmatter.epic.clone(),
        depends_on: frontmatter.depends_on.clone(),
        owned_surfaces: frontmatter.owned_surfaces.clone(),
        external_refs: frontmatter.external_refs.clone(),
        execution_path: frontmatter.execution_path.clone(),
        planning_gate: frontmatter.planning_gate.clone(),
    };
    let yaml = serde_yaml::to_string(&frontmatter).map_err(|error| error.to_string())?;
    let mut text = format!(
        "---\n{}---\n\n# {} {}\n",
        yaml, frontmatter.id, frontmatter.title
    );
    for section in ["Goal", "Implementation Contract", "Acceptance"] {
        text.push_str(&render_section(
            section,
            body.sections.get(section).map(String::as_str).unwrap_or(""),
        ));
    }
    let mut rendered = BTreeSet::from([
        "Goal".to_string(),
        "Implementation Contract".to_string(),
        "Acceptance".to_string(),
    ]);
    for section in &body.order {
        if rendered.insert(section.clone()) {
            text.push_str(&render_section(
                section,
                body.sections.get(section).map(String::as_str).unwrap_or(""),
            ));
        }
    }
    Ok(text)
}

fn render_section(title: &str, content: &str) -> String {
    format!("\n## {title}\n\n{}\n", content.trim())
}

fn split_frontmatter(text: &str) -> std::result::Result<(&str, &str), String> {
    let rest = text
        .strip_prefix("---\n")
        .ok_or_else(|| "missing YAML frontmatter".to_string())?;
    let marker = "\n---\n";
    let end = rest
        .find(marker)
        .ok_or_else(|| "unterminated YAML frontmatter".to_string())?;
    Ok((&rest[..end], &rest[end + marker.len()..]))
}

fn parse_body(body: &str) -> ParsedBody {
    let mut sections = BTreeMap::new();
    let mut order = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_lines = Vec::new();
    for line in body.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            flush_section(
                &mut sections,
                &mut order,
                current_title.take(),
                &mut current_lines,
            );
            current_title = Some(title.trim().to_string());
        } else if current_title.is_some() {
            current_lines.push(line.to_string());
        }
    }
    flush_section(&mut sections, &mut order, current_title, &mut current_lines);
    ParsedBody { sections, order }
}

fn flush_section(
    sections: &mut BTreeMap<String, String>,
    order: &mut Vec<String>,
    title: Option<String>,
    lines: &mut Vec<String>,
) {
    let Some(title) = title else {
        return;
    };
    if !sections.contains_key(&title) {
        order.push(title.clone());
    }
    sections.insert(title, lines.join("\n").trim().to_string());
    lines.clear();
}

fn ensure_section_order(order: &mut Vec<String>, section: &str) {
    if !order.iter().any(|value| value == section) {
        order.push(section.to_string());
    }
}

fn failed_with_next<T: serde::Serialize + schemars::JsonSchema>(
    action: &str,
    summary: impl Into<String>,
    error: impl Into<String>,
    next_action: impl Into<String>,
) -> ActionResult<T> {
    let next_action = next_action.into();
    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Failed,
        summary: summary.into(),
        next_action: Some(next_action.clone()),
        recovery_action: Some(next_action),
        data: None,
        error: Some(error.into()),
    }
}

fn backlog_validation_data(
    root: &Path,
    validation: &BacklogValidation,
    include_errors: bool,
) -> BacklogValidationData {
    BacklogValidationData {
        root: root.display().to_string(),
        ok: validation.ok,
        item_count: validation.items.len(),
        epic_count: validation.epic_ids.len(),
        errors: if include_errors {
            validation.errors.clone()
        } else {
            Vec::new()
        },
    }
}

fn format_existing_validation_error(validation: &BacklogValidation) -> String {
    let mut message = format!(
        "existing backlog has {} validation issue(s)",
        validation.errors.len()
    );
    if !validation.errors.is_empty() {
        message.push_str(": ");
        message.push_str(&validation.errors.join("\n"));
    }
    message
}

fn normalize_item_id(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn normalize_item_type(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "backlog" | "story" | "user_story" | "task" | "chore" => "feature".to_string(),
        value => value.to_string(),
    }
}

fn clean_vec(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn clean_item_ids(values: &[String]) -> Vec<String> {
    clean_vec(values)
        .into_iter()
        .map(|value| value.to_ascii_uppercase())
        .collect()
}
