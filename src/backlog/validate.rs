use super::{
    filesystem::{read_markdown_paths, resolve_root},
    parse::{parse_backlog_item, parse_epic},
    types::{
        BacklogValidation, ParsedBacklogItem, REQUIRED_SECTIONS, VALID_PRIORITIES, VALID_TYPES,
    },
};
use crate::models::{ActionResult, BacklogValidationData};
use std::{collections::BTreeSet, path::Path};

pub fn validate_backlog(
    default_root: &Path,
    root: Option<&str>,
    include_errors: bool,
) -> ActionResult<BacklogValidationData> {
    let action = "validate_backlog";
    let root = match resolve_root(default_root, root) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not validate backlog.", error),
    };
    let validation = validate_backlog_at_root(&root, include_errors);
    let data = BacklogValidationData {
        root: root.display().to_string(),
        ok: validation.ok,
        item_count: validation.items.len(),
        epic_count: validation.epic_ids.len(),
        errors: if include_errors {
            validation.errors.clone()
        } else {
            Vec::new()
        },
    };
    if validation.ok {
        ActionResult::completed(
            action,
            format!(
                "Backlog valid: {} item(s), {} epic(s).",
                data.item_count, data.epic_count
            ),
            data,
        )
    } else {
        ActionResult {
            action: action.to_string(),
            status: crate::models::ActionStatus::Failed,
            summary: format!(
                "Backlog has {} validation issue(s).",
                validation.errors.len()
            ),
            next_action: Some("Fix backlog frontmatter, sections, or dependencies.".to_string()),
            data: Some(data),
            error: Some(validation.errors.join("\n")),
        }
    }
}

pub(super) fn validate_backlog_at_root(root: &Path, include_errors: bool) -> BacklogValidation {
    let mut errors = Vec::new();
    let mut epic_ids = BTreeSet::new();
    let epics_dir = root.join("backlog").join("epics");
    if epics_dir.is_dir() {
        match read_markdown_paths(root, &epics_dir) {
            Ok(paths) => {
                for path in paths {
                    match parse_epic(&path) {
                        Ok(epic) => {
                            if epic.id.trim().is_empty()
                                || epic.title.trim().is_empty()
                                || epic.status.trim().is_empty()
                                || epic.area.trim().is_empty()
                                || !VALID_PRIORITIES.contains(&epic.priority.as_str())
                            {
                                errors
                                    .push(format!("{}: invalid epic frontmatter", path.display()));
                            }
                            epic_ids.insert(epic.id);
                        }
                        Err(error) => errors.push(format!("{}: {}", path.display(), error)),
                    }
                }
            }
            Err(error) => errors.push(error),
        }
    }
    let mut items = Vec::new();
    let items_dir = root.join("backlog").join("items");
    if !items_dir.is_dir() {
        errors.push(format!(
            "{}: backlog items directory missing",
            items_dir.display()
        ));
    } else {
        match read_markdown_paths(root, &items_dir) {
            Ok(paths) => {
                for path in paths {
                    match parse_backlog_item(&path) {
                        Ok(item) => {
                            validate_item_shape(&item, &epic_ids, &mut errors);
                            items.push(item);
                        }
                        Err(error) => errors.push(format!("{}: {}", path.display(), error)),
                    }
                }
            }
            Err(error) => errors.push(error),
        }
    }
    let item_ids: BTreeSet<String> = items
        .iter()
        .map(|item| item.frontmatter.id.clone())
        .collect();
    for item in &items {
        for dependency in &item.frontmatter.depends_on {
            if !item_ids.contains(dependency) {
                errors.push(format!(
                    "{}: unknown dependency `{}`",
                    item.path.display(),
                    dependency
                ));
            }
        }
    }
    BacklogValidation {
        ok: errors.is_empty(),
        errors: if include_errors { errors } else { Vec::new() },
        items,
        epic_ids,
    }
}

pub(super) fn valid_item_id(value: &str) -> bool {
    let Some((prefix, number)) = value.split_once('-') else {
        return false;
    };
    !prefix.is_empty()
        && prefix
            .chars()
            .all(|character| character.is_ascii_uppercase())
        && number.len() == 3
        && number.chars().all(|character| character.is_ascii_digit())
}

fn validate_item_shape(
    item: &ParsedBacklogItem,
    epic_ids: &BTreeSet<String>,
    errors: &mut Vec<String>,
) {
    let frontmatter = &item.frontmatter;
    let path = &item.path;
    if !valid_item_id(&frontmatter.id) {
        errors.push(format!(
            "{}: invalid id `{}`",
            path.display(),
            frontmatter.id
        ));
    }
    if path.file_stem().and_then(|value| value.to_str()) != Some(frontmatter.id.as_str()) {
        errors.push(format!(
            "{}: filename does not match frontmatter id",
            path.display()
        ));
    }
    if frontmatter.title.trim().is_empty()
        || frontmatter.area.trim().is_empty()
        || frontmatter.epic.trim().is_empty()
    {
        errors.push(format!(
            "{}: required frontmatter field is empty",
            path.display()
        ));
    }
    if !VALID_PRIORITIES.contains(&frontmatter.priority.as_str()) {
        errors.push(format!(
            "{}: invalid priority `{}`",
            path.display(),
            frontmatter.priority
        ));
    }
    if !VALID_TYPES.contains(&frontmatter.item_type.as_str()) {
        errors.push(format!(
            "{}: invalid type `{}`",
            path.display(),
            frontmatter.item_type
        ));
    }
    if !epic_ids.is_empty() && !epic_ids.contains(&frontmatter.epic) {
        errors.push(format!(
            "{}: unknown epic `{}`",
            path.display(),
            frontmatter.epic
        ));
    }
    for section in REQUIRED_SECTIONS {
        if !item.sections.contains(*section) {
            errors.push(format!("{}: missing section `{}`", path.display(), section));
        }
    }
}
