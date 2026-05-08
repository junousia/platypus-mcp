use super::traits::{
    ApprovalStore, EventStore, RepositoryError, RepositoryResult, TaskStore, TransitionInsert,
    TransitionStore,
};
use crate::models::{
    ApprovalRecord, EventRecord, RuntimeTransitionRecord, TaskEventRecord, TaskRecord,
};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde_json::Value;
use std::collections::BTreeMap;

pub struct Repository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> Repository<'connection> {
    pub fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn approvals(&self) -> ApprovalRepository<'connection> {
        ApprovalRepository {
            connection: self.connection,
        }
    }

    pub fn events(&self) -> EventRepository<'connection> {
        EventRepository {
            connection: self.connection,
        }
    }

    pub fn tasks(&self) -> TaskRepository<'connection> {
        TaskRepository {
            connection: self.connection,
        }
    }

    pub fn transitions(&self) -> TransitionRepository<'connection> {
        TransitionRepository {
            connection: self.connection,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApprovalInsert {
    pub scope: String,
    pub title: String,
    pub summary: String,
    pub requested_by: Option<String>,
    pub metadata: BTreeMap<String, Value>,
}

pub struct ApprovalRepository<'connection> {
    connection: &'connection Connection,
}

impl ApprovalStore for ApprovalRepository<'_> {
    fn create(&self, approval: ApprovalInsert) -> RepositoryResult<ApprovalRecord> {
        let id = self.next_id()?;
        let metadata_json = serde_json::to_string(&approval.metadata)?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            r#"
            INSERT INTO approvals(id, scope, status, title, summary, requested_by, metadata_json)
            VALUES (?1, ?2, 'pending', ?3, ?4, ?5, ?6)
            "#,
            params![
                &id,
                approval.scope,
                approval.title,
                approval.summary,
                approval.requested_by,
                metadata_json
            ],
        )?;
        insert_transition_in_transaction(
            &transaction,
            TransitionInsert {
                domain: "approval".to_string(),
                entity_id: id.clone(),
                transition_type: "approval_requested".to_string(),
                summary: format!("Approval `{id}` requested."),
                payload: None,
            },
        )?;
        let record = get_approval_in_transaction(&transaction, &id)?;
        transaction.commit()?;
        Ok(record)
    }

    fn list(&self, status: Option<&str>, limit: usize) -> RepositoryResult<Vec<ApprovalRecord>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT id, scope, status, title, summary, requested_by, response, responder, reason,
                   metadata_json, created_at, responded_at
            FROM approvals
            WHERE (?1 IS NULL OR status = ?1)
            ORDER BY created_at ASC, id ASC
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(params![status, limit], row_to_approval)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(RepositoryError::from)
    }

    fn get(&self, id: &str) -> RepositoryResult<ApprovalRecord> {
        self.connection
            .query_row(
                r#"
            SELECT id, scope, status, title, summary, requested_by, response, responder, reason,
                   metadata_json, created_at, responded_at
            FROM approvals
            WHERE id = ?1
            "#,
                [id],
                row_to_approval,
            )
            .map_err(RepositoryError::from)
    }

    fn respond(
        &self,
        approval_id: &str,
        status: &str,
        response: &str,
        responder: &str,
        reason: Option<&str>,
    ) -> RepositoryResult<usize> {
        let transaction = self.connection.unchecked_transaction()?;
        let updated = transaction.execute(
            r#"
            UPDATE approvals
            SET status = ?2,
                response = ?3,
                responder = ?4,
                reason = ?5,
                responded_at = CURRENT_TIMESTAMP
            WHERE id = ?1 AND status = 'pending'
            "#,
            params![approval_id, status, response, responder, reason],
        )?;
        if updated > 0 {
            insert_transition_in_transaction(
                &transaction,
                TransitionInsert {
                    domain: "approval".to_string(),
                    entity_id: approval_id.to_string(),
                    transition_type: format!("approval_{status}"),
                    summary: format!("Approval `{approval_id}` {status}."),
                    payload: Some(serde_json::json!({
                        "response": response,
                        "responder": responder,
                        "reason": reason
                    })),
                },
            )?;
        }
        transaction.commit()?;
        Ok(updated)
    }
}

