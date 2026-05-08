use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};

use crate::db;
use crate::error::{AppError, AppResult};
use crate::middleware::auth::{create_token, AuthUser};
use crate::models::{AuthResponse, LoginRequest, RegisterRequest};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/register", post(register))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/users/me", axum::routing::get(me))
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> AppResult<Json<AuthResponse>> {
    if req.username.len() < 3 || req.username.len() > 50 {
        return Err(AppError::BadRequest(
            "Username must be 3-50 characters".into(),
        ));
    }
    if !req.email.contains('@') {
        return Err(AppError::BadRequest("Invalid email".into()));
    }
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".into(),
        ));
    }

    let password_hash = hash_password(&req.password)?;
    let user_id = uuid::Uuid::new_v4().to_string();

    let user = {
        let conn = state.db.conn();
        db::users::create_user(&*conn, &user_id, &req.username, &req.email, &password_hash)
            .map_err(|e| {
                if e.to_string().contains("UNIQUE") {
                    AppError::BadRequest("Username or email already exists".into())
                } else {
                    AppError::Database(e)
                }
            })?
    };

    // Auto-assign free plan
    {
        let conn = state.db.conn();
        let sub_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now();
        let period_end = now + chrono::Duration::days(30);
        db::subscriptions::create_subscription(
            &*conn,
            &sub_id,
            &user_id,
            "plan-free",
            &now.format("%Y-%m-%d").to_string(),
            &period_end.format("%Y-%m-%d").to_string(),
        )?;
    }

    let token = create_token(&user.id, user.role.as_str(), &state.jwt_secret)
        .map_err(AppError::Internal)?;

    Ok(Json(AuthResponse { token, user }))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<AuthResponse>> {
    let user = {
        let conn = state.db.conn();
        db::users::get_user_by_email(&*conn, &req.email)
            .map_err(|_| AppError::Unauthorized("Invalid email or password".into()))?
    };

    verify_password(&req.password, &user.password_hash)?;

    let token = create_token(&user.id, user.role.as_str(), &state.jwt_secret)
        .map_err(AppError::Internal)?;

    Ok(Json(AuthResponse { token, user }))
}

async fn me(State(state): State<AppState>, auth: AuthUser) -> AppResult<Json<serde_json::Value>> {
    let conn = state.db.conn();
    let user = db::users::get_user_by_id(&*conn, &auth.user_id)?;
    let quota = db::subscriptions::get_quota_info(&*conn, &auth.user_id)?;

    Ok(Json(serde_json::json!({
        "user": user,
        "quota": quota,
    })))
}

fn hash_password(password: &str) -> AppResult<String> {
    use argon2::{
        password_hash::{rand_core::OsRng, SaltString},
        Argon2, PasswordHasher,
    };
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Hash error: {e}")))?
        .to_string();
    Ok(hash)
}

fn verify_password(password: &str, hash: &str) -> AppResult<()> {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    let parsed = PasswordHash::new(hash)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Hash parse error: {e}")))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthorized("Invalid email or password".into()))
}
