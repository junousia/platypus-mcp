use rusqlite::Connection;

pub const SCHEMA_VERSION: i32 = 3;

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
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_tasks_source_item_id
            ON tasks(source_item_id);

        CREATE INDEX IF NOT EXISTS idx_tasks_status
            ON tasks(status);

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
