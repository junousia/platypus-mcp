use super::{
    filesystem::{ensure_child_dir, read_markdown_paths, resolve_root},
    parse::parse_epic,
    types::{EpicFrontmatterOut, VALID_EPIC_STATUSES, VALID_PRIORITIES},
};
use crate::models::{
    ActionResult, ActionStatus, CreateEpicParams, CreatedEpicData, EpicRecord, ListEpicsData,
    RootParams,
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

pub fn create_epic(default_root: &Path, params: CreateEpicParams) -> ActionResult<CreatedEpicData> {
    let action = "create_epic";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not create epic.", error),
    };
    let epics_dir = root.join("backlog").join("epics");
    if let Err(error) = ensure_child_dir(&root, &epics_dir) {
        return failed_with_next(
            action,
            "Could not create epic.",
            error,
            "Run init_project for this root, or create backlog/epics inside the project root before creating epics.",
        );
    }

    let id = params.id.trim();
    if !valid_epic_id(id) {
        return failed_with_next(
            action,
            "Could not create epic.",
            format!(
                "invalid epic id `{}`; expected only letters, digits, `_`, or `-`",
                params.id
            ),
            "Choose a simple epic id such as `webapp`, `platform`, or `release-1`.",
        );
    }

    let title = params.title.trim();
    if title.is_empty() {
        return failed_with_next(
            action,
            "Could not create epic.",
            "title is required",
            "Provide a human-readable title for the epic.",
        );
    }

    let status = params
        .status
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("active");
    if !VALID_EPIC_STATUSES.contains(&status) {
        return failed_with_next(
            action,
            "Could not create epic.",
            format!(
                "invalid epic status `{status}`; expected one of: {}",
                VALID_EPIC_STATUSES.join(", ")
            ),
            format!("Set status to one of: {}.", VALID_EPIC_STATUSES.join(", ")),
        );
    }

    let priority = params
        .priority
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("P1");
    if !VALID_PRIORITIES.contains(&priority) {
        return failed_with_next(
            action,
            "Could not create epic.",
            format!(
                "invalid priority `{priority}`; expected one of: {}",
                VALID_PRIORITIES.join(", ")
            ),
            format!("Set priority to one of: {}.", VALID_PRIORITIES.join(", ")),
        );
    }

    let area = params
        .area
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(id);
    let path = epics_dir.join(format!("{id}.md"));
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            "symlink"
        } else if file_type.is_dir() {
            "directory"
        } else {
            "file"
        };
        return failed_with_next(
            action,
            format!("Could not create epic {id}."),
            format!("epic target already exists as {kind} at {}", path.display()),
            format!("Use list_epics to inspect existing epics, or choose a different epic id than `{id}`."),
        );
    }

    let frontmatter = EpicFrontmatterOut {
        id: id.to_string(),
        title: title.to_string(),
        status: status.to_string(),
        priority: priority.to_string(),
        area: area.to_string(),
    };
    let yaml = match serde_yaml::to_string(&frontmatter) {
        Ok(yaml) => yaml,
        Err(error) => {
            return ActionResult::failed(action, "Could not create epic.", error.to_string())
        }
    };
    let description = params
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Describe the epic.");
    let text = format!("---\n{yaml}---\n\n# {title}\n\n{description}\n");
    let write_result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut file| file.write_all(text.as_bytes()));
    if let Err(error) = write_result {
        return ActionResult::failed(
            action,
            format!("Could not create epic {id}."),
            error.to_string(),
        );
    }

    ActionResult::completed(
        action,
        format!("Created epic {id}."),
        CreatedEpicData {
            root: root.display().to_string(),
            epic: EpicRecord {
                id: id.to_string(),
                title: title.to_string(),
                status: status.to_string(),
                priority: priority.to_string(),
                area: area.to_string(),
                path: path.display().to_string(),
            },
            created: true,
        },
    )
}

pub fn list_epics(default_root: &Path, params: RootParams) -> ActionResult<ListEpicsData> {
    let action = "list_epics";
    let root = match resolve_root(default_root, params.root.as_deref()) {
        Ok(root) => root,
        Err(error) => return ActionResult::failed(action, "Could not list epics.", error),
    };
    let epics_dir = root.join("backlog").join("epics");
    if let Err(error) = ensure_child_dir(&root, &epics_dir) {
        return failed_with_next(
            action,
            "Could not list epics.",
            error,
            "Run init_project for this root, or create backlog/epics inside the project root before listing epics.",
        );
    }
    let paths = match read_markdown_paths(&root, &epics_dir) {
        Ok(paths) => paths,
        Err(error) => return ActionResult::failed(action, "Could not list epics.", error),
    };
    let mut epics = Vec::new();
    for path in paths {
        let epic = match parse_epic(&path) {
            Ok(epic) => epic,
            Err(error) => {
                return ActionResult::failed(
                    action,
                    "Could not list epics.",
                    format!("{}: {}", path.display(), error),
                )
            }
        };
        epics.push(EpicRecord {
            id: epic.id,
            title: epic.title,
            status: epic.status,
            priority: epic.priority,
            area: epic.area,
            path: path.display().to_string(),
        });
    }
    epics.sort_by(|left, right| left.id.cmp(&right.id));
    let returned = epics.len();
    ActionResult::completed(
        action,
        format!("Listed {returned} epic(s)."),
        ListEpicsData {
            root: root.display().to_string(),
            epics,
            returned,
        },
    )
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

fn valid_epic_id(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}
