mod config;
mod db;
mod error;
mod middleware;
mod models;
mod routes;
mod services;

use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::services::billing::BillingService;
use crate::services::defense::DefenseLayer;
use crate::services::executor::SkillExecutor;
use crate::services::llm_proxy::LlmProxy;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<db::Database>,
    pub executor: Arc<SkillExecutor>,
    pub jwt_secret: String,
}

impl AsRef<AppState> for AppState {
    fn as_ref(&self) -> &AppState {
        self
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "skill_store=info,tower_http=info".into()),
        )
        .init();

    let config = config::Config::load()?;
    tracing::info!("Starting Skill Store on {}", config.bind);

    let database_url = config
        .database_url
        .as_deref()
        .expect("Config::load validates database_url");
    let database = Arc::new(db::Database::connect(database_url).await?);
    tracing::info!("PostgreSQL database initialized");

    // LLM proxy
    let llm = LlmProxy::new(config.llm.providers.clone());

    // Defense layer
    let defense = Arc::new(DefenseLayer::new(
        config.defense.clone(),
        llm.clone(),
        config.llm.clewdr_endpoint.clone(),
    ));

    // Billing
    let billing = Arc::new(BillingService::new(config.platform_cut));

    // Executor
    let executor = Arc::new(SkillExecutor {
        llm,
        defense,
        billing: billing.clone(),
        db: database.clone(),
    });

    let state = AppState {
        db: database.clone(),
        executor,
        jwt_secret: config.jwt_secret.clone(),
    };

    // Revenue settlement cron (every hour)
    let db_for_cron = database.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            match services::billing::settle_creator_revenues(&db_for_cron).await {
                Ok(n) if n > 0 => tracing::info!("Settled {n} revenue entries"),
                Err(e) => tracing::error!("Revenue settlement error: {e}"),
                _ => {}
            }
        }
    });

    let app = routes::build_router()
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    tracing::info!("Skill Store listening on {}", config.bind);
    axum::serve(listener, app).await?;

    Ok(())
}
