use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Quota exceeded")]
    QuotaExceeded,

    #[error("Injection detected: {0}")]
    InjectionBlocked(String),

    #[error("Leak detected")]
    LeakBlocked,

    #[error("LLM upstream error: {0}")]
    LlmError(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::QuotaExceeded => (
                StatusCode::PAYMENT_REQUIRED,
                "Quota exceeded. Please upgrade your plan or purchase a top-up.".into(),
            ),
            AppError::InjectionBlocked(_) => (
                StatusCode::FORBIDDEN,
                "Request blocked by security policy.".into(),
            ),
            AppError::LeakBlocked => (
                StatusCode::FORBIDDEN,
                "Response blocked by security policy.".into(),
            ),
            AppError::LlmError(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
            AppError::Database(e) => {
                tracing::error!("Database error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error".into())
            }
            AppError::Internal(e) => {
                tracing::error!("Internal error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error".into())
            }
        };

        let body = json!({ "error": message });
        (status, axum::Json(body)).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