impl ApprovalRepository<'_> {
    fn next_id(&self) -> rusqlite::Result<String> {
        let existing: i64 =
            self.connection
                .query_row("SELECT COUNT(*) FROM approvals", [], |row| row.get(0))?;
        Ok(format!("APR-{:03}", existing + 1))
    }
}

#[derive(Debug, Clone)]
pub struct EventInsert {
    pub event_type: String,
    pub scope: String,
    pub task_id: Option<String>,
    pub summary: String,
    pub payload: Option<Value>,
}

pub struct EventRepository<'connection> {
    connection: &'connection Connection,
}

pub struct TransitionRepository<'connection> {
    connection: &'connection Connection,
}

#[derive(Debug, Clone)]
pub struct TaskInsert {
    pub source_item_id: String,
    pub title: String,
    pub worker: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskEventInsert {
    pub task_id: String,
    pub sequence: Option<i64>,
    pub event_type: String,
    pub summary: String,
    pub payload: Option<Value>,
}

pub struct TaskRepository<'connection> {
    connection: &'connection Connection,
}

impl TaskStore for TaskRepository<'_> {
    fn create(&self, task: TaskInsert) -> RepositoryResult<TaskRecord> {
        let id = self.next_task_id(&task.source_item_id)?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            r#"
            INSERT INTO tasks(id, source_item_id, title, status, worker)
            VALUES (?1, ?2, ?3, 'queued', ?4)
            "#,
            params![&id, task.source_item_id, task.title, task.worker],
        )?;
        insert_transition_in_transaction(
            &transaction,
            TransitionInsert {
                domain: "task".to_string(),
                entity_id: id.clone(),
                transition_type: "task_created".to_string(),
                summary: format!("Task `{id}` created."),
                payload: None,
            },
        )?;
        let record = get_task_in_transaction(&transaction, &id)?;
        transaction.commit()?;
        Ok(record)
    }

    fn record_event(&self, event: TaskEventInsert) -> RepositoryResult<TaskEventRecord> {
        let sequence = event
            .sequence
            .map(Ok)
            .unwrap_or_else(|| self.next_sequence(&event.task_id))?;
        let payload_json = event
            .payload
            .map(|payload| serde_json::to_string(&payload).unwrap_or_else(|_| "null".to_string()));
        self.connection.execute(
            r#"
            INSERT INTO task_events(task_id, sequence, event_type, summary, payload_json)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                &event.task_id,
                sequence,
                event.event_type,
                event.summary,
                payload_json
            ],
        )?;
        self.get_event(&event.task_id, sequence)
            .map_err(RepositoryError::from)
    }

    fn list_events(&self, task_id: &str, limit: usize) -> RepositoryResult<Vec<TaskEventRecord>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT task_id, sequence, event_type, summary, payload_json, created_at
            FROM task_events
            WHERE task_id = ?1
            ORDER BY sequence ASC, id ASC
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(params![task_id, limit], row_to_task_event)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(RepositoryError::from)
    }

    fn get(&self, id: &str) -> RepositoryResult<TaskRecord> {
        self.connection
            .query_row(
                r#"
            SELECT id, source_item_id, title, status, worker, claimed_by, claimed_at, started_at,
                   finished_at, workspace_path, workspace_branch, workspace_base_ref, created_at,
                   updated_at
            FROM tasks
            WHERE id = ?1
            "#,
                [id],
                row_to_task,
            )
            .map_err(RepositoryError::from)
    }

    fn mark_running(&self, task_id: &str) -> RepositoryResult<usize> {
        let transaction = self.connection.unchecked_transaction()?;
        let updated = transaction.execute(
            r#"
            UPDATE tasks
            SET status = 'running',
                started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1 AND status IN ('claimed', 'running')
            "#,
            [task_id],
        )?;
        if updated > 0 {
            insert_transition_in_transaction(
                &transaction,
                TransitionInsert {
                    domain: "task".to_string(),
                    entity_id: task_id.to_string(),
                    transition_type: "task_running".to_string(),
                    summary: format!("Task `{task_id}` marked running."),
                    payload: None,
                },
            )?;
        }
        transaction.commit()?;
        Ok(updated)
    }

    fn finish(&self, task_id: &str, status: &str) -> RepositoryResult<usize> {
        let transaction = self.connection.unchecked_transaction()?;
        let updated = transaction.execute(
            r#"
            UPDATE tasks
            SET status = ?2,
                finished_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1 AND status IN ('claimed', 'running')
            "#,
            params![task_id, status],
        )?;
        if updated > 0 {
            insert_transition_in_transaction(
                &transaction,
                TransitionInsert {
                    domain: "task".to_string(),
                    entity_id: task_id.to_string(),
                    transition_type: format!("task_{status}"),
                    summary: format!("Task `{task_id}` {status}."),
                    payload: Some(serde_json::json!({ "status": status })),
                },
            )?;
        }
        transaction.commit()?;
        Ok(updated)
    }
}

