use rusqlite::Connection;

pub const SCHEMA_VERSION: i32 = 8;

pub fn initialize(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "busy_timeout", 5_000)?;

    let transaction = connection.transaction()?;
    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS task_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            sequence INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            summary TEXT NOT NULL,
            payload_json TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(task_id, sequence)
        );

        CREATE INDEX IF NOT EXISTS idx_task_events_task_id_sequence
            ON task_events(task_id, sequence);

        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            source_item_id TEXT NOT NULL,
            title TEXT NOT NULL,
            status TEXT NOT NULL,
            worker TEXT,
            claimed_by TEXT,
            claimed_at TEXT,
            started_at TEXT,
            finished_at TEXT,
            workspace_path TEXT,
            workspace_branch TEXT,
            workspace_base_ref TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_tasks_source_item_id
            ON tasks(source_item_id);

        CREATE INDEX IF NOT EXISTS idx_tasks_status
            ON tasks(status);

        CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_one_active_source_item
            ON tasks(source_item_id)
            WHERE status IN ('queued', 'claimed', 'running');

        CREATE TABLE IF NOT EXISTS findings (
            id TEXT PRIMARY KEY,
            source_item_id TEXT,
            source_task_id TEXT,
            source_finding_ref TEXT,
            title TEXT NOT NULL,
            status TEXT NOT NULL,
            severity TEXT,
            required INTEGER NOT NULL DEFAULT 1,
            summary TEXT NOT NULL,
            owner TEXT,
            disposition_reason TEXT,
            evidence_json TEXT,
            metadata_json TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_findings_source_item_id
            ON findings(source_item_id);

        CREATE INDEX IF NOT EXISTS idx_findings_source_task_id
            ON findings(source_task_id);

        CREATE INDEX IF NOT EXISTS idx_findings_status
            ON findings(status);

        CREATE TABLE IF NOT EXISTS approvals (
            id TEXT PRIMARY KEY,
            scope TEXT NOT NULL,
            status TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            requested_by TEXT,
            response TEXT,
            responder TEXT,
            reason TEXT,
            metadata_json TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            responded_at TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_approvals_status
            ON approvals(status);

        CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_type TEXT NOT NULL,
            scope TEXT NOT NULL,
            task_id TEXT,
            summary TEXT NOT NULL,
            payload_json TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_events_scope
            ON events(scope);

        CREATE INDEX IF NOT EXISTS idx_events_task_id
            ON events(task_id);

        CREATE TABLE IF NOT EXISTS evidence (
            id TEXT PRIMARY KEY,
            source_item_id TEXT,
            source_task_id TEXT,
            kind TEXT NOT NULL,
            summary TEXT NOT NULL,
            refs_json TEXT,
            metadata_json TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_evidence_source_item_id
            ON evidence(source_item_id);

        CREATE INDEX IF NOT EXISTS idx_evidence_source_task_id
            ON evidence(source_task_id);

        CREATE INDEX IF NOT EXISTS idx_evidence_kind
            ON evidence(kind);

        CREATE TABLE IF NOT EXISTS worker_assignments (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            worker TEXT,
            status TEXT NOT NULL,
            assigned_by TEXT,
            worktree_path TEXT NOT NULL,
            bundle_json TEXT NOT NULL,
            worker_session TEXT,
            started_at TEXT,
            completed_at TEXT,
            result_status TEXT,
            summary TEXT,
            changed_files_json TEXT,
            verification_status TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_worker_assignments_task_id
            ON worker_assignments(task_id);

        CREATE INDEX IF NOT EXISTS idx_worker_assignments_status
            ON worker_assignments(status);

        CREATE UNIQUE INDEX IF NOT EXISTS idx_worker_assignments_one_active_task
            ON worker_assignments(task_id)
            WHERE status IN ('prepared', 'running');
        "#,
    )?;
    add_column_if_missing(
        &transaction,
        "findings",
        "required",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    add_column_if_missing(&transaction, "findings", "owner", "TEXT")?;
    add_column_if_missing(&transaction, "findings", "disposition_reason", "TEXT")?;
    add_column_if_missing(&transaction, "tasks", "claimed_by", "TEXT")?;
    add_column_if_missing(&transaction, "tasks", "claimed_at", "TEXT")?;
    add_column_if_missing(&transaction, "tasks", "started_at", "TEXT")?;
    add_column_if_missing(&transaction, "tasks", "finished_at", "TEXT")?;
    add_column_if_missing(&transaction, "tasks", "workspace_path", "TEXT")?;
    add_column_if_missing(&transaction, "tasks", "workspace_branch", "TEXT")?;
    add_column_if_missing(&transaction, "tasks", "workspace_base_ref", "TEXT")?;
    transaction.execute(
        r#"
        INSERT INTO metadata(key, value, updated_at)
        VALUES ('schema_version', ?1, CURRENT_TIMESTAMP)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = CURRENT_TIMESTAMP
        "#,
        [SCHEMA_VERSION.to_string()],
    )?;
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    transaction.commit()
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> rusqlite::Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let existing: String = row.get(1)?;
        if existing == column {
            return Ok(());
        }
    }

    connection.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )?;
    Ok(())
}
