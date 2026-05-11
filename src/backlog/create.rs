use super::{
    filesystem::{ensure_child_dir, resolve_root},
    types::{BacklogItemFrontmatterOut, ParsedBacklogItem, VALID_PRIORITIES, VALID_TYPES},
    validate::{valid_item_id, validate_backlog_at_root},
};
use crate::models::{
    ActionResult, ActionStatus, CreateBacklogItemParams, CreateBacklogItemsEntry,
    CreateBacklogItemsParams, CreatedBacklogBatchItem, CreatedBacklogItemData,
    CreatedBacklogItemsData,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug)]
struct NormalizedBacklogInput {
    title: String,
    goal: String,
    contract: String,
    acceptance: Vec<String>,
}

#[derive(Debug)]
struct PlannedBatchItem {
    index: usize,
    client_key: Option<String>,
    params: CreateBacklogItemParams,
    depends_on_keys: Vec<String>,
    item_id: String,
}

#[derive(Debug)]
struct PlannedBacklogWrite {
    index: usize,
    client_key: Option<String>,
    item_id: String,
    path: PathBuf,
    text: String,
    depends_on: Vec<String>,
}

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
        return failed_with_next(
            action,
            "Could not create backlog item.",
            error,
            "Run init_project for this root, or create backlog/items inside the project root before creating items.",
        );
    }
    let validation = validate_backlog_at_root(&root, true);
    let configured_prefix = configured_id_prefix(&root);
    let item_id = match params.id.as_deref() {
        Some(id) => normalize_item_id(id),
        None => allocate_item_id(
            &validation.items,
            params.id_prefix.as_deref().unwrap_or(&configured_prefix),
        ),
    };
    if !valid_item_id(&item_id) {
        return failed_with_next(
            action,
            "Could not create backlog item.",
            format!(
                "invalid backlog id `{}`; expected format `{}-NNN` with three digits",
                item_id, configured_prefix
            ),
            format!(
                "Use an ID such as `{configured_prefix}-001`, or omit id and let Platypus allocate the next `{configured_prefix}-NNN` value."
            ),
        );
    }
    let item_path = items_dir.join(format!("{}.md", item_id));
    if let Some(kind) = existing_path_kind(&item_path) {
        return failed_with_next(
            action,
            format!("Could not create backlog item {}.", item_id),
            format!(
                "backlog item target already exists as {kind} at {}",
                item_path.display()
            ),
            format!(
                "Omit id to allocate the next `{configured_prefix}-NNN` value, choose a different id, or inspect `{}` before editing it.",
                item_path.display()
            ),
        );
    }
    let epic = clean_optional(params.epic.clone()).unwrap_or_else(|| "general".to_string());
    if !validation.epic_ids.is_empty() && !validation.epic_ids.contains(&epic) {
        return failed_with_next(
            action,
            format!("Could not create backlog item {}.", item_id),
            unknown_epic_error(&epic, &validation.epic_ids),
            unknown_epic_next_action(&epic, &validation.epic_ids),
        );
    }
    let priority = clean_optional(params.priority.clone()).unwrap_or_else(|| "P1".to_string());
    let item_type = normalize_item_type(params.item_type.as_deref());
    if !VALID_PRIORITIES.contains(&priority.as_str()) {
        return failed_with_next(
            action,
            "Could not create backlog item.",
            format!(
                "invalid priority `{}`; expected one of: {}",
                priority,
                VALID_PRIORITIES.join(", ")
            ),
            format!("Set priority to one of: {}.", VALID_PRIORITIES.join(", ")),
        );
    }
    if !VALID_TYPES.contains(&item_type.as_str()) {
        return failed_with_next(
            action,
            "Could not create backlog item.",
            format!(
                "invalid type `{}`; expected one of: {}",
                item_type,
                VALID_TYPES.join(", ")
            ),
            format!("Set type to one of: {}.", VALID_TYPES.join(", ")),
        );
    }
    if let Err(error) = validate_external_refs(&params.external_refs) {
        return failed_with_next(
            action,
            "Could not create backlog item.",
            error,
            "For each external_ref, provide provider, kind, id, and either url or locator.",
        );
    }
    let existing_ids = existing_item_ids(&validation.items);
    if let Err(error) = validate_dependencies(&item_id, &params.depends_on, &existing_ids) {
        return failed_with_next(
            action,
            "Could not create backlog item.",
            error,
            "Create the missing dependency item first, remove it from depends_on, or reference an existing backlog item ID.",
        );
    }
    let normalized = match normalize_backlog_input(&params) {
        Ok(normalized) => normalized,
        Err(missing_fields) => {
            return failed_with_next(
                action,
                "Could not create backlog item.",
                format!(
                    "missing required field(s): {}. Provide at least title or goal; explicit fields can still override derived defaults. Required persisted fields: title, goal, implementation_contract or contract, and at least one acceptance criterion.",
                    missing_fields.join(", ")
                ),
                "Provide title or goal. Platypus can derive implementation_contract and acceptance from that minimal input when explicit values are omitted.",
            );
        }
    };
    let text = match backlog_item_text(&item_id, &params, &normalized, &priority, &item_type, &epic)
    {
        Ok(text) => text,
        Err(error) => return ActionResult::failed(action, "Could not create backlog item.", error),
    };
    if let Err(error) = write_new_file(&item_path, &text) {
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

pub fn create_backlog_items(
    default_root: &Path,
    params: CreateBacklogItemsParams,
) -> ActionResult<CreatedBacklogItemsData> {
    let action = "create_backlog_items";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => {
            return ActionResult::failed(action, "Could not create backlog items.", error)
        }
    };
    let items_dir = root.join("backlog").join("items");
    if let Err(error) = ensure_child_dir(&root, &items_dir) {
        return failed_batch(
            action,
            "Could not create backlog items.",
            error,
            "Run init_project for this root, or create backlog/items inside the project root before creating items.",
        );
    }
    if params.items.is_empty() {
        return failed_batch(
            action,
            "Could not create backlog items.",
            "items must contain at least one backlog item",
            "Provide one or more backlog item entries.",
        );
    }

    let validation = validate_backlog_at_root(&root, true);
    let configured_prefix = configured_id_prefix(&root);
    let batch_prefix = params.id_prefix.clone();
    let mut used_ids = existing_item_ids(&validation.items);
    let mut key_to_id = BTreeMap::new();
    let mut planned = Vec::with_capacity(params.items.len());

    for (index, item) in params.items.into_iter().enumerate() {
        let client_key = clean_optional(item.client_key.clone());
        if let Some(key) = &client_key {
            if key_to_id.contains_key(key) {
                return failed_batch_item(
                    action,
                    index,
                    client_key.as_deref(),
                    "duplicate client_key in batch",
                    "Use unique client_key values, or omit client_key for items that are not referenced by depends_on_keys.",
                );
            }
        }
        let id_prefix = item
            .id_prefix
            .as_deref()
            .or(batch_prefix.as_deref())
            .unwrap_or(&configured_prefix);
        let item_id = match item.id.as_deref() {
            Some(id) => normalize_item_id(id),
            None => allocate_item_id_from_ids(&used_ids, id_prefix),
        };
        if !valid_item_id(&item_id) {
            return failed_batch_item(
                action,
                index,
                client_key.as_deref(),
                format!(
                    "invalid backlog id `{}`; expected format `{}-NNN` with three digits",
                    item_id, configured_prefix
                ),
                format!(
                    "Use an ID such as `{configured_prefix}-001`, or omit id and let Platypus allocate the next `{configured_prefix}-NNN` value."
                ),
            );
        }
        if !used_ids.insert(item_id.clone()) {
            return failed_batch_item(
                action,
                index,
                client_key.as_deref(),
                format!("duplicate backlog id `{item_id}`"),
                "Choose a different explicit id or omit id so Platypus can allocate it.",
            );
        }
        if let Some(key) = &client_key {
            key_to_id.insert(key.clone(), item_id.clone());
        }
        planned.push(PlannedBatchItem {
            index,
            client_key,
            depends_on_keys: item.depends_on_keys.clone(),
            params: batch_item_params(item, item_id.clone()),
            item_id,
        });
    }

    let mut writes = Vec::with_capacity(planned.len());
    for planned_item in planned {
        let mut params = planned_item.params;
        let resolved_depends_on_keys =
            match resolve_dependency_keys(&planned_item.depends_on_keys, &key_to_id) {
                Ok(dependencies) => dependencies,
                Err(error) => return failed_batch_item(
                    action,
                    planned_item.index,
                    planned_item.client_key.as_deref(),
                    error,
                    "Use depends_on_keys only for client_key values present in this same batch.",
                ),
            };
        params.depends_on = merge_dependencies(params.depends_on, resolved_depends_on_keys);
        let item_path = items_dir.join(format!("{}.md", planned_item.item_id));
        if let Some(kind) = existing_path_kind(&item_path) {
            return failed_batch_item(
                action,
                planned_item.index,
                planned_item.client_key.as_deref(),
                format!(
                    "backlog item target already exists as {kind} at {}",
                    item_path.display()
                ),
                "Choose a different explicit id, remove the existing target, or omit id so Platypus can allocate the next value.",
            );
        }
        let epic = clean_optional(params.epic.clone()).unwrap_or_else(|| "general".to_string());
        if !validation.epic_ids.is_empty() && !validation.epic_ids.contains(&epic) {
            return failed_batch_item(
                action,
                planned_item.index,
                planned_item.client_key.as_deref(),
                unknown_epic_error(&epic, &validation.epic_ids),
                unknown_epic_next_action(&epic, &validation.epic_ids),
            );
        }
        let priority = clean_optional(params.priority.clone()).unwrap_or_else(|| "P1".to_string());
        let item_type = normalize_item_type(params.item_type.as_deref());
        if !VALID_PRIORITIES.contains(&priority.as_str()) {
            return failed_batch_item(
                action,
                planned_item.index,
                planned_item.client_key.as_deref(),
                format!(
                    "invalid priority `{}`; expected one of: {}",
                    priority,
                    VALID_PRIORITIES.join(", ")
                ),
                format!("Set priority to one of: {}.", VALID_PRIORITIES.join(", ")),
            );
        }
        if !VALID_TYPES.contains(&item_type.as_str()) {
            return failed_batch_item(
                action,
                planned_item.index,
                planned_item.client_key.as_deref(),
                format!(
                    "invalid type `{}`; expected one of: {}",
                    item_type,
                    VALID_TYPES.join(", ")
                ),
                format!("Set type to one of: {}.", VALID_TYPES.join(", ")),
            );
        }
        if let Err(error) = validate_external_refs(&params.external_refs) {
            return failed_batch_item(
                action,
                planned_item.index,
                planned_item.client_key.as_deref(),
                error,
                "For each external_ref, provide provider, kind, id, and either url or locator.",
            );
        }
        if let Err(error) =
            validate_dependencies(&planned_item.item_id, &params.depends_on, &used_ids)
        {
            return failed_batch_item(
                action,
                planned_item.index,
                planned_item.client_key.as_deref(),
                error,
                "Create the missing dependency item first, remove it from depends_on, or reference an existing backlog item ID or batch client_key.",
            );
        }
        let normalized = match normalize_backlog_input(&params) {
            Ok(normalized) => normalized,
            Err(missing_fields) => {
                return failed_batch_item(
                    action,
                    planned_item.index,
                    planned_item.client_key.as_deref(),
                    format!(
                        "missing required field(s): {}. Provide at least title or goal; explicit fields can still override derived defaults. Required persisted fields: title, goal, implementation_contract or contract, and at least one acceptance criterion.",
                        missing_fields.join(", ")
                    ),
                    "Provide title or goal. Platypus can derive implementation_contract and acceptance from that minimal input when explicit values are omitted.",
                )
            }
        };
        let text = match backlog_item_text(
            &planned_item.item_id,
            &params,
            &normalized,
            &priority,
            &item_type,
            &epic,
        ) {
            Ok(text) => text,
            Err(error) => {
                return ActionResult::failed(action, "Could not create backlog items.", error)
            }
        };
        writes.push(PlannedBacklogWrite {
            index: planned_item.index,
            client_key: planned_item.client_key,
            item_id: planned_item.item_id,
            path: item_path,
            text,
            depends_on: clean_vec(params.depends_on),
        });
    }
    if let Some(cycle) = batch_dependency_cycle(&writes) {
        let first_id = cycle.first().map(String::as_str);
        let write =
            first_id.and_then(|item_id| writes.iter().find(|write| write.item_id == item_id));
        return failed_batch_item(
            action,
            write.map(|write| write.index).unwrap_or(0),
            write.and_then(|write| write.client_key.as_deref()),
            format!("cyclic batch dependency: {}", cycle.join(" -> ")),
            "Remove or reverse one depends_on or depends_on_keys edge so the batch dependency graph is acyclic.",
        );
    }

    let mut written_paths = Vec::new();
    for write in &writes {
        if let Err(error) = write_new_file(&write.path, &write.text) {
            rollback_written_files(&written_paths);
            return failed_batch_item(
                action,
                write.index,
                write.client_key.as_deref(),
                error,
                "No batch item files were kept. Inspect the target path and retry the batch.",
            );
        }
        written_paths.push(write.path.clone());
    }

    let items = writes
        .into_iter()
        .map(|write| CreatedBacklogBatchItem {
            client_key: write.client_key,
            item_id: write.item_id,
            path: write.path.display().to_string(),
            depends_on: write.depends_on,
            created: true,
        })
        .collect::<Vec<_>>();
    ActionResult::completed(
        action,
        format!("Created {} backlog item(s).", items.len()),
        CreatedBacklogItemsData {
            root: root.display().to_string(),
            created: items.len(),
            items,
        },
    )
}

