use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::ExecStatus;
use std::sync::Arc;

pub struct BillingService {
    platform_cut: f64,
}

impl BillingService {
    pub fn new(platform_cut: f64) -> Self {
        Self { platform_cut }
    }

    /// Check if user has available quota. Returns (can_proceed, is_quota_based)
    pub async fn check_quota(&self, pool: &sqlx::PgPool, user_id: &str) -> AppResult<QuotaCheck> {
        let sub = db::subscriptions::get_active_subscription(pool, user_id).await?;

        let sub = match sub {
            Some(s) => s,
            None => {
                // No subscription — check if user has balance for pay-per-use
                let user = db::users::get_user_by_id(pool, user_id).await?;
                if user.balance > 0.0 {
                    return Ok(QuotaCheck::PayPerUse {
                        balance: user.balance,
                    });
                }
                return Err(AppError::QuotaExceeded);
            }
        };

        let plan = db::subscriptions::get_plan(pool, &sub.plan_id).await?;
        let (used, extra) = db::subscriptions::get_or_create_quota(
            pool,
            user_id,
            &sub.current_period_start,
            &sub.current_period_end,
        )
        .await?;

        let total = plan.monthly_quota + extra;
        if used < total {
            Ok(QuotaCheck::WithinQuota {
                remaining: total - used,
                period_start: sub.current_period_start,
                max_tier: plan.max_skill_tier,
            })
        } else {
            // Quota exhausted — fall back to balance
            let user = db::users::get_user_by_id(pool, user_id).await?;
            if user.balance > 0.0 {
                Ok(QuotaCheck::PayPerUse {
                    balance: user.balance,
                })
            } else {
                Err(AppError::QuotaExceeded)
            }
        }
    }

    /// Record a completed execution and handle billing
    pub async fn record_execution(
        &self,
        pool: &sqlx::PgPool,
        execution_id: &str,
        user_id: &str,
        skill_id: &str,
        status: &ExecStatus,
        input_tokens: i64,
        output_tokens: i64,
        price_per_use: f64,
        quota_check: &QuotaCheck,
    ) -> AppResult<()> {
        let cost;
        let creator_revenue;

        match quota_check {
            QuotaCheck::WithinQuota { period_start, .. } => {
                // Quota-based: no direct cost, just increment usage
                cost = 0.0;
                creator_revenue = 0.0;
                db::subscriptions::increment_usage(pool, user_id, period_start).await?;
            }
            QuotaCheck::PayPerUse { .. } => {
                // Pay-per-use: charge from balance
                cost = price_per_use;
                creator_revenue = price_per_use * (1.0 - self.platform_cut);
                if *status == ExecStatus::Success {
                    db::users::update_balance(pool, user_id, -cost).await?;
                }
            }
        }

        db::executions::record_execution(
            pool,
            execution_id,
            user_id,
            skill_id,
            status,
            input_tokens,
            output_tokens,
            cost,
            creator_revenue,
        )
        .await?;

        if *status == ExecStatus::Success {
            db::skills::increment_invoke_count(pool, skill_id).await?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum QuotaCheck {
    WithinQuota {
        remaining: i64,
        period_start: String,
        max_tier: i32,
    },
    PayPerUse {
        balance: f64,
    },
}

impl QuotaCheck {
    pub fn max_tier(&self) -> i32 {
        match self {
            Self::WithinQuota { max_tier, .. } => *max_tier,
            Self::PayPerUse { .. } => 3, // Pay-per-use can access all tiers
        }
    }
}

/// Settle creator revenues (called by cron job)
pub async fn settle_creator_revenues(db: &Arc<crate::db::Database>) -> AppResult<i64> {
    let pool = db.pool();
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM revenue_ledger WHERE settled = false")
            .fetch_one(pool)
            .await?;

    if count == 0 {
        return Ok(0);
    }

    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE users SET balance = balance + (
            SELECT COALESCE(SUM(rl.amount), 0)
            FROM revenue_ledger rl
            JOIN executions e ON rl.execution_id = e.id
            JOIN skills s ON e.skill_id = s.id
            WHERE s.creator_id = users.id AND rl.settled = false
        ) WHERE id IN (
            SELECT DISTINCT s.creator_id
            FROM revenue_ledger rl
            JOIN executions e ON rl.execution_id = e.id
            JOIN skills s ON e.skill_id = s.id
            WHERE rl.settled = false
        )",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE revenue_ledger SET settled = true WHERE settled = false")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    tracing::info!("Settled {count} revenue entries");
    Ok(count)
}