impl TaskRepository<'_> {
    fn get_event(&self, task_id: &str, sequence: i64) -> rusqlite::Result<TaskEventRecord> {
        self.connection.query_row(
            r#"
            SELECT task_id, sequence, event_type, summary, payload_json, created_at
            FROM task_events
            WHERE task_id = ?1 AND sequence = ?2
            "#,
            params![task_id, sequence],
            row_to_task_event,
        )
    }

    fn next_task_id(&self, source_item_id: &str) -> rusqlite::Result<String> {
        let existing: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM tasks WHERE source_item_id = ?1",
            [source_item_id],
            |row| row.get(0),
        )?;
        Ok(format!(
            "{}-T{:03}",
            safe_id_prefix(source_item_id),
            existing + 1
        ))
    }

    fn next_sequence(&self, task_id: &str) -> rusqlite::Result<i64> {
        let current: Option<i64> = self
            .connection
            .query_row(
                "SELECT MAX(sequence) FROM task_events WHERE task_id = ?1",
                [task_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        Ok(current.unwrap_or(0) + 1)
    }
}

impl EventStore for EventRepository<'_> {
    fn record(&self, event: EventInsert) -> RepositoryResult<EventRecord> {
        let payload_json = event
            .payload
            .map(|payload| serde_json::to_string(&payload).unwrap_or_else(|_| "null".to_string()));
        self.connection.execute(
            r#"
            INSERT INTO events(event_type, scope, task_id, summary, payload_json)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                event.event_type,
                event.scope,
                event.task_id,
                event.summary,
                payload_json
            ],
        )?;
        self.get(self.connection.last_insert_rowid())
            .map_err(RepositoryError::from)
    }

    fn list(
        &self,
        scope: Option<&str>,
        task_id: Option<&str>,
        limit: usize,
    ) -> RepositoryResult<Vec<EventRecord>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT id, event_type, scope, task_id, summary, payload_json, created_at
            FROM events
            WHERE (?1 IS NULL OR scope = ?1)
              AND (?2 IS NULL OR task_id = ?2)
            ORDER BY id ASC
            LIMIT ?3
            "#,
        )?;
        let rows = statement.query_map(params![scope, task_id, limit], row_to_event)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(RepositoryError::from)
    }

    fn list_task_events(
        &self,
        task_id: Option<&str>,
        limit: usize,
    ) -> RepositoryResult<Vec<EventRecord>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT task_id, sequence, event_type, summary, payload_json, created_at
            FROM task_events
            WHERE (?1 IS NULL OR task_id = ?1)
            ORDER BY id ASC
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(params![task_id, limit], |row| {
            let task_id: String = row.get("task_id")?;
            let sequence: i64 = row.get("sequence")?;
            let payload_json: Option<String> = row.get("payload_json")?;
            Ok(EventRecord {
                cursor: format!("task:{task_id}:{sequence}"),
                event_type: row.get("event_type")?,
                scope: "task".to_string(),
                task_id: Some(task_id),
                summary: row.get("summary")?,
                payload: payload_json.and_then(|raw| serde_json::from_str(&raw).ok()),
                created_at: row.get("created_at")?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(RepositoryError::from)
    }
}

impl EventRepository<'_> {
    fn get(&self, id: i64) -> rusqlite::Result<EventRecord> {
        self.connection.query_row(
            r#"
            SELECT id, event_type, scope, task_id, summary, payload_json, created_at
            FROM events
            WHERE id = ?1
            "#,
            [id],
            row_to_event,
        )
    }
}

impl TransitionStore for TransitionRepository<'_> {
    fn record(&self, transition: TransitionInsert) -> RepositoryResult<RuntimeTransitionRecord> {
        let id = insert_transition(self.connection, transition)?;
        self.get(id)
    }

    fn list(
        &self,
        domain: Option<&str>,
        entity_id: Option<&str>,
        limit: usize,
    ) -> RepositoryResult<Vec<RuntimeTransitionRecord>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT id, domain, entity_id, transition_type, summary, payload_json, created_at
            FROM runtime_transitions
            WHERE (?1 IS NULL OR domain = ?1)
              AND (?2 IS NULL OR entity_id = ?2)
            ORDER BY id ASC
            LIMIT ?3
            "#,
        )?;
        let rows = statement.query_map(params![domain, entity_id, limit], row_to_transition)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(RepositoryError::from)
    }
}

