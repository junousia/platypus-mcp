use super::{
    filesystem::{read_markdown_paths, resolve_root},
    parse::{parse_backlog_item, parse_epic},
    types::{
        BacklogValidation, ParsedBacklogItem, REQUIRED_SECTIONS, VALID_EPIC_STATUSES,
        VALID_EXECUTION_PATHS, VALID_PLANNING_GATES, VALID_PRIORITIES, VALID_TYPES,
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
        ActionResult {
            action: action.to_string(),
            status: crate::models::ActionStatus::Completed,
            summary: format!(
                "Backlog valid: {} item(s), {} epic(s).",
                data.item_count, data.epic_count
            ),
            next_action: Some(
                "Commit backlog artifacts before dispatching work if this validation followed writes."
                    .to_string(),
            ),
            recovery_action: None,
            data: Some(data),
            error: None,
        }
    } else {
        ActionResult {
            action: action.to_string(),
            status: crate::models::ActionStatus::Failed,
            summary: format!(
                "Backlog has {} validation issue(s).",
                validation.errors.len()
            ),
            next_action: Some("Fix backlog frontmatter, sections, or dependencies.".to_string()),
            recovery_action: None,
            data: Some(data),
            error: Some(validation.errors.join("\n")),
        }
    }
}

pub(super) fn validate_backlog_at_root(root: &Path, include_errors: bool) -> BacklogValidation {
    let mut errors = Vec::new();
    let mut epic_ids = BTreeSet::new();
    let expected_prefix = configured_id_prefix(root);
    let epics_dir = root.join("backlog").join("epics");
    if epics_dir.is_dir() {
        match read_markdown_paths(root, &epics_dir) {
            Ok(paths) => {
                for path in paths {
                    match parse_epic(&path) {
                        Ok(epic) => {
                            if epic.id.trim().is_empty()
                                || epic.title.trim().is_empty()
                                || epic.area.trim().is_empty()
                            {
                                errors
                                    .push(format!("{}: invalid epic frontmatter", path.display()));
                            }
                            if !VALID_EPIC_STATUSES.contains(&epic.status.as_str()) {
                                errors.push(format!(
                                    "{}: invalid epic status `{}`; expected one of: {}",
                                    path.display(),
                                    epic.status,
                                    VALID_EPIC_STATUSES.join(", ")
                                ));
                            }
                            if !VALID_PRIORITIES.contains(&epic.priority.as_str()) {
                                errors.push(format!(
                                    "{}: invalid epic priority `{}`; expected one of: {}",
                                    path.display(),
                                    epic.priority,
                                    VALID_PRIORITIES.join(", ")
                                ));
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
                            validate_item_shape(&item, &epic_ids, &expected_prefix, &mut errors);
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
    expected_prefix: &str,
    errors: &mut Vec<String>,
) {
    let frontmatter = &item.frontmatter;
    let path = &item.path;
    if !valid_item_id(&frontmatter.id) {
        errors.push(format!(
            "{}: invalid id `{}`; expected format `{}-NNN` (three digits, matching backlog.id_prefix in platy.yaml)",
            path.display(),
            frontmatter.id,
            expected_prefix
        ));
    }
    if path.file_stem().and_then(|value| value.to_str()) != Some(frontmatter.id.as_str()) {
        errors.push(format!(
            "{}: filename does not match frontmatter id `{}`; rename the file to `{}.md` or update the frontmatter id",
            path.display(),
            frontmatter.id,
            frontmatter.id
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
            "{}: invalid type `{}`; expected one of: {}",
            path.display(),
            frontmatter.item_type,
            VALID_TYPES.join(", ")
        ));
    }
    if let Some(execution_path) = &frontmatter.execution_path {
        if !VALID_EXECUTION_PATHS.contains(&execution_path.as_str()) {
            errors.push(format!(
                "{}: invalid execution_path `{}`; expected one of: {}",
                path.display(),
                execution_path,
                VALID_EXECUTION_PATHS.join(", ")
            ));
        }
    }
    if let Some(planning_gate) = &frontmatter.planning_gate {
        if !VALID_PLANNING_GATES.contains(&planning_gate.as_str()) {
            errors.push(format!(
                "{}: invalid planning_gate `{}`; expected one of: {}",
                path.display(),
                planning_gate,
                VALID_PLANNING_GATES.join(", ")
            ));
        }
    }
    if !epic_ids.is_empty() && !epic_ids.contains(&frontmatter.epic) {
        errors.push(format!(
            "{}: unknown epic `{}`",
            path.display(),
            frontmatter.epic
        ));
    }
    for reference in &frontmatter.external_refs {
        if reference.provider.trim().is_empty()
            || reference.kind.trim().is_empty()
            || reference.id.trim().is_empty()
        {
            errors.push(format!(
                "{}: external_refs provider, kind, and id are required",
                path.display()
            ));
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
            errors.push(format!(
                "{}: external_refs require url or locator",
                path.display()
            ));
        }
    }
    for section in REQUIRED_SECTIONS {
        if !item.sections.contains(*section) {
            errors.push(format!("{}: missing section `{}`", path.display(), section));
        }
    }
}

fn configured_id_prefix(root: &Path) -> String {
    let path = root.join("platy.yaml");
    let Ok(text) = std::fs::read_to_string(path) else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn validation_errors_explain_expected_id_prefix_and_filename() {
        let project = TempDir::new().expect("temp dir");
        fs::create_dir_all(project.path().join("backlog/items")).expect("items");
        fs::create_dir_all(project.path().join("backlog/epics")).expect("epics");
        fs::write(
            project.path().join("platy.yaml"),
            "backlog:\n  id_prefix: PROJ\n",
        )
        .expect("config");
        fs::write(
            project.path().join("backlog/epics/general.md"),
            "---\nid: general\ntitle: General\npriority: P1\nstatus: active\narea: general\n---\n",
        )
        .expect("epic");
        fs::write(
            project.path().join("backlog/items/WEB-2.md"),
            "---\nid: WEB-2\ntitle: Bad\npriority: P1\ntype: feature\narea: general\nepic: general\ndepends_on: []\nowned_surfaces: []\n---\n\n# Bad\n\n## Goal\n\nBad.\n\n## Implementation Contract\n\nBad.\n\n## Acceptance\n\n- Bad.\n",
        )
        .expect("item");
        fs::write(
            project.path().join("backlog/items/WRONG-NAME.md"),
            "---\nid: PROJ-002\ntitle: Mismatch\npriority: P1\ntype: feature\narea: general\nepic: general\ndepends_on: []\nowned_surfaces: []\n---\n\n# Mismatch\n\n## Goal\n\nMismatch.\n\n## Implementation Contract\n\nMismatch.\n\n## Acceptance\n\n- Mismatch.\n",
        )
        .expect("mismatch item");

        let validation = validate_backlog_at_root(project.path(), true);

        assert!(!validation.ok);
        let errors = validation.errors.join("\n");
        assert!(errors.contains("expected format `PROJ-NNN`"));
        assert!(errors.contains("rename the file"));
    }

    #[test]
    fn validation_rejects_invalid_epic_status_and_priority() {
        let project = TempDir::new().expect("temp dir");
        fs::create_dir_all(project.path().join("backlog/items")).expect("items");
        fs::create_dir_all(project.path().join("backlog/epics")).expect("epics");
        fs::write(
            project.path().join("backlog/epics/general.md"),
            "---\nid: general\ntitle: General\npriority: urgent\nstatus: open\narea: general\n---\n",
        )
        .expect("epic");

        let validation = validate_backlog_at_root(project.path(), true);

        assert!(!validation.ok);
        let errors = validation.errors.join("\n");
        assert!(errors.contains("invalid epic status `open`; expected one of: active, archived"));
        assert!(errors.contains("invalid epic priority `urgent`; expected one of: P0, P1, P2"));
    }
}
