use super::traits::{
    ApprovalStore, EventStore, LeaseInsert, LeaseStore, RepositoryError, RepositoryResult,
    TaskStore, TransitionInsert, TransitionStore,
};
use crate::models::{
    ApprovalRecord, EventRecord, LeaseRecord, RuntimeTransitionRecord, TaskEventRecord, TaskRecord,
};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction, TransactionBehavior};
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

    pub fn leases(&self) -> LeaseRepository<'connection> {
        LeaseRepository {
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

    pub fn list_newest_unbounded(
        &self,
        status: Option<&str>,
    ) -> RepositoryResult<Vec<ApprovalRecord>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT id, scope, status, title, summary, requested_by, response, responder, reason,
                   metadata_json, created_at, responded_at
            FROM approvals
            WHERE (?1 IS NULL OR status = ?1)
            ORDER BY created_at DESC, id DESC
            "#,
        )?;
        let rows = statement.query_map(params![status], row_to_approval)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(RepositoryError::from)
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

pub struct LeaseRepository<'connection> {
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
        let transaction =
            Transaction::new_unchecked(self.connection, TransactionBehavior::Immediate)?;
        let sequence = event.sequence.map(Ok).unwrap_or_else(|| {
            next_task_event_sequence_in_transaction(&transaction, &event.task_id)
        })?;
        let payload_json = event
            .payload
            .map(|payload| serde_json::to_string(&payload).unwrap_or_else(|_| "null".to_string()));
        transaction.execute(
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
        record_stream_entry_in_transaction(
            &transaction,
            "task",
            &format!("{}:{sequence}", event.task_id),
        )?;
        let record = get_task_event_in_transaction(&transaction, &event.task_id, sequence)?;
        transaction.commit()?;
        Ok(record)
    }

    fn list_events(&self, task_id: &str, limit: usize) -> RepositoryResult<Vec<TaskEventRecord>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT task_events.task_id, task_events.sequence, task_events.event_type,
                   task_events.summary, task_events.payload_json, task_events.created_at,
                   COALESCE(runtime_stream.id, 0) AS replay_order
            FROM task_events
            LEFT JOIN runtime_stream
              ON runtime_stream.source = 'task'
             AND runtime_stream.source_key = task_events.task_id || ':' || task_events.sequence
            WHERE task_events.task_id = ?1
            ORDER BY task_events.sequence ASC, task_events.id ASC
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
    pub fn active_source_items(&self) -> RepositoryResult<Vec<String>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT DISTINCT source_item_id
            FROM tasks
            WHERE status IN ('queued', 'claimed', 'running')
            ORDER BY source_item_id ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>("source_item_id"))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(RepositoryError::from)
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
}

impl EventStore for EventRepository<'_> {
    fn record(&self, event: EventInsert) -> RepositoryResult<EventRecord> {
        let payload_json = event
            .payload
            .map(|payload| serde_json::to_string(&payload).unwrap_or_else(|_| "null".to_string()));
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
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
        let id = transaction.last_insert_rowid();
        record_stream_entry_in_transaction(&transaction, "event", &id.to_string())?;
        let record = get_event_in_transaction(&transaction, id)?;
        transaction.commit()?;
        Ok(record)
    }

    fn list(
        &self,
        scope: Option<&str>,
        task_id: Option<&str>,
        limit: usize,
    ) -> RepositoryResult<Vec<EventRecord>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT events.id, events.event_type, events.scope, events.task_id, events.summary,
                   events.payload_json, events.created_at,
                   COALESCE(runtime_stream.id, 0) AS replay_order
            FROM events
            LEFT JOIN runtime_stream
              ON runtime_stream.source = 'event'
             AND runtime_stream.source_key = CAST(events.id AS TEXT)
            WHERE (?1 IS NULL OR events.scope = ?1)
              AND (?2 IS NULL OR events.task_id = ?2)
            ORDER BY COALESCE(runtime_stream.id, 0) ASC, events.id ASC
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
            SELECT task_events.task_id, task_events.sequence, task_events.event_type,
                   task_events.summary, task_events.payload_json, task_events.created_at,
                   COALESCE(runtime_stream.id, 0) AS replay_order
            FROM task_events
            LEFT JOIN runtime_stream
              ON runtime_stream.source = 'task'
             AND runtime_stream.source_key = task_events.task_id || ':' || task_events.sequence
            WHERE (?1 IS NULL OR task_events.task_id = ?1)
            ORDER BY COALESCE(runtime_stream.id, 0) ASC, task_events.id ASC
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
                replay_order: row.get("replay_order")?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(RepositoryError::from)
    }
}

