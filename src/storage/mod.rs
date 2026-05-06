mod paths;
mod repository;
mod schema;

use rusqlite::Connection;
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    path::{Path, PathBuf},
};

pub use schema::SCHEMA_VERSION;

pub use repository::{
    ApprovalInsert, ApprovalRepository, EventInsert, EventRepository, Repository, TaskEventInsert,
    TaskInsert, TaskRepository,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Storage {
    pub root: PathBuf,
    pub db_path: PathBuf,
    pub schema_version: i32,
}

pub struct StorageConnection {
    pub storage: Storage,
    pub connection: Connection,
}

impl StorageConnection {
    pub fn repository(&self) -> Repository<'_> {
        Repository::new(&self.connection)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageInit {
    pub root: PathBuf,
    pub state_dir: PathBuf,
    pub db_path: PathBuf,
    pub created_state_dir: bool,
    pub schema_version: i32,
}

#[derive(Debug)]
pub enum StorageError {
    InvalidRoot {
        path: PathBuf,
        message: String,
    },
    PathEscapedRoot {
        root: PathBuf,
        path: PathBuf,
    },
    CreateStateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    OpenDatabase {
        path: PathBuf,
        source: rusqlite::Error,
    },
    InitializeSchema {
        path: PathBuf,
        source: rusqlite::Error,
    },
}

impl Display for StorageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRoot { path, message } => {
                write!(
                    formatter,
                    "invalid project root {}: {}",
                    path.display(),
                    message
                )
            }
            Self::PathEscapedRoot { root, path } => write!(
                formatter,
                "storage path {} escapes project root {}",
                path.display(),
                root.display()
            ),
            Self::CreateStateDir { path, source } => {
                write!(
                    formatter,
                    "could not create state directory {}: {}",
                    path.display(),
                    source
                )
            }
            Self::OpenDatabase { path, source } => {
                write!(
                    formatter,
                    "could not open SQLite database {}: {}",
                    path.display(),
                    source
                )
            }
            Self::InitializeSchema { path, source } => {
                write!(
                    formatter,
                    "could not initialize SQLite schema {}: {}",
                    path.display(),
                    source
                )
            }
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateStateDir { source, .. } => Some(source),
            Self::OpenDatabase { source, .. } => Some(source),
            Self::InitializeSchema { source, .. } => Some(source),
            Self::InvalidRoot { .. } | Self::PathEscapedRoot { .. } => None,
        }
    }
}

pub type StorageResult<T> = Result<T, StorageError>;

pub fn initialize(default_root: &Path, root: Option<&str>) -> StorageResult<StorageInit> {
    let (init, _connection) = connect_inner(default_root, root)?;
    Ok(init)
}

pub fn open(default_root: &Path, root: Option<&str>) -> StorageResult<Storage> {
    let init = initialize(default_root, root)?;
    Ok(Storage {
        root: init.root,
        db_path: init.db_path,
        schema_version: init.schema_version,
    })
}

pub fn connect(default_root: &Path, root: Option<&str>) -> StorageResult<StorageConnection> {
    let (init, connection) = connect_inner(default_root, root)?;
    Ok(StorageConnection {
        storage: Storage {
            root: init.root,
            db_path: init.db_path,
            schema_version: init.schema_version,
        },
        connection,
    })
}

fn connect_inner(
    default_root: &Path,
    root: Option<&str>,
) -> StorageResult<(StorageInit, Connection)> {
    let project_root = paths::resolve_project_root(default_root, root)?;
    let state = paths::prepare_state_dir(&project_root)?;
    let db_path = paths::resolve_database_path(&project_root, &state.path)?;
    let mut connection =
        Connection::open(&db_path).map_err(|source| StorageError::OpenDatabase {
            path: db_path.clone(),
            source,
        })?;

    schema::initialize(&mut connection).map_err(|source| StorageError::InitializeSchema {
        path: db_path.clone(),
        source,
    })?;

    let init = StorageInit {
        root: project_root,
        state_dir: state.path,
        db_path,
        created_state_dir: state.created,
        schema_version: SCHEMA_VERSION,
    };
    Ok((init, connection))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::TempDir;

    #[test]
    fn initialize_creates_storage_schema_inside_project_root() {
        let project = TempDir::new().expect("temp dir");

        let init = initialize(project.path(), None).expect("storage init");

        assert_eq!(
            init.root,
            project.path().canonicalize().expect("canonical root")
        );
        assert!(init.created_state_dir);
        assert_eq!(init.state_dir, init.root.join(".platy"));
        assert_eq!(init.db_path, init.state_dir.join("platypus.sqlite3"));
        assert_eq!(init.schema_version, SCHEMA_VERSION);
        assert!(init.db_path.is_file());

        let connection = Connection::open(&init.db_path).expect("open db");
        assert_eq!(read_user_version(&connection), SCHEMA_VERSION);
        for table in ["metadata", "tasks", "task_events", "findings"] {
            assert!(table_exists(&connection, table), "missing table {table}");
        }
    }

    #[test]
    fn initialize_is_repeatable_without_recreating_state_dir() {
        let project = TempDir::new().expect("temp dir");

        let first = initialize(project.path(), None).expect("first init");
        let second = initialize(project.path(), None).expect("second init");

        assert!(first.created_state_dir);
        assert!(!second.created_state_dir);
        assert_eq!(first.db_path, second.db_path);
        assert_eq!(second.schema_version, SCHEMA_VERSION);
    }

    #[cfg(unix)]
    #[test]
    fn initialize_blocks_state_dir_symlink_escape() {
        use std::os::unix::fs::symlink;

        let project = TempDir::new().expect("project dir");
        let outside = TempDir::new().expect("outside dir");
        symlink(outside.path(), project.path().join(".platy")).expect("symlink");

        let error = initialize(project.path(), None).expect_err("escape should fail");

        assert!(
            matches!(error, StorageError::PathEscapedRoot { .. }),
            "unexpected error: {error}"
        );
    }

    fn read_user_version(connection: &Connection) -> i32 {
        connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("user version")
    }

    fn table_exists(connection: &Connection, table: &str) -> bool {
        connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |_| Ok(()),
            )
            .is_ok()
    }
}