impl TransitionRepository<'_> {
    fn get(&self, id: i64) -> RepositoryResult<RuntimeTransitionRecord> {
        self.connection
            .query_row(
                r#"
                SELECT id, domain, entity_id, transition_type, summary, payload_json, created_at
                FROM runtime_transitions
                WHERE id = ?1
                "#,
                [id],
                row_to_transition,
            )
            .map_err(RepositoryError::from)
    }
}

fn insert_transition(
    connection: &Connection,
    transition: TransitionInsert,
) -> RepositoryResult<i64> {
    let payload_json = encode_payload(transition.payload)?;
    connection.execute(
        r#"
        INSERT INTO runtime_transitions(domain, entity_id, transition_type, summary, payload_json)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
        params![
            transition.domain,
            transition.entity_id,
            transition.transition_type,
            transition.summary,
            payload_json
        ],
    )?;
    Ok(connection.last_insert_rowid())
}

fn insert_transition_in_transaction(
    transaction: &Transaction<'_>,
    transition: TransitionInsert,
) -> RepositoryResult<i64> {
    let payload_json = encode_payload(transition.payload)?;
    transaction.execute(
        r#"
        INSERT INTO runtime_transitions(domain, entity_id, transition_type, summary, payload_json)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
        params![
            transition.domain,
            transition.entity_id,
            transition.transition_type,
            transition.summary,
            payload_json
        ],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn encode_payload(payload: Option<Value>) -> RepositoryResult<Option<String>> {
    payload
        .map(|payload| serde_json::to_string(&payload).map_err(RepositoryError::from))
        .transpose()
}

fn get_approval_in_transaction(
    connection: &Transaction<'_>,
    id: &str,
) -> RepositoryResult<ApprovalRecord> {
    connection
        .query_row(
            r#"
            SELECT id, scope, status, title, summary, requested_by, response, responder, reason,
                   metadata_json, created_at, responded_at
            FROM approvals
            WHERE id = ?1
            "#,
            [id],
            row_to_approval,
        )
        .map_err(RepositoryError::from)
}

fn get_task_in_transaction(connection: &Transaction<'_>, id: &str) -> RepositoryResult<TaskRecord> {
    connection
        .query_row(
            r#"
            SELECT id, source_item_id, title, status, worker, claimed_by, claimed_at, started_at,
                   finished_at, workspace_path, workspace_branch, workspace_base_ref, created_at,
                   updated_at
            FROM tasks
            WHERE id = ?1
            "#,
            [id],
            row_to_task,
        )
        .map_err(RepositoryError::from)
}

fn row_to_approval(row: &Row<'_>) -> rusqlite::Result<ApprovalRecord> {
    let metadata_json: Option<String> = row.get("metadata_json")?;
    Ok(ApprovalRecord {
        id: row.get("id")?,
        scope: row.get("scope")?,
        status: row.get("status")?,
        title: row.get("title")?,
        summary: row.get("summary")?,
        requested_by: row.get("requested_by")?,
        response: row.get("response")?,
        responder: row.get("responder")?,
        reason: row.get("reason")?,
        metadata: metadata_json
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default(),
        created_at: row.get("created_at")?,
        responded_at: row.get("responded_at")?,
    })
}

fn row_to_event(row: &Row<'_>) -> rusqlite::Result<EventRecord> {
    let id: i64 = row.get("id")?;
    let payload_json: Option<String> = row.get("payload_json")?;
    Ok(EventRecord {
        cursor: format!("event:{id}"),
        event_type: row.get("event_type")?,
        scope: row.get("scope")?,
        task_id: row.get("task_id")?,
        summary: row.get("summary")?,
        payload: payload_json.and_then(|raw| serde_json::from_str(&raw).ok()),
        created_at: row.get("created_at")?,
    })
}

fn row_to_task_event(row: &Row<'_>) -> rusqlite::Result<TaskEventRecord> {
    let payload_json: Option<String> = row.get("payload_json")?;
    Ok(TaskEventRecord {
        task_id: row.get("task_id")?,
        sequence: row.get("sequence")?,
        event_type: row.get("event_type")?,
        summary: row.get("summary")?,
        payload: payload_json.and_then(|raw| serde_json::from_str(&raw).ok()),
        created_at: row.get("created_at")?,
    })
}

fn row_to_task(row: &Row<'_>) -> rusqlite::Result<TaskRecord> {
    Ok(TaskRecord {
        id: row.get("id")?,
        source_item_id: row.get("source_item_id")?,
        title: row.get("title")?,
        status: row.get("status")?,
        worker: row.get("worker")?,
        claimed_by: row.get("claimed_by")?,
        claimed_at: row.get("claimed_at")?,
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
        workspace_path: row.get("workspace_path")?,
        workspace_branch: row.get("workspace_branch")?,
        workspace_base_ref: row.get("workspace_base_ref")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_transition(row: &Row<'_>) -> rusqlite::Result<RuntimeTransitionRecord> {
    let id: i64 = row.get("id")?;
    let payload_json: Option<String> = row.get("payload_json")?;
    Ok(RuntimeTransitionRecord {
        cursor: format!("transition:{id}"),
        domain: row.get("domain")?,
        entity_id: row.get("entity_id")?,
        transition_type: row.get("transition_type")?,
        summary: row.get("summary")?,
        payload: payload_json.and_then(|raw| serde_json::from_str(&raw).ok()),
        created_at: row.get("created_at")?,
    })
}

fn safe_id_prefix(value: &str) -> String {
    let prefix: String = value
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                Some(character.to_ascii_uppercase())
            } else if character == '-' || character == '_' {
                Some('-')
            } else {
                None
            }
        })
        .collect();
    let prefix = prefix.trim_matches('-');
    if prefix.is_empty() {
        "TASK".to_string()
    } else {
        prefix.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn repository_creates_lists_and_responds_to_approvals() {
        let project = TempDir::new().expect("temp dir");
        let storage = storage::connect(project.path(), None).expect("storage");
        let repository = storage.repository();
        let mut metadata = BTreeMap::new();
        metadata.insert("tool".to_string(), json!("create_backlog_item"));

        let approval = repository
            .approvals()
            .create(ApprovalInsert {
                scope: "manager_tool".to_string(),
                title: "Create backlog item".to_string(),
                summary: "Create PROJ-001".to_string(),
                requested_by: Some("manager".to_string()),
                metadata,
            })
            .expect("create approval");

        assert_eq!(approval.id, "APR-001");
        assert_eq!(approval.status, "pending");

        let pending = repository
            .approvals()
            .list(Some("pending"), 10)
            .expect("list approvals");
        assert_eq!(pending.len(), 1);

        let updated = repository
            .approvals()
            .respond(
                &approval.id,
                "approved",
                "approved",
                "user",
                Some("trusted"),
            )
            .expect("respond approval");
        assert_eq!(updated, 1);
        let approved = repository.approvals().get(&approval.id).expect("approval");
        assert_eq!(approved.status, "approved");
        assert_eq!(approved.reason.as_deref(), Some("trusted"));
    }

    #[test]
    fn repository_records_and_replays_events() {
        let project = TempDir::new().expect("temp dir");
        let storage = storage::connect(project.path(), None).expect("storage");
        let repository = storage.repository();

        let event = repository
            .events()
            .record(EventInsert {
                event_type: "approval_requested".to_string(),
                scope: "approval".to_string(),
                task_id: None,
                summary: "Approval requested.".to_string(),
                payload: Some(json!({"approval_id": "APR-001"})),
            })
            .expect("record event");

        assert_eq!(event.cursor, "event:1");
        let events = repository
            .events()
            .list(Some("approval"), None, 10)
            .expect("list events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "approval_requested");
    }

    #[test]
    fn repository_creates_reads_and_finishes_tasks() {
        let project = TempDir::new().expect("temp dir");
        let storage = storage::connect(project.path(), None).expect("storage");
        let repository = storage.repository();

        let task = repository
            .tasks()
            .create(TaskInsert {
                source_item_id: "PROJ-001".to_string(),
                title: "Build thing".to_string(),
                worker: Some("coder".to_string()),
            })
            .expect("create task");
        assert_eq!(task.id, "PROJ-001-T001");
        assert_eq!(task.status, "queued");

        let event = repository
            .tasks()
            .record_event(TaskEventInsert {
                task_id: task.id.clone(),
                sequence: None,
                event_type: "worker_started".to_string(),
                summary: "Worker started.".to_string(),
                payload: None,
            })
            .expect("record task event");
        assert_eq!(event.sequence, 1);

        let events = repository
            .tasks()
            .list_events(&task.id, 10)
            .expect("list task events");
        assert_eq!(events.len(), 1);

        let transitions = repository
            .transitions()
            .list(Some("task"), Some(&task.id), 10)
            .expect("list transitions");
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].transition_type, "task_created");
    }

    #[test]
    fn repository_records_runtime_transitions() {
        let project = TempDir::new().expect("temp dir");
        let storage = storage::connect(project.path(), None).expect("storage");
        let repository = storage.repository();

        let transition = repository
            .transitions()
            .record(TransitionInsert {
                domain: "task".to_string(),
                entity_id: "PROJ-001-T001".to_string(),
                transition_type: "task_created".to_string(),
                summary: "Task created.".to_string(),
                payload: Some(json!({"source": "test"})),
            })
            .expect("record transition");

        assert_eq!(transition.cursor, "transition:1");
        let transitions = repository
            .transitions()
            .list(Some("task"), Some("PROJ-001-T001"), 10)
            .expect("list transitions");
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].payload.as_ref().unwrap()["source"], "test");
    }

    #[test]
    fn repository_returns_backend_neutral_not_found_errors() {
        let project = TempDir::new().expect("temp dir");
        let storage = storage::connect(project.path(), None).expect("storage");
        let repository = storage.repository();

        let task_error = repository
            .tasks()
            .get("missing-task")
            .expect_err("missing task");
        let approval_error = repository
            .approvals()
            .get("APR-404")
            .expect_err("missing approval");

        assert_eq!(task_error, RepositoryError::NotFound);
        assert!(approval_error.is_not_found());
    }

    #[test]
    fn repository_returns_backend_neutral_conflict_errors() {
        let project = TempDir::new().expect("temp dir");
        let storage = storage::connect(project.path(), None).expect("storage");
        let repository = storage.repository();
        let tasks = repository.tasks();

        let task = tasks
            .create(TaskInsert {
                source_item_id: "PROJ-001".to_string(),
                title: "Build thing".to_string(),
                worker: Some("coder".to_string()),
            })
            .expect("create task");
        assert_eq!(task.id, "PROJ-001-T001");

        let conflict = tasks
            .create(TaskInsert {
                source_item_id: "PROJ-001".to_string(),
                title: "Build thing again".to_string(),
                worker: Some("coder".to_string()),
            })
            .expect_err("duplicate active task");

        assert!(conflict.is_conflict());
    }
}