fn failed_with_next<T: serde::Serialize + schemars::JsonSchema>(
    action: &str,
    summary: impl Into<String>,
    error: impl Into<String>,
    next_action: impl Into<String>,
) -> ActionResult<T> {
    ActionResult {
        action: action.to_string(),
        status: ActionStatus::Failed,
        summary: summary.into(),
        next_action: Some(next_action.into()),
        data: None,
        error: Some(error.into()),
    }
}

fn failed_batch(
    action: &str,
    summary: impl Into<String>,
    error: impl Into<String>,
    next_action: impl Into<String>,
) -> ActionResult<CreatedBacklogItemsData> {
    failed_with_next(action, summary, error, next_action)
}

fn failed_batch_item(
    action: &str,
    index: usize,
    client_key: Option<&str>,
    error: impl Into<String>,
    next_action: impl Into<String>,
) -> ActionResult<CreatedBacklogItemsData> {
    let item = match client_key {
        Some(key) => format!("item[{index}] client_key `{key}`"),
        None => format!("item[{index}]"),
    };
    failed_with_next(
        action,
        format!("Could not create backlog items; {item} failed."),
        format!("{item}: {}", error.into()),
        format!("No batch item files were written. {}", next_action.into()),
    )
}

fn normalize_item_id(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn allocate_item_id(items: &[ParsedBacklogItem], preferred_prefix: &str) -> String {
    allocate_item_id_from_ids(&existing_item_ids(items), preferred_prefix)
}

fn allocate_item_id_from_ids(existing_ids: &BTreeSet<String>, preferred_prefix: &str) -> String {
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
    for item_id in existing_ids {
        if let Some(number) = item_id
            .strip_prefix(prefix)
            .and_then(|suffix| suffix.strip_prefix('-'))
            .and_then(|number| number.parse::<u32>().ok())
        {
            max_number = max_number.max(number);
        }
    }
    format!("{}-{:03}", prefix, max_number + 1)
}

fn existing_item_ids(items: &[ParsedBacklogItem]) -> BTreeSet<String> {
    items
        .iter()
        .map(|item| item.frontmatter.id.clone())
        .collect()
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_item_type(value: Option<&str>) -> String {
    match value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("feature")
        .to_ascii_lowercase()
        .as_str()
    {
        "backlog" | "story" | "user_story" | "task" | "chore" => "feature".to_string(),
        value => value.to_string(),
    }
}

fn configured_id_prefix(root: &Path) -> String {
    let path = root.join("platy.yaml");
    let Ok(text) = fs::read_to_string(path) else {
        return "PROJ".to_string();
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&text) else {
        return "PROJ".to_string();
    };
    value
        .get("backlog")
        .and_then(|backlog| backlog.get("id_prefix"))
        .and_then(serde_yaml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("PROJ")
        .to_ascii_uppercase()
}

fn clean_vec(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn validate_dependencies(
    item_id: &str,
    depends_on: &[String],
    existing_ids: &BTreeSet<String>,
) -> Result<(), String> {
    let requested = clean_vec(depends_on.to_vec());
    for dependency in requested {
        if dependency == item_id {
            return Err(format!(
                "invalid dependency `{dependency}`; backlog item `{item_id}` cannot depend on itself"
            ));
        }
        if !existing_ids.contains(&dependency) {
            return Err(format!(
                "unknown dependency `{dependency}`; existing backlog items: {}",
                existing_item_hint(&existing_ids)
            ));
        }
    }
    Ok(())
}

fn existing_item_hint(existing_ids: &BTreeSet<String>) -> String {
    if existing_ids.is_empty() {
        return "none".to_string();
    }
    let shown = existing_ids.iter().take(8).cloned().collect::<Vec<_>>();
    let suffix = if existing_ids.len() > shown.len() {
        format!(" and {} more", existing_ids.len() - shown.len())
    } else {
        String::new()
    };
    format!("{}{}", shown.join(", "), suffix)
}

fn batch_item_params(item: CreateBacklogItemsEntry, item_id: String) -> CreateBacklogItemParams {
    CreateBacklogItemParams {
        root: None,
        id: Some(item_id),
        id_prefix: item.id_prefix,
        title: item.title,
        priority: item.priority,
        item_type: item.item_type,
        area: item.area,
        epic: item.epic,
        depends_on: item.depends_on,
        suggested_worker: item.suggested_worker,
        owned_surfaces: item.owned_surfaces,
        external_refs: item.external_refs,
        goal: item.goal,
        implementation_contract: item.implementation_contract,
        contract: item.contract,
        acceptance: item.acceptance,
        notes: item.notes,
    }
}

fn resolve_dependency_keys(
    values: &[String],
    key_to_id: &BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    let mut dependencies = Vec::new();
    for key in clean_vec(values.to_vec()) {
        let Some(item_id) = key_to_id.get(&key) else {
            return Err(format!("unknown dependency client_key `{key}`"));
        };
        dependencies.push(item_id.clone());
    }
    Ok(dependencies)
}

fn merge_dependencies(existing: Vec<String>, resolved: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();
    for dependency in clean_vec(existing).into_iter().chain(resolved) {
        if seen.insert(dependency.clone()) {
            merged.push(dependency);
        }
    }
    merged
}

fn batch_dependency_cycle(writes: &[PlannedBacklogWrite]) -> Option<Vec<String>> {
    let planned_ids = writes
        .iter()
        .map(|write| write.item_id.clone())
        .collect::<BTreeSet<_>>();
    let edges = writes
        .iter()
        .map(|write| {
            let dependencies = write
                .depends_on
                .iter()
                .filter(|dependency| planned_ids.contains(*dependency))
                .cloned()
                .collect::<Vec<_>>();
            (write.item_id.clone(), dependencies)
        })
        .collect::<BTreeMap<_, _>>();
    let mut states = BTreeMap::<String, u8>::new();
    let mut stack = Vec::new();
    for item_id in planned_ids {
        if let Some(cycle) = visit_batch_dependency(&item_id, &edges, &mut states, &mut stack) {
            return Some(cycle);
        }
    }
    None
}

fn visit_batch_dependency(
    item_id: &str,
    edges: &BTreeMap<String, Vec<String>>,
    states: &mut BTreeMap<String, u8>,
    stack: &mut Vec<String>,
) -> Option<Vec<String>> {
    match states.get(item_id).copied().unwrap_or(0) {
        1 => {
            let start = stack
                .iter()
                .position(|stack_id| stack_id == item_id)
                .unwrap_or(0);
            let mut cycle = stack[start..].to_vec();
            cycle.push(item_id.to_string());
            Some(cycle)
        }
        2 => None,
        _ => {
            states.insert(item_id.to_string(), 1);
            stack.push(item_id.to_string());
            for dependency in edges.get(item_id).into_iter().flatten() {
                if let Some(cycle) = visit_batch_dependency(dependency, edges, states, stack) {
                    return Some(cycle);
                }
            }
            stack.pop();
            states.insert(item_id.to_string(), 2);
            None
        }
    }
}

fn unknown_epic_error(epic: &str, epic_ids: &BTreeSet<String>) -> String {
    format!(
        "unknown epic `{epic}`; existing epics: {}",
        existing_epic_hint(epic_ids)
    )
}

fn unknown_epic_next_action(epic: &str, epic_ids: &BTreeSet<String>) -> String {
    if safe_epic_id(epic) {
        format!(
            "Call create_epic for `{epic}` (or create `backlog/epics/{epic}.md` manually), or set epic to one of the existing epics from list_epics: {}.",
            existing_epic_hint(epic_ids)
        )
    } else {
        format!(
            "Set epic to one of the existing epics from list_epics: {}, or choose a simple epic id containing only letters, digits, `_`, or `-` before calling create_epic.",
            existing_epic_hint(epic_ids)
        )
    }
}

fn existing_epic_hint(epic_ids: &BTreeSet<String>) -> String {
    if epic_ids.is_empty() {
        return "none".to_string();
    }
    let shown = epic_ids.iter().take(8).cloned().collect::<Vec<_>>();
    let suffix = if epic_ids.len() > shown.len() {
        format!(" and {} more", epic_ids.len() - shown.len())
    } else {
        String::new()
    };
    format!("{}{}", shown.join(", "), suffix)
}

fn safe_epic_id(epic: &str) -> bool {
    let epic = epic.trim();
    !epic.is_empty()
        && !matches!(epic, "." | "..")
        && epic
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn normalize_backlog_input(
    params: &CreateBacklogItemParams,
) -> Result<NormalizedBacklogInput, Vec<&'static str>> {
    let explicit_title = clean_text(&params.title);
    let explicit_goal = clean_text(&params.goal);
    if explicit_title.is_none() && explicit_goal.is_none() {
        return Err(vec![
            "title",
            "goal",
            "implementation_contract|contract",
            "acceptance",
        ]);
    }

    let title = explicit_title
        .or_else(|| explicit_goal.as_deref().and_then(derive_title))
        .ok_or_else(|| vec!["title"])?;
    let goal = explicit_goal.unwrap_or_else(|| with_period(&title));
    let contract = params
        .implementation_contract
        .as_deref()
        .or(params.contract.as_deref())
        .and_then(clean_text)
        .unwrap_or_else(|| format!("Implement the requested change: {}", with_period(&goal)));
    let mut acceptance = clean_vec(params.acceptance.clone());
    if acceptance.is_empty() {
        acceptance.push(format!(
            "{} is implemented and verification notes are recorded.",
            title
        ));
    }

    Ok(NormalizedBacklogInput {
        title,
        goal,
        contract,
        acceptance,
    })
}

fn clean_text(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn derive_title(value: &str) -> Option<String> {
    let line = value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '#' | '-' | '*' | ':')
        })
        .trim();
    clean_text(&limit_title(line))
}

fn limit_title(value: &str) -> String {
    const MAX_CHARS: usize = 80;
    let mut chars = value.chars();
    let mut title = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        title.push_str("...");
    }
    title
}

fn with_period(value: impl AsRef<str>) -> String {
    let value = value.as_ref().trim();
    if value.ends_with(['.', '!', '?']) {
        value.to_string()
    } else {
        format!("{value}.")
    }
}

fn backlog_item_text(
    item_id: &str,
    params: &CreateBacklogItemParams,
    normalized: &NormalizedBacklogInput,
    priority: &str,
    item_type: &str,
    epic: &str,
) -> std::result::Result<String, String> {
    let frontmatter = BacklogItemFrontmatterOut {
        id: item_id.to_string(),
        title: normalized.title.clone(),
        priority: priority.to_string(),
        item_type: item_type.to_string(),
        area: clean_optional(params.area.clone()).unwrap_or_else(|| "tooling".to_string()),
        epic: epic.to_string(),
        depends_on: clean_vec(params.depends_on.clone()),
        suggested_worker: clean_optional(params.suggested_worker.clone())
            .or_else(|| Some("coder".to_string())),
        owned_surfaces: clean_vec(params.owned_surfaces.clone()),
        external_refs: params.external_refs.clone(),
    };
    let yaml = serde_yaml::to_string(&frontmatter).map_err(|error| error.to_string())?;
    let mut body = format!(
        "---\n{}---\n\n# {} {}\n\n## Goal\n\n{}\n\n## Implementation Contract\n\n{}\n\n## Acceptance\n\n{}\n",
        yaml,
        item_id,
        normalized.title,
        normalized.goal,
        normalized.contract,
        normalized
            .acceptance
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

fn validate_external_refs(refs: &[crate::models::ExternalRef]) -> Result<(), String> {
    for reference in refs {
        if reference.provider.trim().is_empty()
            || reference.kind.trim().is_empty()
            || reference.id.trim().is_empty()
        {
            return Err("external_refs provider, kind, and id are required".to_string());
        }
        let has_location = reference
            .url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
            || reference
                .locator
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some();
        if !has_location {
            return Err("external_refs require url or locator".to_string());
        }
    }
    Ok(())
}

fn existing_path_kind(path: &Path) -> Option<&'static str> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        Some("symlink")
    } else if file_type.is_dir() {
        Some("directory")
    } else {
        Some("file")
    }
}

fn write_new_file(path: &Path, text: &str) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    if let Err(error) = file.write_all(text.as_bytes()) {
        let _ = fs::remove_file(path);
        return Err(error.to_string());
    }
    Ok(())
}

fn rollback_written_files(paths: &[PathBuf]) {
    for path in paths.iter().rev() {
        let _ = fs::remove_file(path);
    }
}
