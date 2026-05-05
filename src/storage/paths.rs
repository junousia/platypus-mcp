use super::{StorageError, StorageResult};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct StateDir {
    pub path: PathBuf,
    pub created: bool,
}

pub fn resolve_project_root(default_root: &Path, root: Option<&str>) -> StorageResult<PathBuf> {
    let requested = root
        .map(PathBuf::from)
        .unwrap_or_else(|| default_root.to_path_buf());
    let canonical = fs::canonicalize(&requested).map_err(|error| StorageError::InvalidRoot {
        path: requested.clone(),
        message: error.to_string(),
    })?;
    if !canonical.is_dir() {
        return Err(StorageError::InvalidRoot {
            path: canonical,
            message: "not a directory".to_string(),
        });
    }
    Ok(canonical)
}

pub fn prepare_state_dir(root: &Path) -> StorageResult<StateDir> {
    let requested = root.join(".platy");
    let created = if requested.exists() {
        false
    } else {
        fs::create_dir(&requested).map_err(|source| StorageError::CreateStateDir {
            path: requested.clone(),
            source,
        })?;
        true
    };

    let canonical = fs::canonicalize(&requested).map_err(|error| StorageError::InvalidRoot {
        path: requested.clone(),
        message: error.to_string(),
    })?;
    ensure_inside_root(root, &canonical)?;

    if !canonical.is_dir() {
        return Err(StorageError::InvalidRoot {
            path: canonical,
            message: "state path is not a directory".to_string(),
        });
    }

    Ok(StateDir {
        path: canonical,
        created,
    })
}

pub fn resolve_database_path(root: &Path, state_dir: &Path) -> StorageResult<PathBuf> {
    let requested = state_dir.join("platypus.sqlite3");
    if requested.exists() {
        let canonical =
            fs::canonicalize(&requested).map_err(|error| StorageError::InvalidRoot {
                path: requested.clone(),
                message: error.to_string(),
            })?;
        ensure_inside_root(root, &canonical)?;
        return Ok(canonical);
    }

    ensure_inside_root(root, state_dir)?;
    Ok(requested)
}

fn ensure_inside_root(root: &Path, path: &Path) -> StorageResult<()> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(StorageError::PathEscapedRoot {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
        })
    }
}
