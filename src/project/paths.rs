use std::{
    fs,
    path::{Component, Path, PathBuf},
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

pub(super) fn checked_relative_path(relative: &str) -> std::result::Result<&Path, String> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("invalid scaffold path `{}`", relative));
    }
    Ok(path)
}
