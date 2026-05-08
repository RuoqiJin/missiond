use crate::models::{ExecStatus, Execution};
use rusqlite::{params, Connection};

pub fn record_execution(
    conn: &Connection,
    id: &str,
    user_id: &str,
    skill_id: &str,
    status: &ExecStatus,
    input_tokens: i64,
    output_tokens: i64,
    cost: f64,
    creator_revenue: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO executions (id, user_id, skill_id, status, input_tokens, output_tokens, cost, creator_revenue)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, user_id, skill_id, status.as_str(), input_tokens, output_tokens, cost, creator_revenue],
    )?;

    // Also record revenue ledger entry for creator
    if creator_revenue > 0.0 {
        let ledger_id = uuid::Uuid::new_v4().to_string();
        let creator_id: String = conn.query_row(
            "SELECT creator_id FROM skills WHERE id = ?1",
            [skill_id],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO revenue_ledger (id, creator_id, execution_id, amount) VALUES (?1, ?2, ?3, ?4)",
            params![ledger_id, creator_id, id, creator_revenue],
        )?;
    }
    Ok(())
}

pub fn list_user_executions(
    conn: &Connection,
    user_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<Execution>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, skill_id, status, input_tokens, output_tokens, cost, creator_revenue, created_at
         FROM executions WHERE user_id = ?1
         ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
    )?;

    let rows = stmt.query_map(params![user_id, limit, offset], row_to_execution)?;
    rows.collect()
}

pub fn get_creator_stats(
    conn: &Connection,
    creator_id: &str,
) -> Result<CreatorStats, rusqlite::Error> {
    let total_invocations: i64 = conn.query_row(
        "SELECT COALESCE(SUM(invoke_count), 0) FROM skills WHERE creator_id = ?1",
        [creator_id],
        |r| r.get(0),
    )?;

    let total_revenue: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM revenue_ledger WHERE creator_id = ?1",
        [creator_id],
        |r| r.get(0),
    )?;

    let unsettled_revenue: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM revenue_ledger WHERE creator_id = ?1 AND settled = 0",
        [creator_id],
        |r| r.get(0),
    )?;

    Ok(CreatorStats {
        total_invocations,
        total_revenue,
        unsettled_revenue,
    })
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatorStats {
    pub total_invocations: i64,
    pub total_revenue: f64,
    pub unsettled_revenue: f64,
}

fn row_to_execution(row: &rusqlite::Row) -> Result<Execution, rusqlite::Error> {
    Ok(Execution {
        id: row.get(0)?,
        user_id: row.get(1)?,
        skill_id: row.get(2)?,
        status: ExecStatus::from_str(&row.get::<_, String>(3)?),
        input_tokens: row.get(4)?,
        output_tokens: row.get(5)?,
        cost: row.get(6)?,
        creator_revenue: row.get(7)?,
        created_at: row.get(8)?,
    })
}
