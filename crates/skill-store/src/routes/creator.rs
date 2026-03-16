use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use crate::db;
use crate::error::AppResult;
use crate::middleware::auth::AuthUser;
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/creator/stats", get(stats))
}

async fn stats(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<db::executions::CreatorStats>> {
    let conn = state.db.conn();
    let stats = db::executions::get_creator_stats(&*conn, &auth.user_id)?;
    Ok(Json(stats))
}
