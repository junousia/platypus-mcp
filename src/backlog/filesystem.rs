use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn resolve_root(
    default_root: &Path,
    root: Option<&str>,
) -> std::result::Result<PathBuf, String> {
    let requested = root
        .map(PathBuf::from)
        .unwrap_or_else(|| default_root.to_path_buf());
    let canonical = fs::canonicalize(&requested)
        .map_err(|error| format!("{}: {}", requested.display(), error))?;
    if !canonical.is_dir() {
        return Err(format!("{} is not a directory", canonical.display()));
    }
    Ok(canonical)
}

pub(super) fn read_markdown_paths(
    root: &Path,
    directory: &Path,
) -> std::result::Result<Vec<PathBuf>, String> {
    let canonical_dir = fs::canonicalize(directory)
        .map_err(|error| format!("{}: {}", directory.display(), error))?;
    if !canonical_dir.starts_with(root) {
        return Err(format!("{} escapes project root", canonical_dir.display()));
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(&canonical_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            let canonical_path = fs::canonicalize(&path).map_err(|error| error.to_string())?;
            if !canonical_path.starts_with(root) {
                return Err(format!("{} escapes project root", canonical_path.display()));
            }
            paths.push(canonical_path);
        }
    }
    paths.sort();
    Ok(paths)
}

pub(super) fn ensure_child_dir(root: &Path, directory: &Path) -> std::result::Result<(), String> {
    let canonical = fs::canonicalize(directory)
        .map_err(|error| format!("{}: {}", directory.display(), error))?;
    if !canonical.is_dir() {
        return Err(format!("{} is not a directory", canonical.display()));
    }
    if !canonical.starts_with(root) {
        return Err(format!("{} escapes project root", canonical.display()));
    }
    Ok(())
}
