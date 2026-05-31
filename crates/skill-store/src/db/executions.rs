use crate::models::ExecStatus;
use sqlx::{PgPool, Row};

pub async fn record_execution(
    pool: &PgPool,
    id: &str,
    user_id: &str,
    skill_id: &str,
    status: &ExecStatus,
    input_tokens: i64,
    output_tokens: i64,
    cost: f64,
    creator_revenue: f64,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO executions (id, user_id, skill_id, status, input_tokens, output_tokens, cost, creator_revenue)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(user_id)
    .bind(skill_id)
    .bind(status.as_str())
    .bind(input_tokens)
    .bind(output_tokens)
    .bind(cost)
    .bind(creator_revenue)
    .execute(&mut *tx)
    .await?;

    if creator_revenue > 0.0 {
        let ledger_id = uuid::Uuid::new_v4().to_string();
        let row = sqlx::query("SELECT creator_id FROM skills WHERE id = $1")
            .bind(skill_id)
            .fetch_one(&mut *tx)
            .await?;
        let creator_id: String = row.try_get("creator_id")?;
        sqlx::query(
            "INSERT INTO revenue_ledger (id, creator_id, execution_id, amount) VALUES ($1, $2, $3, $4)",
        )
        .bind(ledger_id)
        .bind(creator_id)
        .bind(id)
        .bind(creator_revenue)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn get_creator_stats(
    pool: &PgPool,
    creator_id: &str,
) -> Result<CreatorStats, sqlx::Error> {
    let total_invocations: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(invoke_count), 0)::bigint FROM skills WHERE creator_id = $1",
    )
    .bind(creator_id)
    .fetch_one(pool)
    .await?;

    let total_revenue: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0)::float8 FROM revenue_ledger WHERE creator_id = $1",
    )
    .bind(creator_id)
    .fetch_one(pool)
    .await?;

    let unsettled_revenue: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0)::float8 FROM revenue_ledger WHERE creator_id = $1 AND settled = false",
    )
    .bind(creator_id)
    .fetch_one(pool)
    .await?;

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
