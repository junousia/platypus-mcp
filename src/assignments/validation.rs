use std::path::{Component, PathBuf};

pub(crate) fn clean_required(field: &str, value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(trimmed.to_string())
    }
}

pub(crate) fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(crate) fn clean_event_type(value: &str) -> Result<String, String> {
    let value = clean_required("event_type", value)?;
    if value.len() > 80 {
        return Err("event_type must be at most 80 characters".to_string());
    }
    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
    {
        return Err(
            "event_type may only contain ASCII letters, digits, '_', '-', or '.'".to_string(),
        );
    }
    Ok(value)
}

pub(crate) fn clean_terminal_status(value: &str) -> Result<String, String> {
    match value.trim() {
        "completed" | "failed" | "cancelled" => Ok(value.trim().to_string()),
        _ => Err("status must be completed, failed, or cancelled".to_string()),
    }
}

pub(crate) fn clean_changed_files(files: Vec<String>) -> Result<Vec<String>, String> {
    files
        .into_iter()
        .map(|file| clean_relative_path("changed_files", &file))
        .filter(|result| !matches!(result, Ok(value) if value.is_empty()))
        .collect()
}

pub(crate) fn validate_changed_files(
    owned_surfaces: &[String],
    changed_files: &[String],
) -> Result<(), String> {
    if changed_files.is_empty() || owned_surfaces.is_empty() {
        return Ok(());
    }
    let surfaces = owned_surfaces
        .iter()
        .filter_map(|surface| clean_relative_path("owned_surfaces", surface).ok())
        .map(|surface| surface.trim_end_matches('/').to_string())
        .filter(|surface| !surface.is_empty())
        .collect::<Vec<_>>();
    if surfaces.iter().any(|surface| surface == ".") {
        return Ok(());
    }
    for file in changed_files {
        let allowed = surfaces.iter().any(|surface| {
            file == surface
                || file.starts_with(&format!("{surface}/"))
                || same_owned_file_directory(surface, file)
        });
        if !allowed {
            return Err(format!(
                "`{file}` is outside owned surfaces: {}. Recovery: if this change is required for the same task, update the task plan owned_surfaces and prepare a new assignment; if it is separate work, split it into a follow-up backlog item; otherwise discard or revert the out-of-scope change before retrying finish_work.",
                surfaces.join(", ")
            ));
        }
    }
    Ok(())
}

fn same_owned_file_directory(surface: &str, file: &str) -> bool {
    let Some((surface_dir, surface_name)) = surface.rsplit_once('/') else {
        return false;
    };
    if !surface_name.contains('.') {
        return false;
    }
    file.rsplit_once('/')
        .map(|(file_dir, _)| file_dir == surface_dir)
        .unwrap_or(false)
}

fn clean_relative_path(field: &str, value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Err(format!("{field} must contain project-relative paths"));
    }
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(format!("{field} cannot contain path traversal"));
        }
    }
    Ok(path.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_files_allow_auxiliary_files_in_owned_file_directory() {
        let owned = vec!["app/main.py".to_string()];
        let changed = vec!["app/__init__.py".to_string(), "app/main.py".to_string()];

        validate_changed_files(&owned, &changed).expect("same directory auxiliary file");
    }

    #[test]
    fn changed_files_still_reject_unowned_directories() {
        let owned = vec!["app/main.py".to_string()];
        let changed = vec!["tests/test_app.py".to_string()];

        let error = validate_changed_files(&owned, &changed).expect_err("unowned file");
        assert!(error.contains("outside owned surfaces"));
        assert!(error.contains("update the task plan owned_surfaces"));
        assert!(error.contains("split it into a follow-up backlog item"));
        assert!(error.contains("discard or revert"));
    }
}
