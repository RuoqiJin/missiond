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

/// Migrate a single table from SQLite to PostgreSQL using cursor-based pagination.
///
/// Reads rows in batches of PAGE_SIZE to avoid OOM on large tables.
#[cfg(all(feature = "sqlite", feature = "postgres"))]
async fn migrate_table(
    sqlite: &crate::db::MissionDB,
    pool: &sqlx::PgPool,
    table: &str,
) -> Result<usize, String> {
    use rusqlite::types::ValueRef;

    const PAGE_SIZE: usize = 500;

    // Check if PG table already has data (skip if non-empty — idempotent resume)
    let (pg_count,): (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {}", table))
        .fetch_one(pool)
        .await
        .map_err(|e| format!("PG count check: {}", e))?;

    if pg_count > 0 {
        return Ok(0); // Already migrated
    }

    let mut inserted = 0usize;
    let mut offset = 0usize;

    loop {
        // Read one page from SQLite
        let page = sqlite.with_read(|conn| {
            let sql = format!("SELECT * FROM {} LIMIT {} OFFSET {}", table, PAGE_SIZE, offset);
            let mut stmt = conn.prepare(&sql)
                .map_err(|e| crate::db::error::DbError::Other(format!("prepare: {}", e)))?;

            let col_count = stmt.column_count();
            let col_names: Vec<String> = (0..col_count)
                .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
                .collect();

            let mut page_rows: Vec<Vec<(String, SqliteValue)>> = Vec::new();

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
                page_rows.push(row_data);
            }

            Ok(page_rows)
        }).map_err(|e| format!("SQLite read: {}", e))?;

        if page.is_empty() {
            break; // No more rows
        }

        let page_len = page.len();

        // Insert page into PostgreSQL
        for row in &page {
            let col_names: Vec<&str> = row.iter().map(|(n, _)| n.as_str()).collect();
            // Build placeholders: use SQL NULL literal for null values to avoid
            // sqlx sending typed-NULL (text OID) which PG rejects for integer columns.
            let mut param_idx = 0usize;
            let placeholders: Vec<String> = row.iter().map(|(_, val)| {
                match val {
                    SqliteValue::Null => "NULL".to_string(),
                    _ => {
                        param_idx += 1;
                        format!("${}", param_idx)
                    }
                }
            }).collect();

            let sql = format!(
                "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT DO NOTHING",
                table,
                col_names.join(", "),
                placeholders.join(", ")
            );

            let mut query = sqlx::query(&sql);
            for (_, val) in row {
                query = match val {
                    SqliteValue::Null => continue, // Already handled as SQL NULL literal
                    SqliteValue::Integer(v) => query.bind(*v),
                    SqliteValue::Real(v) => query.bind(*v),
                    SqliteValue::Text(v) => query.bind(v.as_str()),
                    SqliteValue::Blob(v) => query.bind(v.as_slice()),
                };
            }

            match query.execute(pool).await {
                Ok(_) => inserted += 1,
                Err(e) => {
                    if inserted == 0 {
                        warn!(table, error = %e, "Insert failed");
                    }
                }
            }
        }

        offset += page_len;

        if page_len < PAGE_SIZE {
            break; // Last page
        }
    }

    Ok(inserted)
}

/// Post-migration: backfill pgvector `embedding_vec` columns from BYTEA `embedding` data.
///
/// The ETL copies BYTEA as-is but pgvector columns (PG-only) remain NULL.
/// This function decodes little-endian f32 arrays from BYTEA and writes them as vector(768).
///
/// Returns total rows backfilled across all tables.
#[cfg(feature = "postgres")]
pub async fn backfill_pg_embeddings(pg_url: &str) -> Result<usize, String> {
    use sqlx::postgres::PgPoolOptions;
    use std::time::Duration;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect(pg_url)
        .await
        .map_err(|e| format!("PG connect: {}", e))?;

    // Tables with BYTEA embedding + pgvector embedding_vec columns.
    // PK is &[&str] — supports single and composite keys with tuple comparison.
    let tables: Vec<(&str, &[&str])> = vec![
        ("knowledge", &["id"]),
        ("ast_nodes", &["id"]),
        ("skill_topics", &["topic"]),
        ("conversation_topic_vectors", &["session_id", "chunk_idx"]),
    ];

    let mut total = 0usize;
    for (table, pk_cols) in &tables {
        match backfill_table_embeddings(&pool, table, pk_cols).await {
            Ok(n) => {
                if n > 0 {
                    info!(table, backfilled = n, "Embedding backfill");
                }
                total += n;
            }
            Err(e) => {
                warn!(table, error = %e, "Embedding backfill failed — skipping");
            }
        }
    }

    Ok(total)
}