impl TransitionStore for TransitionRepository<'_> {
    fn record(&self, transition: TransitionInsert) -> RepositoryResult<RuntimeTransitionRecord> {
        let transaction = self.connection.unchecked_transaction()?;
        let id = insert_transition_in_transaction(&transaction, transition)?;
        let record = get_transition_in_transaction(&transaction, id)?;
        transaction.commit()?;
        Ok(record)
    }

    fn list(
        &self,
        domain: Option<&str>,
        entity_id: Option<&str>,
        limit: usize,
    ) -> RepositoryResult<Vec<RuntimeTransitionRecord>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT runtime_transitions.id, runtime_transitions.domain, runtime_transitions.entity_id,
                   runtime_transitions.transition_type, runtime_transitions.summary,
                   runtime_transitions.payload_json, runtime_transitions.created_at,
                   COALESCE(runtime_stream.id, 0) AS replay_order
            FROM runtime_transitions
            LEFT JOIN runtime_stream
              ON runtime_stream.source = 'transition'
             AND runtime_stream.source_key = CAST(runtime_transitions.id AS TEXT)
            WHERE (?1 IS NULL OR runtime_transitions.domain = ?1)
              AND (?2 IS NULL OR runtime_transitions.entity_id = ?2)
            ORDER BY COALESCE(runtime_stream.id, 0) ASC, runtime_transitions.id ASC
            LIMIT ?3
            "#,
        )?;
        let rows = statement.query_map(params![domain, entity_id, limit], row_to_transition)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(RepositoryError::from)
    }
}

