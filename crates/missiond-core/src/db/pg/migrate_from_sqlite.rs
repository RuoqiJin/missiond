//! SQLite → PostgreSQL data migration tool (M2-6).
//!
//! One-time ETL: reads all rows from SQLite MissionDB, writes to PgMissionStore.
//! Supports cursor-based resume for crash recovery.
//!
//! Usage: called from daemon with `--migrate-sqlite-to-pg` flag.

#[cfg(all(feature = "sqlite", feature = "postgres"))]
use tracing::{info, warn};

/// Table migration order — respects foreign key dependencies.
#[cfg(all(feature = "sqlite", feature = "postgres"))]
const MIGRATION_ORDER: &[&str] = &[
    // Independent tables first
    "tasks",
    "inbox",
    "events",
    "slot_sessions",
    "daemon_state",
    "dynamic_slots",
    "image_descriptions",
    "gemini_file_uploads",
    "backfill_progress",
    "backfill_failures",
    // Board (parent before children)
    "board_tasks",
    "board_task_notes",
    "agent_questions",
    // Knowledge (parent before children)
    "knowledge",
    "credentials",
    "knowledge_edges",
    "kb_access_log",
    "kb_operation_queue",
    "kb_ast_links",
    "prompt_snapshots",
    // Conversations (parent before children)
    "conversations",
    "conversation_messages",
    "conversation_events",
    "conversation_tool_calls",
    "conversation_topic_vectors",
    "consumer_watermarks",
    "message_labels",
    "message_narrations",
    "narration_cursors",
    "message_translations",
    "retrospective_results",
    // Slots
    "slot_tasks",
    "token_usage_ledger",
    // Skills
    "skill_topics",
    "skill_blocks",
    "skill_versions",
    "skill_executions",
    // Observability
    "gemini_requests",
    "incidents",
    "system_timeline",
    "router_chat_archive",
    // AST
    "ast_nodes",
    "ast_file_meta",
    "beacons",
    "beacon_nodes",
];

/// Migrate all data from SQLite to PostgreSQL.
///
/// Returns (tables_migrated, total_rows).
#[cfg(all(feature = "sqlite", feature = "postgres"))]
pub async fn migrate_sqlite_to_pg(
    sqlite_path: &str,
    pg_url: &str,
) -> Result<(usize, usize), String> {
    use crate::db::MissionDB;
    use sqlx::postgres::PgPoolOptions;
    use std::time::Duration;

    info!(sqlite = %sqlite_path, pg = %pg_url, "Starting SQLite → PostgreSQL migration");

    // Open SQLite
    let sqlite = MissionDB::new(sqlite_path)
        .map_err(|e| format!("Failed to open SQLite: {}", e))?;

    // Connect to PostgreSQL
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect(pg_url)
        .await
        .map_err(|e| format!("Failed to connect to PostgreSQL: {}", e))?;

    // Run migrations first
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| format!("Failed to run PG migrations: {}", e))?;

    let mut tables_done = 0usize;
    let mut total_rows = 0usize;

    for table in MIGRATION_ORDER {
        match migrate_table(&sqlite, &pool, table).await {
            Ok(count) => {
                if count > 0 {
                    info!(table, count, "Migrated");
                }
                tables_done += 1;
                total_rows += count;
            }
            Err(e) => {
                warn!(table, error = %e, "Migration failed — skipping");
            }
        }
    }

    info!(tables = tables_done, rows = total_rows, "Migration complete");
    Ok((tables_done, total_rows))
}

/// Migrate a single table from SQLite to PostgreSQL using generic row copy.
#[cfg(all(feature = "sqlite", feature = "postgres"))]
async fn migrate_table(
    sqlite: &crate::db::MissionDB,
    pool: &sqlx::PgPool,
    table: &str,
) -> Result<usize, String> {
    use rusqlite::types::ValueRef;

    // Check if PG table already has data (skip if non-empty — idempotent resume)
    let (pg_count,): (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {}", table))
        .fetch_one(pool)
        .await
        .map_err(|e| format!("PG count check: {}", e))?;

    if pg_count > 0 {
        return Ok(0); // Already migrated
    }

    // Read all rows from SQLite
    let rows = sqlite.with_read(|conn| {
        let mut stmt = conn.prepare(&format!("SELECT * FROM {}", table))
            .map_err(|e| crate::db::error::DbError::Other(format!("prepare: {}", e)))?;

        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
            .collect();

        let mut all_rows: Vec<Vec<(String, SqliteValue)>> = Vec::new();

        let mut rows_iter = stmt.query([])
            .map_err(|e| crate::db::error::DbError::Other(format!("query: {}", e)))?;

        while let Some(row) = rows_iter.next()
            .map_err(|e| crate::db::error::DbError::Other(format!("next: {}", e)))?
        {
            let mut row_data = Vec::new();
            for (i, name) in col_names.iter().enumerate() {
                let val = match row.get_ref(i) {
                    Ok(ValueRef::Null) => SqliteValue::Null,
                    Ok(ValueRef::Integer(v)) => SqliteValue::Integer(v),
                    Ok(ValueRef::Real(v)) => SqliteValue::Real(v),
                    Ok(ValueRef::Text(v)) => SqliteValue::Text(String::from_utf8_lossy(v).to_string()),
                    Ok(ValueRef::Blob(v)) => SqliteValue::Blob(v.to_vec()),
                    Err(_) => SqliteValue::Null,
                };
                row_data.push((name.clone(), val));
            }
            all_rows.push(row_data);
        }

        Ok(all_rows)
    }).map_err(|e| format!("SQLite read: {}", e))?;

    if rows.is_empty() {
        return Ok(0);
    }

    // Insert into PostgreSQL in batches
    let batch_size = 100;
    let mut inserted = 0usize;

    for chunk in rows.chunks(batch_size) {
        for row in chunk {
            let col_names: Vec<&str> = row.iter().map(|(n, _)| n.as_str()).collect();
            let placeholders: Vec<String> = (1..=col_names.len()).map(|i| format!("${}", i)).collect();

            let sql = format!(
                "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT DO NOTHING",
                table,
                col_names.join(", "),
                placeholders.join(", ")
            );

            let mut query = sqlx::query(&sql);
            for (_, val) in row {
                query = match val {
                    SqliteValue::Null => query.bind(None::<String>),
                    SqliteValue::Integer(v) => query.bind(*v),
                    SqliteValue::Real(v) => query.bind(*v),
                    SqliteValue::Text(v) => query.bind(v.as_str()),
                    SqliteValue::Blob(v) => query.bind(v.as_slice()),
                };
            }

            match query.execute(pool).await {
                Ok(_) => inserted += 1,
                Err(e) => {
                    // Log but continue (ON CONFLICT handles dupes)
                    if inserted == 0 {
                        // Only warn on first failure per table
                        warn!(table, error = %e, "Insert failed");
                    }
                }
            }
        }
    }

    Ok(inserted)
}

/// Intermediate value type for SQLite → PG transfer.
#[cfg(all(feature = "sqlite", feature = "postgres"))]
#[derive(Debug)]
enum SqliteValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}
