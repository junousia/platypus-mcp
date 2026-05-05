use rusqlite::Connection;

pub const SCHEMA_VERSION: i32 = 1;

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

        CREATE TABLE IF NOT EXISTS findings (
            id TEXT PRIMARY KEY,
            source_item_id TEXT,
            source_task_id TEXT,
            source_finding_ref TEXT,
            title TEXT NOT NULL,
            status TEXT NOT NULL,
            severity TEXT,
            summary TEXT NOT NULL,
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
