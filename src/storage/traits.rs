use super::repository::{ApprovalInsert, EventInsert, TaskEventInsert, TaskInsert};
use crate::models::{
    ApprovalRecord, EventRecord, RuntimeTransitionRecord, TaskEventRecord, TaskRecord,
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

pub trait TaskStore {
    fn create(&self, task: TaskInsert) -> RepositoryResult<TaskRecord>;
    fn record_event(&self, event: TaskEventInsert) -> RepositoryResult<TaskEventRecord>;
    fn list_events(&self, task_id: &str, limit: usize) -> RepositoryResult<Vec<TaskEventRecord>>;
    fn get(&self, id: &str) -> RepositoryResult<TaskRecord>;
    fn mark_running(&self, task_id: &str) -> RepositoryResult<usize>;
    fn finish(&self, task_id: &str, status: &str) -> RepositoryResult<usize>;
}