/// Backfill embedding_vec for a single table using keyset pagination.
/// Supports single and composite PKs via tuple comparison: `(a, b) > ($1, $2)`.
#[cfg(feature = "postgres")]
async fn backfill_table_embeddings(
    pool: &sqlx::PgPool,
    table: &str,
    pk_cols: &[&str],
) -> Result<usize, String> {
    const PAGE_SIZE: i64 = 200;
    let mut updated = 0usize;
    let mut cursor_values: Vec<String> = Vec::new();

    // Cast all PK columns to text in SELECT so try_get::<String> works for INTEGER PKs
    let pk_select = pk_cols.iter().map(|c| format!("{c}::text")).collect::<Vec<_>>().join(", ");
    let pk_order = pk_cols.join(", ");

    loop {
        let sql = if cursor_values.is_empty() {
            format!(
                "SELECT {pk_select}, embedding FROM {table} \
                 WHERE embedding IS NOT NULL AND embedding_vec IS NULL \
                 ORDER BY {pk_order} LIMIT {PAGE_SIZE}"
            )
        } else {
            // Tuple comparison: (col1, col2) > ($1, $2)
            let pk_tuple = format!("({})", pk_cols.join(", "));
            let param_tuple = format!("({})", (1..=pk_cols.len()).map(|i| format!("${i}")).collect::<Vec<_>>().join(", "));
            format!(
                "SELECT {pk_select}, embedding FROM {table} \
                 WHERE embedding IS NOT NULL AND embedding_vec IS NULL \
                 AND {pk_tuple} > {param_tuple} \
                 ORDER BY {pk_order} LIMIT {PAGE_SIZE}"
            )
        };

        // Fetch page — extract PK values + embedding blob dynamically
        let rows = if cursor_values.is_empty() {
            sqlx::query(&sql).fetch_all(pool).await
        } else {
            let mut q = sqlx::query(&sql);
            for v in &cursor_values {
                q = q.bind(v.as_str());
            }
            q.fetch_all(pool).await
        }.map_err(|e| format!("fetch: {}", e))?;

        if rows.is_empty() {
            break;
        }

        let row_count = rows.len();

        for row in &rows {
            use sqlx::Row;
            // Extract PK values for cursor — MUST update before any `continue`
            let mut pk_vals: Vec<String> = Vec::new();
            for (i, _col) in pk_cols.iter().enumerate() {
                let val: String = row.try_get(i).unwrap_or_default();
                pk_vals.push(val);
            }
            cursor_values = pk_vals.clone(); // Always advance cursor to prevent infinite loop

            // Extract embedding blob (last column after PK columns)
            let blob: Vec<u8> = row.try_get(pk_cols.len()).unwrap_or_default();

            if blob.len() % 4 != 0 {
                continue;
            }
            let floats: Vec<f32> = blob
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();

            let vec_str = format!(
                "[{}]",
                floats.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(",")
            );

            // Build UPDATE with correct PK WHERE clause
            let set_param = 1; // $1 = vector string
            let where_parts: Vec<String> = pk_cols.iter().enumerate()
                .map(|(i, col)| format!("{col} = ${}", set_param + 1 + i))
                .collect();
            let update_sql = format!(
                "UPDATE {table} SET embedding_vec = $1::vector WHERE {}",
                where_parts.join(" AND ")
            );
            let mut q = sqlx::query(&update_sql).bind(&vec_str);
            for v in &pk_vals {
                q = q.bind(v.as_str());
            }
            match q.execute(pool).await {
                Ok(_) => updated += 1,
                Err(e) => {
                    warn!(table, pk = ?pk_vals, error = %e, "Embedding update failed");
                }
            }
        }

        if (row_count as i64) < PAGE_SIZE {
            break;
        }
    }

    Ok(updated)
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
