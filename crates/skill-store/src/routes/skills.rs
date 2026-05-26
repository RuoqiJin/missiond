use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::models::{CreateSkillRequest, SkillPublic, UpdateSkillRequest, UserRole};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/skills", get(list_skills))
        .route("/api/v1/skills/{skill_id}", get(get_skill))
        .route(
            "/api/v1/creator/skills",
            post(create_skill).get(list_my_skills),
        )
        .route("/api/v1/creator/skills/{skill_id}", put(update_skill))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default = "default_offset")]
    offset: i64,
    #[serde(default = "default_limit")]
    limit: i64,
    /// Optional max tier filter (default: show all accessible)
    #[serde(default = "default_tier")]
    tier: i32,
}

fn default_offset() -> i64 {
    0
}
fn default_limit() -> i64 {
    20
}
fn default_tier() -> i32 {
    3
}

/// Public skill listing (no auth required, tier defaults to showing all)
async fn list_skills(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Vec<SkillPublic>>> {
    let skills = db::skills::list_skills_public(state.db.pool(), q.tier, q.offset, q.limit).await?;
    Ok(Json(skills))
}

async fn get_skill(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
) -> AppResult<Json<SkillPublic>> {
    let skill = db::skills::get_skill_by_id(state.db.pool(), &skill_id)
        .await
        .map_err(|_| AppError::NotFound(format!("Skill {skill_id} not found")))?;
    let creator = db::users::get_user_by_id(state.db.pool(), &skill.creator_id).await?;

    Ok(Json(SkillPublic {
        id: skill.id,
        creator_id: skill.creator_id,
        creator_name: creator.username,
        name: skill.name,
        description: skill.description,
        input_schema: skill.input_schema,
        tier: skill.tier,
        price_per_use: skill.price_per_use,
        invoke_count: skill.invoke_count,
        created_at: skill.created_at,
    }))
}

async fn create_skill(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateSkillRequest>,
) -> AppResult<Json<serde_json::Value>> {
    if auth.role != UserRole::Creator.as_str() && auth.role != UserRole::Admin.as_str() {
        db::users::update_role(state.db.pool(), &auth.user_id, &UserRole::Creator).await?;
    }

    if req.name.is_empty() {
        return Err(AppError::BadRequest("Name is required".into()));
    }
    if req.prompt_template.is_empty() {
        return Err(AppError::BadRequest("Prompt template is required".into()));
    }

    let skill_id = uuid::Uuid::new_v4().to_string();
    let skill = db::skills::create_skill(
        state.db.pool(),
        &skill_id,
        &auth.user_id,
        &req.name,
        &req.description,
        &req.prompt_template,
        &req.input_schema,
        req.tier,
        req.price_per_use,
    )
    .await?;

    Ok(Json(serde_json::json!({
        "id": skill.id,
        "name": skill.name,
        "tier": skill.tier,
        "created": true,
    })))
}

async fn list_my_skills(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<crate::models::Skill>>> {
    let skills = db::skills::list_creator_skills(state.db.pool(), &auth.user_id).await?;
    Ok(Json(skills))
}

async fn update_skill(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(skill_id): Path<String>,
    Json(req): Json<UpdateSkillRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let skill = db::skills::get_skill_by_id(state.db.pool(), &skill_id)
        .await
        .map_err(|_| AppError::NotFound(format!("Skill {skill_id} not found")))?;
    if skill.creator_id != auth.user_id && auth.role != UserRole::Admin.as_str() {
        return Err(AppError::Forbidden("Not your skill".into()));
    }

    db::skills::update_skill(
        state.db.pool(),
        &skill_id,
        req.name.as_deref(),
        req.description.as_deref(),
        req.prompt_template.as_deref(),
        req.input_schema.as_ref(),
        req.tier,
        req.price_per_use,
        req.is_active,
    )
    .await?;

    Ok(Json(serde_json::json!({ "updated": true })))
}
