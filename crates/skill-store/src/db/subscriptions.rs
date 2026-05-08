use crate::models::{Plan, QuotaInfo, SubStatus, Subscription};
use rusqlite::{params, Connection};

pub fn list_plans(conn: &Connection) -> Result<Vec<Plan>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, name, monthly_quota, max_skill_tier, price, is_active FROM plans WHERE is_active = 1 ORDER BY price",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Plan {
            id: row.get(0)?,
            name: row.get(1)?,
            monthly_quota: row.get(2)?,
            max_skill_tier: row.get(3)?,
            price: row.get(4)?,
            is_active: row.get::<_, i32>(5)? != 0,
        })
    })?;
    rows.collect()
}

pub fn get_plan(conn: &Connection, plan_id: &str) -> Result<Plan, rusqlite::Error> {
    conn.query_row(
        "SELECT id, name, monthly_quota, max_skill_tier, price, is_active FROM plans WHERE id = ?1",
        [plan_id],
        |row| {
            Ok(Plan {
                id: row.get(0)?,
                name: row.get(1)?,
                monthly_quota: row.get(2)?,
                max_skill_tier: row.get(3)?,
                price: row.get(4)?,
                is_active: row.get::<_, i32>(5)? != 0,
            })
        },
    )
}

pub fn get_active_subscription(
    conn: &Connection,
    user_id: &str,
) -> Result<Option<Subscription>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, plan_id, status, current_period_start, current_period_end, created_at
         FROM subscriptions WHERE user_id = ?1 AND status = 'active'
         ORDER BY created_at DESC LIMIT 1",
    )?;

    let mut rows = stmt.query_map([user_id], |row| {
        Ok(Subscription {
            id: row.get(0)?,
            user_id: row.get(1)?,
            plan_id: row.get(2)?,
            status: SubStatus::from_str(&row.get::<_, String>(3)?),
            current_period_start: row.get(4)?,
            current_period_end: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;

    match rows.next() {
        Some(Ok(sub)) => Ok(Some(sub)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

pub fn create_subscription(
    conn: &Connection,
    id: &str,
    user_id: &str,
    plan_id: &str,
    period_start: &str,
    period_end: &str,
) -> Result<(), rusqlite::Error> {
    // Cancel any existing active subscription
    conn.execute(
        "UPDATE subscriptions SET status = 'cancelled' WHERE user_id = ?1 AND status = 'active'",
        [user_id],
    )?;

    conn.execute(
        "INSERT INTO subscriptions (id, user_id, plan_id, status, current_period_start, current_period_end)
         VALUES (?1, ?2, ?3, 'active', ?4, ?5)",
        params![id, user_id, plan_id, period_start, period_end],
    )?;
    Ok(())
}

pub fn get_or_create_quota(
    conn: &Connection,
    user_id: &str,
    period_start: &str,
    period_end: &str,
) -> Result<(i64, i64), rusqlite::Error> {
    // Try get existing
    let result = conn.query_row(
        "SELECT used, extra_purchased FROM quota_usage WHERE user_id = ?1 AND period_start = ?2",
        params![user_id, period_start],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    );

    match result {
        Ok(v) => Ok(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO quota_usage (id, user_id, period_start, period_end) VALUES (?1, ?2, ?3, ?4)",
                params![id, user_id, period_start, period_end],
            )?;
            Ok((0, 0))
        }
        Err(e) => Err(e),
    }
}

pub fn increment_usage(
    conn: &Connection,
    user_id: &str,
    period_start: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE quota_usage SET used = used + 1 WHERE user_id = ?1 AND period_start = ?2",
        params![user_id, period_start],
    )?;
    Ok(())
}

pub fn add_extra_quota(
    conn: &Connection,
    user_id: &str,
    period_start: &str,
    amount: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE quota_usage SET extra_purchased = extra_purchased + ?1 WHERE user_id = ?2 AND period_start = ?3",
        params![amount, user_id, period_start],
    )?;
    Ok(())
}

pub fn get_quota_info(
    conn: &Connection,
    user_id: &str,
) -> Result<Option<QuotaInfo>, rusqlite::Error> {
    let sub = match get_active_subscription(conn, user_id)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let plan = get_plan(conn, &sub.plan_id)?;
    let (used, extra) = get_or_create_quota(
        conn,
        user_id,
        &sub.current_period_start,
        &sub.current_period_end,
    )?;

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
