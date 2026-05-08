pub mod auth;
pub mod creator;
pub mod invoke;
pub mod skills;
pub mod subscriptions;

use axum::Router;

pub fn build_router() -> Router<crate::AppState> {
    Router::new()
        .merge(auth::routes())
        .merge(skills::routes())
        .merge(invoke::routes())
        .merge(subscriptions::routes())
        .merge(creator::routes())
}