impl LeaseStore for LeaseRepository<'_> {
    fn acquire(&self, lease: LeaseInsert) -> RepositoryResult<LeaseRecord> {
        let transaction =
            Transaction::new_unchecked(self.connection, TransactionBehavior::Immediate)?;
        if let Some(conflict) = active_lease_conflict_in_transaction(
            &transaction,
            &lease.scope,
            &lease.target_id,
            None,
        )? {
            return Err(RepositoryError::Conflict {
                message: format!(
                    "active lease `{}` already held by `{}`",
                    conflict.id, conflict.owner
                ),
            });
        }
        let id = next_lease_id_in_transaction(&transaction)?;
        let ttl = ttl_modifier(lease.ttl_seconds);
        let metadata_json = serde_json::to_string(&lease.metadata)?;
        transaction.execute(
            r#"
            INSERT INTO leases(id, scope, target_id, owner, status, metadata_json, expires_at)
            VALUES (?1, ?2, ?3, ?4, 'active', ?5, datetime('now', ?6))
            "#,
            params![
                &id,
                lease.scope,
                lease.target_id,
                lease.owner,
                metadata_json,
                ttl
            ],
        )?;
        let record = get_lease_in_transaction(&transaction, &id)?;
        transaction.commit()?;
        Ok(record)
    }

    fn list(
        &self,
        scope: Option<&str>,
        target_id: Option<&str>,
        status: Option<&str>,
        include_expired: bool,
        limit: usize,
    ) -> RepositoryResult<Vec<LeaseRecord>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT id, scope, target_id, owner, status, metadata_json, acquired_at, renewed_at,
                   released_at, expires_at
            FROM leases
            WHERE (?1 IS NULL OR scope = ?1)
              AND (?2 IS NULL OR target_id = ?2)
              AND (?3 IS NULL OR status = ?3)
              AND (?4 OR status != 'active' OR expires_at > CURRENT_TIMESTAMP)
            ORDER BY acquired_at ASC, id ASC
            LIMIT ?5
            "#,
        )?;
        let rows = statement.query_map(
            params![scope, target_id, status, include_expired, limit],
            row_to_lease,
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(RepositoryError::from)
    }

    fn active_conflict(
        &self,
        scope: &str,
        target_id: &str,
        owner: Option<&str>,
    ) -> RepositoryResult<Option<LeaseRecord>> {
        self.connection
            .query_row(
                r#"
                SELECT id, scope, target_id, owner, status, metadata_json, acquired_at, renewed_at,
                       released_at, expires_at
                FROM leases
                WHERE scope = ?1
                  AND target_id = ?2
                  AND status = 'active'
                  AND expires_at > CURRENT_TIMESTAMP
                  AND (?3 IS NULL OR owner != ?3)
                ORDER BY acquired_at ASC, id ASC
                LIMIT 1
                "#,
                params![scope, target_id, owner],
                row_to_lease,
            )
            .optional()
            .map_err(RepositoryError::from)
    }

    fn renew(
        &self,
        lease_id: &str,
        owner: &str,
        ttl_seconds: u64,
    ) -> RepositoryResult<LeaseRecord> {
        let ttl = ttl_modifier(ttl_seconds);
        let updated = self.connection.execute(
            r#"
            UPDATE leases
            SET renewed_at = CURRENT_TIMESTAMP,
                expires_at = datetime('now', ?3)
            WHERE id = ?1 AND owner = ?2 AND status = 'active' AND expires_at > CURRENT_TIMESTAMP
            "#,
            params![lease_id, owner, ttl],
        )?;
        if updated == 0 {
            return Err(RepositoryError::NotFound);
        }
        self.get(lease_id)
    }

    fn release(&self, lease_id: &str, owner: &str) -> RepositoryResult<LeaseRecord> {
        let updated = self.connection.execute(
            r#"
            UPDATE leases
            SET status = 'released',
                released_at = CURRENT_TIMESTAMP
            WHERE id = ?1 AND owner = ?2 AND status = 'active'
            "#,
            params![lease_id, owner],
        )?;
        if updated == 0 {
            return Err(RepositoryError::NotFound);
        }
        self.get(lease_id)
    }
}

impl LeaseRepository<'_> {
    fn get(&self, id: &str) -> RepositoryResult<LeaseRecord> {
        self.connection
            .query_row(
                r#"
                SELECT id, scope, target_id, owner, status, metadata_json, acquired_at, renewed_at,
                       released_at, expires_at
                FROM leases
                WHERE id = ?1
                "#,
                [id],
                row_to_lease,
            )
            .map_err(RepositoryError::from)
    }
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
    let id = transaction.last_insert_rowid();
    record_stream_entry_in_transaction(transaction, "transition", &id.to_string())?;
    Ok(id)
}

fn record_stream_entry_in_transaction(
    transaction: &Transaction<'_>,
    source: &str,
    source_key: &str,
) -> RepositoryResult<()> {
    transaction.execute(
        r#"
        INSERT OR IGNORE INTO runtime_stream(source, source_key)
        VALUES (?1, ?2)
        "#,
        params![source, source_key],
    )?;
    Ok(())
}

fn encode_payload(payload: Option<Value>) -> RepositoryResult<Option<String>> {
    payload
        .map(|payload| serde_json::to_string(&payload).map_err(RepositoryError::from))
        .transpose()
}

