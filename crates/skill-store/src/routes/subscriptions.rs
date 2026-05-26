use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::db;
use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::models::{Plan, QuotaInfo, SubscribeRequest, TopUpRequest};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/plans", get(list_plans))
        .route("/api/v1/subscriptions/subscribe", post(subscribe))
        .route("/api/v1/subscriptions/top-up", post(top_up))
        .route("/api/v1/subscriptions/quota", get(get_quota))
}

async fn list_plans(State(state): State<AppState>) -> AppResult<Json<Vec<Plan>>> {
    let plans = db::subscriptions::list_plans(state.db.pool()).await?;
    Ok(Json(plans))
}

async fn subscribe(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SubscribeRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let plan = db::subscriptions::get_plan(state.db.pool(), &req.plan_id)
        .await
        .map_err(|_| AppError::NotFound(format!("Plan {} not found", req.plan_id)))?;

    if plan.price > 0.0 {
        let user = db::users::get_user_by_id(state.db.pool(), &auth.user_id).await?;
        if user.balance < plan.price {
            return Err(AppError::BadRequest(format!(
                "Insufficient balance. Need {:.2}, have {:.2}",
                plan.price, user.balance
            )));
        }
        db::users::update_balance(state.db.pool(), &auth.user_id, -plan.price).await?;
    }

    let sub_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();
    let period_end = now + chrono::Duration::days(30);

    db::subscriptions::create_subscription(
        state.db.pool(),
        &sub_id,
        &auth.user_id,
        &req.plan_id,
        &now.format("%Y-%m-%d").to_string(),
        &period_end.format("%Y-%m-%d").to_string(),
    )
    .await?;

    Ok(Json(serde_json::json!({
        "subscriptionId": sub_id,
        "plan": plan.name,
        "periodEnd": period_end.format("%Y-%m-%d").to_string(),
    })))
}

async fn top_up(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<TopUpRequest>,
) -> AppResult<Json<serde_json::Value>> {
    if req.amount <= 0 {
        return Err(AppError::BadRequest("Amount must be positive".into()));
    }

    let sub = db::subscriptions::get_active_subscription(state.db.pool(), &auth.user_id)
        .await?
        .ok_or_else(|| AppError::BadRequest("No active subscription".into()))?;

    db::subscriptions::add_extra_quota(
        state.db.pool(),
        &auth.user_id,
        &sub.current_period_start,
        req.amount,
    )
    .await?;

    Ok(Json(serde_json::json!({
        "added": req.amount,
        "message": format!("Added {} extra invocations", req.amount),
    })))
}

async fn get_quota(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Option<QuotaInfo>>> {
    let quota = db::subscriptions::get_quota_info(state.db.pool(), &auth.user_id).await?;
    Ok(Json(quota))
}
