use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::ExecStatus;
use rusqlite::Connection;
use std::sync::Arc;

pub struct BillingService {
    platform_cut: f64,
}

impl BillingService {
    pub fn new(platform_cut: f64) -> Self {
        Self { platform_cut }
    }

    /// Check if user has available quota. Returns (can_proceed, is_quota_based)
    pub fn check_quota(&self, conn: &Connection, user_id: &str) -> AppResult<QuotaCheck> {
        let sub = db::subscriptions::get_active_subscription(conn, user_id)?;

        let sub = match sub {
            Some(s) => s,
            None => {
                // No subscription — check if user has balance for pay-per-use
                let user = db::users::get_user_by_id(conn, user_id)?;
                if user.balance > 0.0 {
                    return Ok(QuotaCheck::PayPerUse {
                        balance: user.balance,
                    });
                }
                return Err(AppError::QuotaExceeded);
            }
        };

        let plan = db::subscriptions::get_plan(conn, &sub.plan_id)?;
        let (used, extra) = db::subscriptions::get_or_create_quota(
            conn,
            user_id,
            &sub.current_period_start,
            &sub.current_period_end,
        )?;

        let total = plan.monthly_quota + extra;
        if used < total {
            Ok(QuotaCheck::WithinQuota {
                remaining: total - used,
                period_start: sub.current_period_start,
                max_tier: plan.max_skill_tier,
            })
        } else {
            // Quota exhausted — fall back to balance
            let user = db::users::get_user_by_id(conn, user_id)?;
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
    pub fn record_execution(
        &self,
        conn: &Connection,
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
                db::subscriptions::increment_usage(conn, user_id, period_start)?;
            }
            QuotaCheck::PayPerUse { .. } => {
                // Pay-per-use: charge from balance
                cost = price_per_use;
                creator_revenue = price_per_use * (1.0 - self.platform_cut);
                if *status == ExecStatus::Success {
                    db::users::update_balance(conn, user_id, -cost)?;
                }
            }
        }

        db::executions::record_execution(
            conn,
            execution_id,
            user_id,
            skill_id,
            status,
            input_tokens,
            output_tokens,
            cost,
            creator_revenue,
        )?;

        if *status == ExecStatus::Success {
            db::skills::increment_invoke_count(conn, skill_id)?;
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
pub fn settle_creator_revenues(db: &Arc<crate::db::Database>) -> AppResult<i64> {
    let conn = db.conn();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM revenue_ledger WHERE settled = 0",
        [],
        |r| r.get(0),
    )?;

    if count == 0 {
        return Ok(0);
    }

    conn.execute_batch(
        "UPDATE users SET balance = balance + (
            SELECT COALESCE(SUM(rl.amount), 0)
            FROM revenue_ledger rl
            JOIN executions e ON rl.execution_id = e.id
            JOIN skills s ON e.skill_id = s.id
            WHERE s.creator_id = users.id AND rl.settled = 0
        ) WHERE id IN (
            SELECT DISTINCT s.creator_id
            FROM revenue_ledger rl
            JOIN executions e ON rl.execution_id = e.id
            JOIN skills s ON e.skill_id = s.id
            WHERE rl.settled = 0
        );
        UPDATE revenue_ledger SET settled = 1 WHERE settled = 0;",
    )?;

    tracing::info!("Settled {count} revenue entries");
    Ok(count)
}
