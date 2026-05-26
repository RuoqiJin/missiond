use crate::models::{Plan, QuotaInfo, SubStatus, Subscription};
use sqlx::{PgPool, Row};

pub async fn list_plans(pool: &PgPool) -> Result<Vec<Plan>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, monthly_quota, max_skill_tier, price, is_active FROM plans WHERE is_active = true ORDER BY price",
    )
    .fetch_all(pool)
    .await?;
    rows.iter().map(row_to_plan).collect()
}

pub async fn get_plan(pool: &PgPool, plan_id: &str) -> Result<Plan, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, monthly_quota, max_skill_tier, price, is_active FROM plans WHERE id = $1",
    )
    .bind(plan_id)
    .fetch_one(pool)
    .await?;
    row_to_plan(&row)
}

pub async fn get_active_subscription(
    pool: &PgPool,
    user_id: &str,
) -> Result<Option<Subscription>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, user_id, plan_id, status, current_period_start, current_period_end, created_at
         FROM subscriptions WHERE user_id = $1 AND status = 'active'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(row_to_subscription).transpose()
}

pub async fn create_subscription(
    pool: &PgPool,
    id: &str,
    user_id: &str,
    plan_id: &str,
    period_start: &str,
    period_end: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE subscriptions SET status = 'cancelled' WHERE user_id = $1 AND status = 'active'",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO subscriptions (id, user_id, plan_id, status, current_period_start, current_period_end)
         VALUES ($1, $2, $3, 'active', $4, $5)",
    )
    .bind(id)
    .bind(user_id)
    .bind(plan_id)
    .bind(period_start)
    .bind(period_end)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn get_or_create_quota(
    pool: &PgPool,
    user_id: &str,
    period_start: &str,
    period_end: &str,
) -> Result<(i64, i64), sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO quota_usage (id, user_id, period_start, period_end)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, period_start) DO UPDATE SET period_end = EXCLUDED.period_end
         RETURNING used, extra_purchased",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_id)
    .bind(period_start)
    .bind(period_end)
    .fetch_one(pool)
    .await?;
    Ok((row.try_get("used")?, row.try_get("extra_purchased")?))
}

pub async fn increment_usage(
    pool: &PgPool,
    user_id: &str,
    period_start: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE quota_usage SET used = used + 1 WHERE user_id = $1 AND period_start = $2")
        .bind(user_id)
        .bind(period_start)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn add_extra_quota(
    pool: &PgPool,
    user_id: &str,
    period_start: &str,
    amount: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE quota_usage SET extra_purchased = extra_purchased + $1 WHERE user_id = $2 AND period_start = $3",
    )
    .bind(amount)
    .bind(user_id)
    .bind(period_start)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_quota_info(
    pool: &PgPool,
    user_id: &str,
) -> Result<Option<QuotaInfo>, sqlx::Error> {
    let sub = match get_active_subscription(pool, user_id).await? {
        Some(s) => s,
        None => return Ok(None),
    };
    let plan = get_plan(pool, &sub.plan_id).await?;
    let (used, extra) = get_or_create_quota(
        pool,
        user_id,
        &sub.current_period_start,
        &sub.current_period_end,
    )
    .await?;

    let total = plan.monthly_quota + extra;
    let remaining = (total - used).max(0);

    Ok(Some(QuotaInfo {
        plan_name: plan.name,
        monthly_quota: plan.monthly_quota,
        used_this_period: used,
        remaining,
        extra_purchased: extra,
        period_end: sub.current_period_end,
    }))
}

fn row_to_plan(row: &sqlx::postgres::PgRow) -> Result<Plan, sqlx::Error> {
    Ok(Plan {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        monthly_quota: row.try_get("monthly_quota")?,
        max_skill_tier: row.try_get("max_skill_tier")?,
        price: row.try_get("price")?,
        is_active: row.try_get("is_active")?,
    })
}

fn row_to_subscription(row: &sqlx::postgres::PgRow) -> Result<Subscription, sqlx::Error> {
    let status: String = row.try_get("status")?;
    Ok(Subscription {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        plan_id: row.try_get("plan_id")?,
        status: SubStatus::from_str(&status),
        current_period_start: row.try_get("current_period_start")?,
        current_period_end: row.try_get("current_period_end")?,
        created_at: crate::db::ts_string(row, "created_at")?,
    })
}
