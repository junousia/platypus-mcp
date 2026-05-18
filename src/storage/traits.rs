use super::repository::{ApprovalInsert, EventInsert, TaskEventInsert, TaskInsert};
use crate::models::{
    ApprovalRecord, EventRecord, LeaseRecord, RuntimeTransitionRecord, TaskEventRecord, TaskRecord,
};
use serde_json::Value;
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

pub type RepositoryResult<T> = Result<T, RepositoryError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryError {
    NotFound,
    Conflict { message: String },
    Serialization { message: String },
    Backend { message: String },
}

impl RepositoryError {
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound)
    }

    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict { .. })
    }
}

impl Display for RepositoryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(formatter, "record not found"),
            Self::Conflict { message } => write!(formatter, "{message}"),
            Self::Serialization { message } => write!(formatter, "{message}"),
            Self::Backend { message } => write!(formatter, "{message}"),
        }
    }
}

impl Error for RepositoryError {}

impl From<rusqlite::Error> for RepositoryError {
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::QueryReturnedNoRows => Self::NotFound,
            rusqlite::Error::SqliteFailure(error, _)
                if error.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Self::Conflict {
                    message: "storage constraint violation".to_string(),
                }
            }
            rusqlite::Error::SqliteFailure(error, _)
                if matches!(
                    error.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ) =>
            {
                Self::Conflict {
                    message: "SQLite database remained locked after the bounded busy timeout; retry the operation after concurrent Platypus work finishes or inspect stuck processes holding the project state database.".to_string(),
                }
            }
            other => Self::Backend {
                message: other.to_string(),
            },
        }
    }
}

impl From<serde_json::Error> for RepositoryError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization {
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RepositoryError;
    use rusqlite::Connection;
    use tempfile::TempDir;

    #[test]
    fn sqlite_busy_errors_return_retryable_recovery_guidance() {
        let temp = TempDir::new().expect("temp dir");
        let db_path = temp.path().join("locked.sqlite3");
        let locker = Connection::open(&db_path).expect("locker connection");
        locker
            .execute("CREATE TABLE records(id INTEGER PRIMARY KEY)", [])
            .expect("create table");
        let transaction = locker.unchecked_transaction().expect("start lock");
        transaction
            .execute("INSERT INTO records DEFAULT VALUES", [])
            .expect("hold write lock");

        let contender = Connection::open(&db_path).expect("contender connection");
        contender
            .busy_timeout(std::time::Duration::from_millis(1))
            .expect("short busy timeout");
        let error = contender
            .execute("INSERT INTO records DEFAULT VALUES", [])
            .expect_err("persistent lock should fail");

        let mapped = RepositoryError::from(error);
        assert!(mapped.is_conflict(), "unexpected error: {mapped}");
        assert!(
            mapped.to_string().contains("bounded busy timeout"),
            "unexpected guidance: {mapped}"
        );
    }
}

pub trait ApprovalStore {
    fn create(&self, approval: ApprovalInsert) -> RepositoryResult<ApprovalRecord>;
    fn list(&self, status: Option<&str>, limit: usize) -> RepositoryResult<Vec<ApprovalRecord>>;
    fn get(&self, id: &str) -> RepositoryResult<ApprovalRecord>;
    fn respond(
        &self,
        approval_id: &str,
        status: &str,
        response: &str,
        responder: &str,
        reason: Option<&str>,
    ) -> RepositoryResult<usize>;
}

pub trait EventStore {
    fn record(&self, event: EventInsert) -> RepositoryResult<EventRecord>;
    fn list(
        &self,
        scope: Option<&str>,
        task_id: Option<&str>,
        limit: usize,
    ) -> RepositoryResult<Vec<EventRecord>>;
    fn list_task_events(
        &self,
        task_id: Option<&str>,
        limit: usize,
    ) -> RepositoryResult<Vec<EventRecord>>;
}

#[derive(Debug, Clone)]
pub struct TransitionInsert {
    pub domain: String,
    pub entity_id: String,
    pub transition_type: String,
    pub summary: String,
    pub payload: Option<Value>,
}

pub trait TransitionStore {
    fn record(&self, transition: TransitionInsert) -> RepositoryResult<RuntimeTransitionRecord>;
    fn list(
        &self,
        domain: Option<&str>,
        entity_id: Option<&str>,
        limit: usize,
    ) -> RepositoryResult<Vec<RuntimeTransitionRecord>>;
}

#[derive(Debug, Clone)]
pub struct LeaseInsert {
    pub scope: String,
    pub target_id: String,
    pub owner: String,
    pub ttl_seconds: u64,
    pub metadata: std::collections::BTreeMap<String, Value>,
}

pub trait LeaseStore {
    fn acquire(&self, lease: LeaseInsert) -> RepositoryResult<LeaseRecord>;
    fn list(
        &self,
        scope: Option<&str>,
        target_id: Option<&str>,
        status: Option<&str>,
        include_expired: bool,
        limit: usize,
    ) -> RepositoryResult<Vec<LeaseRecord>>;
    fn active_conflict(
        &self,
        scope: &str,
        target_id: &str,
        owner: Option<&str>,
    ) -> RepositoryResult<Option<LeaseRecord>>;
    fn renew(&self, lease_id: &str, owner: &str, ttl_seconds: u64)
        -> RepositoryResult<LeaseRecord>;
    fn release(&self, lease_id: &str, owner: &str) -> RepositoryResult<LeaseRecord>;
}

pub trait TaskStore {
    fn create(&self, task: TaskInsert) -> RepositoryResult<TaskRecord>;
    fn record_event(&self, event: TaskEventInsert) -> RepositoryResult<TaskEventRecord>;
    fn list_events(&self, task_id: &str, limit: usize) -> RepositoryResult<Vec<TaskEventRecord>>;
    fn get(&self, id: &str) -> RepositoryResult<TaskRecord>;
    fn mark_running(&self, task_id: &str) -> RepositoryResult<usize>;
    fn finish(&self, task_id: &str, status: &str) -> RepositoryResult<usize>;
}