fn ttl_modifier(ttl_seconds: u64) -> String {
    format!("+{} seconds", ttl_seconds.max(1))
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

fn get_event_in_transaction(
    connection: &Transaction<'_>,
    id: i64,
) -> RepositoryResult<EventRecord> {
    connection
        .query_row(
            r#"
            SELECT events.id, events.event_type, events.scope, events.task_id, events.summary,
                   events.payload_json, events.created_at,
                   COALESCE(runtime_stream.id, 0) AS replay_order
            FROM events
            LEFT JOIN runtime_stream
              ON runtime_stream.source = 'event'
             AND runtime_stream.source_key = CAST(events.id AS TEXT)
            WHERE events.id = ?1
            "#,
            [id],
            row_to_event,
        )
        .map_err(RepositoryError::from)
}

fn get_task_event_in_transaction(
    connection: &Transaction<'_>,
    task_id: &str,
    sequence: i64,
) -> RepositoryResult<TaskEventRecord> {
    connection
        .query_row(
            r#"
            SELECT task_events.task_id, task_events.sequence, task_events.event_type,
                   task_events.summary, task_events.payload_json, task_events.created_at,
                   COALESCE(runtime_stream.id, 0) AS replay_order
            FROM task_events
            LEFT JOIN runtime_stream
              ON runtime_stream.source = 'task'
             AND runtime_stream.source_key = task_events.task_id || ':' || task_events.sequence
            WHERE task_events.task_id = ?1 AND task_events.sequence = ?2
            "#,
            params![task_id, sequence],
            row_to_task_event,
        )
        .map_err(RepositoryError::from)
}

fn get_transition_in_transaction(
    connection: &Transaction<'_>,
    id: i64,
) -> RepositoryResult<RuntimeTransitionRecord> {
    connection
        .query_row(
            r#"
            SELECT runtime_transitions.id, runtime_transitions.domain,
                   runtime_transitions.entity_id, runtime_transitions.transition_type,
                   runtime_transitions.summary, runtime_transitions.payload_json,
                   runtime_transitions.created_at,
                   COALESCE(runtime_stream.id, 0) AS replay_order
            FROM runtime_transitions
            LEFT JOIN runtime_stream
              ON runtime_stream.source = 'transition'
             AND runtime_stream.source_key = CAST(runtime_transitions.id AS TEXT)
            WHERE runtime_transitions.id = ?1
            "#,
            [id],
            row_to_transition,
        )
        .map_err(RepositoryError::from)
}

fn get_lease_in_transaction(
    connection: &Transaction<'_>,
    id: &str,
) -> RepositoryResult<LeaseRecord> {
    connection
        .query_row(
            r#"
            SELECT id, scope, target_id, owner, status, metadata_json, acquired_at, renewed_at,
                   released_at, expires_at
            FROM leases
            WHERE id = ?1
            "#,
            [id],
            row_to_lease,
        )
        .map_err(RepositoryError::from)
}

fn active_lease_conflict_in_transaction(
    connection: &Transaction<'_>,
    scope: &str,
    target_id: &str,
    owner: Option<&str>,
) -> RepositoryResult<Option<LeaseRecord>> {
    connection
        .query_row(
            r#"
            SELECT id, scope, target_id, owner, status, metadata_json, acquired_at, renewed_at,
                   released_at, expires_at
            FROM leases
            WHERE scope = ?1
              AND target_id = ?2
              AND status = 'active'
              AND expires_at > CURRENT_TIMESTAMP
              AND (?3 IS NULL OR owner != ?3)
            ORDER BY acquired_at ASC, id ASC
            LIMIT 1
            "#,
            params![scope, target_id, owner],
            row_to_lease,
        )
        .optional()
        .map_err(RepositoryError::from)
}

fn next_lease_id_in_transaction(connection: &Transaction<'_>) -> rusqlite::Result<String> {
    let existing: i64 =
        connection.query_row("SELECT COUNT(*) FROM leases", [], |row| row.get(0))?;
    Ok(format!("LSE-{:03}", existing + 1))
}

fn next_task_event_sequence_in_transaction(
    connection: &Transaction<'_>,
    task_id: &str,
) -> rusqlite::Result<i64> {
    let current: Option<i64> = connection
        .query_row(
            "SELECT MAX(sequence) FROM task_events WHERE task_id = ?1",
            [task_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(current.unwrap_or(0) + 1)
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
        replay_order: row.get("replay_order")?,
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
        replay_order: row.get("replay_order")?,
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
        replay_order: row.get("replay_order")?,
    })
}

fn row_to_lease(row: &Row<'_>) -> rusqlite::Result<LeaseRecord> {
    let metadata_json: Option<String> = row.get("metadata_json")?;
    Ok(LeaseRecord {
        id: row.get("id")?,
        scope: row.get("scope")?,
        target_id: row.get("target_id")?,
        owner: row.get("owner")?,
        status: row.get("status")?,
        metadata: metadata_json
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default(),
        acquired_at: row.get("acquired_at")?,
        renewed_at: row.get("renewed_at")?,
        released_at: row.get("released_at")?,
        expires_at: row.get("expires_at")?,
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
    use std::sync::{Arc, Barrier};
    use std::thread;
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
    fn repository_acquires_renews_and_releases_leases() {
        let project = TempDir::new().expect("temp dir");
        let storage = storage::connect(project.path(), None).expect("storage");
        let repository = storage.repository();
        let leases = repository.leases();

        let lease = leases
            .acquire(LeaseInsert {
                scope: "project".to_string(),
                target_id: "root".to_string(),
                owner: "manager".to_string(),
                ttl_seconds: 60,
                metadata: BTreeMap::new(),
            })
            .expect("acquire lease");
        assert_eq!(lease.id, "LSE-001");

        let conflict = leases
            .acquire(LeaseInsert {
                scope: "project".to_string(),
                target_id: "root".to_string(),
                owner: "worker".to_string(),
                ttl_seconds: 60,
                metadata: BTreeMap::new(),
            })
            .expect_err("conflict");
        assert!(conflict.is_conflict());

        let same_owner_conflict = leases
            .acquire(LeaseInsert {
                scope: "project".to_string(),
                target_id: "root".to_string(),
                owner: "manager".to_string(),
                ttl_seconds: 60,
                metadata: BTreeMap::new(),
            })
            .expect_err("same owner conflict");
        assert!(same_owner_conflict.is_conflict());

        let renewed = leases
            .renew(&lease.id, "manager", 120)
            .expect("renew lease");
        assert!(renewed.renewed_at.is_some());

        let released = leases.release(&lease.id, "manager").expect("release lease");
        assert_eq!(released.status, "released");

        let active = leases
            .active_conflict("project", "root", None)
            .expect("active conflict");
        assert!(active.is_none());
    }

    #[test]
    fn repository_serializes_concurrent_lease_acquisition() {
        let project = TempDir::new().expect("temp dir");
        let manager_storage = storage::connect(project.path(), None).expect("manager storage");
        let worker_storage = storage::connect(project.path(), None).expect("worker storage");
        let barrier = Arc::new(Barrier::new(2));
        let handles = [("manager", manager_storage), ("worker", worker_storage)]
            .into_iter()
            .map(|(owner, storage)| {
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    storage
                        .repository()
                        .leases()
                        .acquire(LeaseInsert {
                            scope: "project".to_string(),
                            target_id: "root".to_string(),
                            owner: owner.to_string(),
                            ttl_seconds: 60,
                            metadata: BTreeMap::new(),
                        })
                        .map(|lease| lease.owner)
                        .map_err(|error| error.to_string())
                })
            })
            .collect::<Vec<_>>();

        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);

        let storage = storage::connect(project.path(), None).expect("storage");
        let active = storage
            .repository()
            .leases()
            .list(Some("project"), Some("root"), Some("active"), false, 10)
            .expect("active leases");
        assert_eq!(active.len(), 1);
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
