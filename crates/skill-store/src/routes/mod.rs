pub mod auth;
pub mod skills;
pub mod invoke;
pub mod subscriptions;
pub mod creator;

use axum::Router;

pub fn build_router() -> Router<crate::AppState> {
    Router::new()
        .merge(auth::routes())
        .merge(skills::routes())
        .merge(invoke::routes())
        .merge(subscriptions::routes())
        .merge(creator::routes())
}
