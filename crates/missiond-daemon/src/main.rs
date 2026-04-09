//! missiond - singleton daemon for missiond
//!
//! Responsibilities:
//! - Own the global state (DB, slot/process/task/inbox, PTY sessions, CC tasks watcher)
//! - Provide a stable WebSocket endpoint for attach + tasks events
//! - Expose an IPC JSON-RPC endpoint for MCP proxy processes

// ── Subdirectory modules ──
mod context;
mod engine;
mod infra;
mod llm;
mod workers;

// ── Root-level modules ──
#[allow(dead_code)]
mod control_tree;
mod event_bus;
mod event_router;
mod events_sync;
mod handlers;
mod helpers;
mod lenient;
mod slot_dispatch;
mod slot_orchestrator;
mod state;
mod supervisor;

// ── Re-exports for backward-compatible `use crate::xxx` paths ──
use context::{claude_md_sync, context_budget, context_pipeline, slot_env, topology_map};
use engine::{
    autopilot, decision_engine, decision_harvest, extraction, flow_engine, memory_scheduler,
};
use infra::{aiops, daemon_stats, ipc_handler, mcp_client};
use llm::{
    codex_cli, gemini_cli, gemini_client, llm_gate, llm_gateway, minimax_client, minimax_gateway,
    prompts, sonnet_gateway,
};
use workers::codex::{step_narrator, vision_worker};
use workers::local::{ast_sync_worker, code_prefetch, experience_harvester};
use workers::sonnet::{briefing_worker, embedding_worker, translation_worker};

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use missiond_core::{
    CCTasksWatcher, CCTasksWatcherOptions, GeminiCliWatcher, GeminiCliWatcherOptions,
};
use missiond_core::{
    InfraConfig, LearnedPermissions, MissionControl, MissionControlOptions, PTYManager,
    PTYWebSocketServer, PermissionPolicy, SkillIndex, WSServerOptions,
};
use missiond_mcp::tools::{all_tools, ToolResult};
use serde_json::Value;
use tokio::io::BufReader;
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, error, info, warn};

// Re-imports from extracted modules
use aiops::{health_scan, process_incident};
use autopilot::autopilot_tick;
use embedding_worker::init_embedding_provider;
use helpers::*;
use ipc_handler::{bind_ipc_listener, handle_ipc_connection};
use mcp_client::McpProcessClient;
use state::*;

impl AppState {
    pub(crate) async fn call_tool(&self, name: &str, args: Value) -> ToolResult {
        match self.call_tool_inner(name, args).await {
            Ok(res) => res,
            Err(e) => {
                error!(tool = %name, error = %e, "Tool call failed");
                ToolResult::error(e.to_string())
            }
        }
    }

    // execute_workflow is in engine/workflow_executor.rs

    async fn call_tool_inner(&self, name: &str, args: Value) -> Result<ToolResult> {
        handlers::dispatch_tool(self, name, args).await
    }
}

/// Phase 6: Timeline Writer — single consumer for MPSC, batch-writes to DB,
/// then broadcasts TimelineEvent to all consumers + serializes to WS String channel.
async fn run_timeline_writer(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<event_bus::TimelineEntry>,
    store: Arc<dyn missiond_core::db::traits::MissionStore>,
    timeline_tx: broadcast::Sender<event_bus::TimelineEvent>,
    ws_tx: broadcast::Sender<String>,
) {
    use event_bus::{TimelineEntry, TimelineEvent};

    loop {
        // Block until first event arrives
        let first = match rx.recv().await {
            Some(e) => e,
            None => break, // Channel closed
        };
        let mut batch: Vec<TimelineEntry> = vec![first];

        // Micro-batch: drain up to 100 ready events
        while batch.len() < 100 {
            match rx.try_recv() {
                Ok(e) => batch.push(e),
                Err(_) => break,
            }
        }

        let ts = chrono::Utc::now().timestamp_millis();

        // Split: ephemeral events skip DB, persistent events get written.
        let (persistent, ephemeral): (Vec<TimelineEntry>, Vec<TimelineEntry>) =
            batch.into_iter().partition(|e| !e.event.is_ephemeral());

        // Ephemeral: broadcast to WS + internal consumers without DB persistence.
        for entry in ephemeral {
            let te = TimelineEvent {
                seq: 0,
                trace_id: entry.trace_id,
                span_id: entry.span_id,
                parent_span_id: entry.parent_span_id,
                event: entry.event,
                summary: entry.summary,
                ts,
            };
            let _ = ws_tx.send(te.to_frontend_json());
            let _ = timeline_tx.send(te);
        }

        // Persistent: batch-write to DB, then broadcast with assigned seq.
        if persistent.is_empty() {
            continue;
        }

        let db_entries: Vec<(
            Option<String>,
            String,
            Option<String>,
            String,
            Option<String>,
            String,
        )> = persistent
            .iter()
            .map(|entry| {
                let payload_json = entry.event.to_frontend_payload().to_string();
                (
                    entry.trace_id.clone(),
                    entry.span_id.clone(),
                    entry.parent_span_id.clone(),
                    entry.event.wire_type().to_string(),
                    entry.summary.clone(),
                    payload_json,
                )
            })
            .collect();

        let params: Vec<(Option<&str>, &str, Option<&str>, &str, Option<&str>, &str)> = db_entries
            .iter()
            .map(|(t, s, p, e, sum, pay)| {
                (
                    t.as_deref(),
                    s.as_str(),
                    p.as_deref(),
                    e.as_str(),
                    sum.as_deref(),
                    pay.as_str(),
                )
            })
            .collect();

        let seqs = match store.insert_timeline_batch(&params).await {
            Ok(seqs) => seqs,
            Err(e) => {
                tracing::error!(error = %e, "Timeline Writer: batch insert failed");
                continue;
            }
        };

        for (entry, seq) in persistent.into_iter().zip(seqs) {
            let te = TimelineEvent {
                seq,
                trace_id: entry.trace_id,
                span_id: entry.span_id,
                parent_span_id: entry.parent_span_id,
                event: entry.event,
                summary: entry.summary,
                ts,
            };
            let _ = ws_tx.send(te.to_frontend_json());
            let _ = timeline_tx.send(te);
        }
    }
    tracing::warn!("Timeline Writer: MPSC channel closed, shutting down");
}

#[tokio::main]
async fn main() -> Result<()> {
    let home = default_mission_home();
    std::fs::create_dir_all(&home).ok();

    // Dual-layer logging: stderr + file (daily rotation)
    let log_dir = home.join("logs");
    std::fs::create_dir_all(&log_dir).ok();
    let file_appender = tracing_appender::rolling::daily(&log_dir, "missiond.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    tracing_subscriber::registry()
        .with(log_filter())
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .init();

    // Panic hook: log panic info before process exits (normal panic output goes to stderr
    // which may not be captured; this ensures it's in the tracing log file too).
    std::panic::set_hook(Box::new(|info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        eprintln!("PANIC at {}: {}", location, payload);
        tracing::error!(location = %location, "DAEMON PANIC: {}", payload);
    }));

    // Ensure config files have restrictive permissions
    #[cfg(unix)]
    ensure_config_permissions(&home);

    // M3: SQLite → PostgreSQL one-time migration CLI
    // Usage: MISSION_PG_URL=postgres://... missiond --migrate-sqlite-to-pg
    #[cfg(all(feature = "sqlite", feature = "postgres"))]
    if std::env::args().any(|a| a == "--migrate-sqlite-to-pg") {
        let sqlite_path = db_path().to_string_lossy().to_string();
        let pg_url =
            pg_url().ok_or_else(|| anyhow!("MISSION_PG_URL env var required for migration"))?;
        info!(sqlite = %sqlite_path, pg = %pg_url, "Starting SQLite → PostgreSQL migration");

        match missiond_core::db::pg::migrate_from_sqlite::migrate_sqlite_to_pg(
            &sqlite_path,
            &pg_url,
        )
        .await
        {
            Ok((tables, rows)) => {
                info!(tables, rows, "Migration complete");
                // Post-migration: backfill pgvector embedding columns from BYTEA
                match missiond_core::db::pg::migrate_from_sqlite::backfill_pg_embeddings(&pg_url)
                    .await
                {
                    Ok(total) => info!(backfilled = total, "Embedding backfill complete"),
                    Err(e) => {
                        warn!(error = %e, "Embedding backfill failed (non-fatal, can re-run)")
                    }
                }
                println!("Migration OK: {} tables, {} rows", tables, rows);
                return Ok(());
            }
            Err(e) => {
                eprintln!("Migration FAILED: {}", e);
                return Err(anyhow!("Migration failed: {}", e));
            }
        }
    }

    let db_path = db_path();
    let slots_path = slots_config_path();
    if !slots_path.exists() {
        return Err(anyhow!(
            "Slots config not found: {} (set MISSION_SLOTS_CONFIG or create slots.yaml)",
            slots_path.display()
        ));
    }

    let logs_dir = logs_dir(&db_path);
    let permission_config_path = db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("config")
        .join("permissions.yaml");
    let learned_db_path = db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("learned_permissions.db");
    let learned = match LearnedPermissions::new(&learned_db_path) {
        Ok(lp) => {
            info!(path = %learned_db_path.display(), "Learned permissions DB ready");
            Some(Arc::new(lp))
        }
        Err(e) => {
            warn!(error = %e, "Failed to init learned permissions DB, learning disabled");
            None
        }
    };
    let permission = Arc::new(PermissionPolicy::new_with_learned(
        &permission_config_path,
        learned.clone(),
    ));

    // In PG mode (MISSION_PG_URL set), skip SQLite entirely
    let mc_db_path = if pg_url().is_some() {
        None
    } else {
        Some(db_path.clone())
    };
    let mission = Arc::new(MissionControl::new(MissionControlOptions {
        db_path: mc_db_path,
        slots_config_path: slots_path.clone(),
        permission_config_path: None,
        logs_dir: Some(logs_dir.clone()),
        default_mode: None,
    })?);
    mission.start().await?;

    // Phase 6.5: Validate static slot IDs don't use reserved 'slot-dyn-' prefix
    for slot in mission.list_slots() {
        if slot.config.id.starts_with("slot-dyn-") {
            return Err(anyhow!(
                "Static slot '{}' uses reserved 'slot-dyn-' prefix. \
                 Dynamic slot IDs are system-managed. Please rename in slots.yaml.",
                slot.config.id
            ));
        }
    }

    // M4: Create store early (before startup cleanup) — conditional PG/SQLite
    let daemon_stats = Arc::new(daemon_stats::DaemonStats::new());
    let db_stats_callback: std::sync::Arc<dyn Fn(u64) + Send + Sync> = {
        let stats = Arc::clone(&daemon_stats);
        std::sync::Arc::new(move |elapsed_us| {
            stats
                .db_exec_runs
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            stats
                .db_exec_total_us
                .fetch_add(elapsed_us, std::sync::atomic::Ordering::Relaxed);
            stats.db_exec_latency.record(elapsed_us);
        })
    };

    let store: Arc<dyn missiond_core::db::traits::MissionStore> = {
        #[cfg(feature = "postgres")]
        {
            let url = pg_url().ok_or_else(|| {
                anyhow!("MISSION_PG_URL required. PostgreSQL is the default backend.")
            })?;
            info!(url = %url, "Connecting to PostgreSQL...");
            let pg_store = missiond_core::db::pg::PgMissionStore::connect(&url)
                .await
                .map_err(|e| anyhow!("PostgreSQL connection failed: {}", e))?;
            pg_store.fix_identity_sequences().await;
            info!("PostgreSQL store ready");
            let _ = db_stats_callback; // PG mode: latency tracked by sqlx instrumentation
            Arc::new(pg_store)
        }

        #[cfg(not(feature = "postgres"))]
        Arc::new(
            missiond_core::db::sqlite::SqliteMissionStore::with_callback(
                mission.db_arc(),
                db_stats_callback,
            ),
        )
    };

    // Startup: clean orphan slot_tasks from previous daemon instance
    match store.cleanup_orphan_slot_tasks().await {
        Ok(n) if n > 0 => info!(count = n, "Cleaned up orphan slot tasks from previous run"),
        Err(e) => warn!(error = %e, "Failed to cleanup orphan slot tasks"),
        _ => {}
    }

    // Phase 6.4: Recover stale running board tasks from previous daemon crash
    match store.recover_stale_running_tasks(0).await {
        Ok(n) if n > 0 => info!(count = n, "Startup: recovered stale running board tasks"),
        Err(e) => warn!(error = %e, "Failed to recover stale board tasks on startup"),
        _ => {}
    }

    // Phase 6.7: Terminate ALL active dynamic slots on daemon restart.
    // Dynamic slots are ephemeral — their PTY processes will be killed by the
    // orphan cleanup (pgrep MISSIOND_SLOT_ID) later in startup. Re-registering
    // them creates zombie slots (DB active, process dead). Clean slate is safer.
    match store.list_dynamic_slots(Some("active")).await {
        Ok(active) => {
            for s in &active {
                let _ = store.terminate_dynamic_slot(&s.id, "daemon_restart").await;
            }
            if !active.is_empty() {
                info!(
                    count = active.len(),
                    "Terminated active dynamic slots on startup (clean slate)"
                );
            }
        }
        Err(e) => warn!(error = %e, "Failed to cleanup dynamic slots on startup"),
    }

    // PTY manager setup
    let pty = Arc::new(PTYManager::new(logs_dir.clone()));
    pty.set_permission_policy(Arc::new(PermissionAdapter {
        permission: Arc::clone(&permission),
    }))
    .await;

    // Init PTY slots
    for slot in mission.list_slots() {
        let pty_slot = missiond_core::PTYSlot {
            id: slot.config.id.clone(),
            role: slot.config.role.clone(),
            cwd: slot.config.cwd.as_deref().map(PathBuf::from),
            engine: slot.config.engine,
        };
        pty.init_slot(&pty_slot).await;
    }

    // CC tasks watcher (with cursor persistence via store)
    let mut cc = CCTasksWatcher::new(CCTasksWatcherOptions {
        store: Some(Arc::clone(&store)),
        ..Default::default()
    });
    cc.start().await?;

    // Gemini CLI watcher: shares the same broadcast channel as CC watcher
    let mut gemini_watcher = GeminiCliWatcher::new(GeminiCliWatcherOptions {
        gemini_home: None,
        event_tx: cc.event_sender(),
        store: Some(Arc::clone(&store)),
    });
    if let Err(e) = gemini_watcher.start().await {
        warn!(error = %e, "Failed to start GeminiCliWatcher (non-fatal)");
    }

    let cc_tasks = Arc::new(Mutex::new(cc));
    let gemini_tasks = Arc::new(Mutex::new(gemini_watcher));

    // Conversation logger: subscribe to watcher events (processed in main select loop)
    // IMPORTANT: subscribe BEFORE run_startup_catchup() — catchup sends to broadcast channel,
    // receivers must exist or messages are silently lost (root cause of startup data loss).
    let conv_logger_rx = cc_tasks.lock().await.subscribe();

    // NOTE: run_startup_catchup() is deferred to AFTER ConversationLoggerWorker is spawned.
    // Receiver exists (subscribe above) but Worker must be actively consuming before catchup
    // sends messages — otherwise messages accumulate in broadcast buffer with no consumer.
    // PTY conversation logger: subscribe to manager events
    let pty_logger_rx = pty.subscribe();

    // AIOps: incident event bus (capacity 500, try_send only — "宁丢不阻塞")
    // Increased from 100 to handle MCP error burst scenarios without losing incidents.
    let (incident_tx, incident_rx) =
        tokio::sync::mpsc::channel::<missiond_core::types::MissionIncident>(500);

    // Embedding worker channel: event-driven, 0 CPU when idle
    let (embedding_tx, embedding_rx) = tokio::sync::mpsc::channel::<EmbeddingTask>(256);

    // AST sync worker channel: code indexing pipeline (P2 HCE)
    let (ast_sync_tx, ast_sync_rx) = tokio::sync::mpsc::channel::<ast_sync_worker::AstSyncTask>(64);

    // Screenshot broker (coordinates browser-based PTY screenshots)
    let screenshot_broker =
        missiond_core::ws::ScreenshotBroker::new(std::time::Duration::from_secs(5));

    // Phase 6: Timeline architecture
    // MPSC for event ingestion, broadcast<TimelineEvent> for fan-out to consumers + WS
    let (timeline_mpsc_tx, timeline_mpsc_rx) =
        tokio::sync::mpsc::unbounded_channel::<event_bus::TimelineEntry>();
    let (timeline_broadcast_tx, _) = broadcast::channel::<event_bus::TimelineEvent>(512);

    // Frontend event stream: TimelineEvent → JSON String → WS /events
    let (frontend_events_tx, _) = broadcast::channel::<String>(256);

    // WebSocket server (PTY attach + Tasks events + AIOps webhooks + EventBus stream)
    let ws_port = ws_port();
    let context_enricher_slot: missiond_core::ContextEnricherSlot =
        Arc::new(tokio::sync::RwLock::new(None));
    let mut ws_server = PTYWebSocketServer::new(WSServerOptions {
        port: ws_port,
        pty_manager: Some(Arc::clone(&pty)),
        cc_tasks_watcher: Some(Arc::clone(&cc_tasks)),
        screenshot_broker: Some(Arc::clone(&screenshot_broker)),
        incident_tx: Some(incident_tx.clone()),
        frontend_events_tx: Some(frontend_events_tx.clone()),
        db: Some(Arc::clone(&store)),
        context_enricher: Arc::clone(&context_enricher_slot),
        tool_count: all_tools().len(),
    });
    if let Err(e) = ws_server.start().await {
        // Match Node behavior: continue running even if WS is unavailable (e.g. port in use).
        warn!(port = ws_port, error = %e, "Failed to start WebSocket server");
    }

    // IPC server
    let endpoint = ipc_endpoint_from_env();
    let listener = bind_ipc_listener(&endpoint).await?;
    info!(endpoint = %endpoint, "missiond IPC listening");

    // Infrastructure registry
    let servers_path = home.join("servers.yaml");
    let infra_loaded = InfraConfig::load(&servers_path);
    info!(count = infra_loaded.servers.len(), path = %servers_path.display(), "Infra registry loaded");
    let infra = Arc::new(std::sync::RwLock::new(infra_loaded));

    // Skill index (scan ~/.claude/skills/)
    let skills_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("skills");
    let skills = Arc::new(SkillIndex::build(&skills_dir));
    info!(count = skills.list().len(), "Skill index loaded");

    // Skill Engine: ingest SKILL.md files into DB for FTS5 search
    // (deferred to after store creation — needs async trait access)

    // Warm PTY session UUID cache from store
    let existing_slot_sessions = store.get_all_slot_sessions().await.unwrap_or_default();
    let pty_uuids: HashSet<String> = existing_slot_sessions
        .iter()
        .map(|(_, session_id)| session_id.clone())
        .collect();
    if !pty_uuids.is_empty() {
        info!(count = pty_uuids.len(), "Loaded PTY session UUIDs from DB");
    }
    let pty_session_uuids_arc = Arc::new(tokio::sync::RwLock::new(pty_uuids));

    let event_bus_instance = Arc::new(event_bus::EventBus::new(timeline_mpsc_tx));

    // M1 Step 6: Skill ingest via async store (moved from pre-store section)
    let ingested = missiond_core::skill::ingest_skills(store.as_ref(), &skills_dir).await;
    info!(count = ingested, "Skill engine: ingested skills into DB");

    // Pre-parse llm.yaml for config flags needed by AppState
    let llm_config_parsed: Option<embedding_worker::LlmConfig> = {
        let llm_yaml = default_mission_home().join("llm.yaml");
        llm_yaml
            .exists()
            .then(|| std::fs::read_to_string(&llm_yaml).ok())
            .flatten()
            .and_then(|content| serde_yaml::from_str(&content).ok())
    };
    let backfill_enabled = llm_config_parsed
        .as_ref()
        .map(|c| c.backfill_enabled)
        .unwrap_or(false);
    let intent_analyst_enabled = llm_config_parsed
        .as_ref()
        .map(|c| c.intent_analyst_enabled)
        .unwrap_or(false);

    // P1 fix (Gemini audit): initialize ControlManager early so its restored
    // state can hydrate legacy AtomicBools during AppState construction.
    let (control_manager_instance, _control_rx) = control_tree::ControlManager::new(&home);
    let control_tree_snapshot = control_manager_instance.current();
    let control_manager_arc = Arc::new(control_manager_instance);

    // Hydrate legacy llm_gate from ControlTree (prevents startup split-brain)
    for (&provider, &paused) in &control_tree_snapshot.providers {
        if paused {
            if let Some(legacy) = provider.to_llm_provider() {
                llm_gate::set_disabled(legacy, true);
                info!(
                    provider = provider.as_str(),
                    "Hydrated legacy llm_gate from ControlTree"
                );
            }
        }
    }

    let state_cc_tasks = Arc::clone(&cc_tasks);

    // Pre-clone Arcs needed for initialization (moved into AppState below)
    let slot_mgr_pty = Arc::clone(&pty);
    let slot_mgr_store = Arc::clone(&store);
    let slot_mgr_pty2 = Arc::clone(&pty);
    let slot_mgr_store2 = Arc::clone(&store);
    let pty_for_gemini_transport = Arc::clone(&pty);
    let pending_spawns_for_slot: Arc<
        tokio::sync::RwLock<Vec<(String, String, String, tokio::time::Instant)>>,
    > = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    let state = AppState {
        mission,
        store: store.clone(),
        permission,
        pty,
        cc_tasks,
        skills,
        infra,
        infra_path: servers_path.clone(),
        pty_session_uuids: pty_session_uuids_arc.clone(),
        extraction_state: Arc::new(tokio::sync::RwLock::new(ExtractionState {
            phase: ExtractionPhase::Idle,
            active_type: None,
            phase_started_at: 0,
            current_deep_conv_id: None,
            watermark_targets: Vec::new(),
            current_task_id: None,
            current_slot_task_id: None,
            is_checkpoint: false,
            checkpoint_message_id: None,
            pending_served: false,
        })),
        slow_extraction_state: Arc::new(tokio::sync::RwLock::new(ExtractionState {
            phase: ExtractionPhase::Idle,
            active_type: None,
            phase_started_at: 0,
            current_deep_conv_id: None,
            watermark_targets: Vec::new(),
            current_task_id: None,
            current_slot_task_id: None,
            is_checkpoint: false,
            checkpoint_message_id: None,
            pending_served: false,
        })),
        memory_slot_busy_since: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        slow_slot_busy_since: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        claude_md_hash: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        last_supervisor_patrol_at: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        // memory_paused / memory_paused_at removed — ControlTree is the single source of truth.
        global_paused: Arc::new(std::sync::atomic::AtomicBool::new(
            home.join("global_paused").exists() || control_tree_snapshot.global_paused,
        )),
        global_paused_at: Arc::new(std::sync::atomic::AtomicI64::new({
            let flag = home.join("global_paused");
            if flag.exists() {
                std::fs::read_to_string(&flag)
                    .ok()
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .unwrap_or_else(|| chrono::Utc::now().timestamp())
            } else if control_tree_snapshot.global_paused {
                chrono::Utc::now().timestamp()
            } else {
                0
            }
        })),
        slot_fail_counts: Arc::new(std::sync::Mutex::new(HashMap::new())),
        task_cited_kbs: Arc::new(std::sync::Mutex::new(HashMap::new())),
        kb_cooccurrence_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        pending_compact_restart: Arc::new(std::sync::Mutex::new(HashSet::new())),
        session_task_bindings: Arc::new(std::sync::Mutex::new(HashMap::new())),
        config_file_locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        job_store: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        backfill_enabled: Arc::new(std::sync::atomic::AtomicBool::new(backfill_enabled)),
        intent_analyst_enabled,
        proactive_cooldowns: Arc::new(std::sync::Mutex::new(HashMap::new())),
        pending_slot_spawns: Arc::clone(&pending_spawns_for_slot),
        slot_current_model: Arc::new(std::sync::Mutex::new(HashMap::new())),
        screenshot_broker: Arc::clone(&screenshot_broker),
        jarvis_trace: ws_server.jarvis_trace_store().clone(),
        slot_last_responses: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        slot_progress: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        http_client: reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .expect("Failed to build HTTP client"),
        gemini: {
            // Check llm.yaml for provider config
            let llm_yaml = default_mission_home().join("llm.yaml");
            let event_tx = event_bus_instance.sender(); // mpsc::UnboundedSender<TimelineEntry>
            if llm_yaml.exists() {
                if let Ok(content) = std::fs::read_to_string(&llm_yaml) {
                    if let Ok(config) =
                        serde_yaml::from_str::<embedding_worker::LlmConfig>(&content)
                    {
                        if config.provider == "gemini-cli" {
                            let cli_cfg = config.gemini_cli.unwrap_or_default();
                            info!(binary = %cli_cfg.binary, model = %cli_cfg.model, "LLM provider: gemini-cli (PTY transport)");
                            // API key pool: hot-reloads from llm.yaml on each call, no restart needed
                            let initial_count = gemini_cli::resolve_apikey_pool().len();
                            info!(
                                count = initial_count,
                                "Gemini API key pool: {} keys (hot-reload enabled)", initial_count
                            );
                            let api_key_pool = std::sync::Arc::new(gemini_cli::ApiKeyPool::new());
                            // PTY transport: uses GeminiPtyDriver → PTYManager (unified)
                            let pty_cwd = std::env::current_dir()
                                .unwrap_or_else(|_| std::path::PathBuf::from("/"));
                            let gemini_driver_for_transport =
                                llm::gemini_driver::GeminiPtyDriver::new(
                                    pty_for_gemini_transport,
                                    store.clone(),
                                    pty_session_uuids_arc.clone(),
                                );
                            let pty_transport =
                                std::sync::Arc::new(llm::gemini_pty::GeminiPtyTransport::new(
                                    gemini_driver_for_transport,
                                    "slot-gemini-router".to_string(),
                                    pty_cwd,
                                ));
                            info!("Gemini PTY transport initialized (via GeminiPtyDriver → PTYManager)");
                            gemini_client::GeminiClient::with_cli(
                                gemini_cli::GeminiCli::new(
                                    cli_cfg.binary,
                                    cli_cfg.model,
                                    std::time::Duration::from_secs(cli_cfg.timeout),
                                    Some(api_key_pool),
                                )
                                .with_pty(pty_transport),
                                event_tx,
                            )
                        } else {
                            info!(provider = %config.provider, "LLM provider: HTTP router");
                            gemini_client::GeminiClient::new(event_tx)
                        }
                    } else {
                        gemini_client::GeminiClient::new(event_tx)
                    }
                } else {
                    gemini_client::GeminiClient::new(event_tx)
                }
            } else {
                gemini_client::GeminiClient::new(event_tx)
            }
        },
        minimax: {
            let gw = minimax_gateway::create_minimax_gateway(event_bus_instance.sender());
            if let Some((handle, gateway)) = gw {
                info!("MinimaxGateway initialized");
                tokio::spawn(gateway.run());
                Some(handle)
            } else {
                warn!("MinimaxGateway: API key not found, gateway disabled");
                None
            }
        },
        sonnet: {
            let (handle, gateway) =
                sonnet_gateway::create_sonnet_gateway(event_bus_instance.sender());
            info!("SonnetGateway initialized");
            tokio::spawn(gateway.run());
            Some(handle)
        },
        xjp_mcp: Arc::new(McpProcessClient::new(
            default_mission_home().join("xjp-mcp-config.json"),
        )),
        flow_in_progress: Arc::new(std::sync::Mutex::new(HashSet::new())),
        embedding_service: {
            // Build HTTP client first, then init provider (needs it for Ollama probe)
            // Note: we already have http_client above; reuse pattern via temp client
            let temp_client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap();
            init_embedding_provider(&temp_client).await
        },
        embedding_cache: missiond_core::embedding::new_cache(),
        conversation_topic_cache: missiond_core::embedding::new_topic_cache(),
        skill_embedding_cache: missiond_core::embedding::new_cache(),
        kb_search_cache: missiond_core::embedding::new_cache(),
        embedding_tx: embedding_tx,
        incident_tx: incident_tx.clone(),
        event_bus: Arc::clone(&event_bus_instance),
        stats: Arc::clone(&daemon_stats),
        prompts: Arc::new(prompts::PromptStore::load()),
        briefing_notify: Arc::new(tokio::sync::Notify::new()),
        strategy_notify: Arc::new(tokio::sync::Notify::new()),
        retro_notify: Arc::new(tokio::sync::Notify::new()),
        ast_sync_tx,
        ast_embedding_cache: missiond_core::embedding::new_cache(),
        last_msg_span: Arc::new(std::sync::Mutex::new(HashMap::new())),
        worker_registry: Arc::new(workers::WorkerRegistry::new()),
        control_manager: Arc::clone(&control_manager_arc),
        slot_dispatch: Arc::new(slot_dispatch::SlotDispatchGuard::new()),
        board_dispatch_notify: Arc::new(tokio::sync::Notify::new()),
        slot_manager: {
            let cc_mgr = Arc::new(slot_orchestrator::ClaudeCodeSlotManager::new(
                slot_mgr_pty,
                slot_mgr_store,
                pty_session_uuids_arc.clone(),
            ));
            let gemini_driver_for_slots = llm::gemini_driver::GeminiPtyDriver::new(
                slot_mgr_pty2,
                slot_mgr_store2.clone(),
                pty_session_uuids_arc.clone(),
            );
            let gemini_mgr = Arc::new(slot_orchestrator::GeminiCliSlotManager::new(
                gemini_driver_for_slots,
                slot_mgr_store2,
            ));
            Arc::new(slot_orchestrator::AgentSlotManager::new(
                vec![
                    (
                        missiond_core::types::CliEngine::ClaudeCode,
                        cc_mgr as Arc<dyn slot_orchestrator::EngineSlotManager>,
                    ),
                    (
                        missiond_core::types::CliEngine::Gemini,
                        gemini_mgr as Arc<dyn slot_orchestrator::EngineSlotManager>,
                    ),
                ],
                Arc::clone(&control_manager_arc),
            ))
        },
        gemini_watch_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        gemini_watch_handle: Arc::new(tokio::sync::Mutex::new(None)),
        gemini_watch_attempts: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        gemini_watch_started_at: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        cursor_ack_tx: {
            // Ack-based cursor: message_handler acks after PG INSERT → cursor_persist_loop persists.
            // This eliminates the data loss window where cursor advances before messages reach PG.
            // Ack routing: .json → GeminiCliWatcher, .jsonl → CCTasksWatcher (prevents cross-talk).
            let (ack_tx, mut ack_rx) = tokio::sync::mpsc::unbounded_channel::<(String, u64)>();
            let cc_tasks_ref = Arc::clone(&state_cc_tasks);
            let gemini_tasks_ref = Arc::clone(&gemini_tasks);
            tokio::spawn(async move {
                while let Some((path, offset)) = ack_rx.recv().await {
                    if path.ends_with(".json") {
                        // Gemini CLI session file → route to GeminiCliWatcher
                        let watcher = gemini_tasks_ref.lock().await;
                        watcher.persist_cursor_ack(&path, offset);
                    } else {
                        // JSONL file → route to CCTasksWatcher
                        let watcher = cc_tasks_ref.lock().await;
                        watcher.persist_cursor_ack(&path, offset);
                    }
                }
            });
            ack_tx
        },
    };

    // Register SlotManager task configs
    {
        use missiond_core::types::{CliEngine, Lifecycle};
        state
            .slot_manager
            .register(slot_orchestrator::SlotTaskConfig {
                task_type: "arch_maintenance".to_string(),
                engine: CliEngine::ClaudeCode,
                lifecycle: Lifecycle::Persistent,
                slot_id: Some("slot-arch-maint".to_string()),
                role: Some("arch-maint".to_string()),
                model: Some("claude-sonnet-4-6".to_string()),
                timeout: std::time::Duration::from_secs(600),
                cwd: std::path::PathBuf::from("<REPO_ROOT>"),
                skip_permissions: true,
            })
            .await?;
        state
            .slot_manager
            .register(slot_orchestrator::SlotTaskConfig {
                task_type: "strategy_analyst".to_string(),
                engine: CliEngine::Gemini,
                lifecycle: Lifecycle::Persistent,
                slot_id: Some("slot-gemini-strategy".to_string()),
                role: Some("strategy".to_string()),
                model: None, // Uses GEMINI_MODEL constant in controller
                timeout: std::time::Duration::from_secs(600),
                cwd: std::path::PathBuf::from("<REPO_ROOT>"),
                skip_permissions: true,
            })
            .await?;
        state
            .slot_manager
            .register(slot_orchestrator::SlotTaskConfig {
                task_type: "gemini_router".to_string(),
                engine: CliEngine::Gemini,
                lifecycle: Lifecycle::Persistent,
                slot_id: Some("slot-gemini-router".to_string()),
                role: Some("gemini-router".to_string()),
                model: None,
                timeout: std::time::Duration::from_secs(120),
                cwd: std::path::PathBuf::from("<REPO_ROOT>"),
                skip_permissions: true,
            })
            .await?;
        info!(
            "SlotManager: 3 tasks registered (arch_maintenance, strategy_analyst, gemini_router)"
        );
    }

    // Late-bind context enricher for Jarvis chat completions
    {
        let state_for_enricher = state.clone();
        let enricher: missiond_core::ContextEnricherFn = Arc::new(move |query: String| {
            let s = state_for_enricher.clone();
            Box::pin(async move {
                let req = context_pipeline::PrefetchRequest {
                    query,
                    source: context_pipeline::PrefetchSource::Jarvis,
                    token_budget: 4000,
                };
                let result = context_pipeline::execute(&s, &req).await;
                missiond_core::ContextEnrichResult {
                    assembled: result.assembled,
                    intent: result.intent,
                }
            })
        });
        *context_enricher_slot.write().await = Some(enricher);
        info!("Jarvis context enricher activated");
    }

    // Startup: kill orphan PTY processes from previous daemon instance.
    // When daemon is killed with SIGKILL or crashes, child Claude Code processes
    // become orphans and hold resources (file locks, ports). We detect them via
    // the MISSIOND_SLOT_ID env var injected into all PTY children.
    {
        use std::process::Command;
        // pgrep -f finds processes whose full command/env contains this marker
        match Command::new("pgrep")
            .args(["-f", "MISSIOND_SLOT_ID"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let pids: Vec<&str> = std::str::from_utf8(&output.stdout)
                    .unwrap_or("")
                    .lines()
                    .filter(|s| !s.is_empty())
                    .collect();
                if !pids.is_empty() {
                    let my_pid = std::process::id().to_string();
                    let orphan_pids: Vec<&&str> = pids.iter().filter(|p| **p != my_pid).collect();
                    if !orphan_pids.is_empty() {
                        warn!(
                            count = orphan_pids.len(),
                            "Found orphan PTY processes from previous daemon, killing"
                        );
                        for pid in &orphan_pids {
                            let _ = Command::new("kill").args(["-9", pid]).output();
                        }
                        // Brief wait for OS to reclaim resources
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        info!("Orphan PTY cleanup complete");
                    }
                }
            }
            _ => {} // pgrep not found or no matches — fine
        }
    }

    // Persistent slots: NO boot-spawn. Slots start on-demand when the first
    // task arrives (lazy-spawn in ClaudeCodeSlotMgr::execute_persistent).
    // This eliminates boot-time resource waste and ghost /exit sessions from restarts.
    {
        let slots = state.mission.list_slots();
        let persistent_count = slots.iter().filter(|s| s.config.is_persistent()).count();
        if persistent_count > 0 {
            info!(
                count = persistent_count,
                "Persistent slots registered (lazy-spawn on first task)"
            );
        }
    }

    // One-time backfill: populate conversation_events from historical JSONL files
    {
        let backfill_state = state.clone();
        tokio::spawn(async move {
            events_sync::backfill_conversation_events(&backfill_state).await;
        });
    }

    // One-time backfill: populate conversation_tool_calls from existing conversation_messages
    {
        let backfill_state = state.clone();
        tokio::spawn(async move {
            events_sync::backfill_tool_calls(&backfill_state).await;
        });
    }

    // One-time backfill: generate embeddings for policy:decision KB entries + warm cache
    if state.embedding_service.is_some() {
        let emb_state = state.clone();
        tokio::spawn(async move {
            let emb_svc = emb_state.embedding_service.as_ref().unwrap();
            match emb_state
                .store
                .kb_entries_missing_embedding(Some("policy:decision"))
                .await
            {
                Ok(missing) if !missing.is_empty() => {
                    info!(
                        count = missing.len(),
                        "Backfilling embeddings for policy:decision entries"
                    );
                    let mut stored = 0usize;
                    let provider_id = emb_svc.provider_id();
                    for (id, summary, detail) in &missing {
                        let text = format!("知识条目：{}\n详情：{}", summary, detail);
                        if let Some(vec) = emb_svc.embed(&text) {
                            if let Err(e) = emb_state
                                .store
                                .kb_set_embedding(id, &vec, provider_id)
                                .await
                            {
                                warn!(id = %id, error = %e, "Failed to store embedding");
                            } else {
                                stored += 1;
                            }
                        }
                    }
                    info!(stored, "Embedding backfill complete");
                }
                Ok(_) => {}
                Err(e) => warn!(error = %e, "Failed to scan for missing embeddings"),
            }
            // Warm the in-memory cache with all policy:decision embeddings
            match emb_state.store.kb_load_embeddings("policy:decision").await {
                Ok(all) => {
                    let mut guard = emb_state.embedding_cache.write().await;
                    *guard = all;
                    info!(count = guard.len(), "Embedding cache warmed");
                }
                Err(e) => warn!(error = %e, "Failed to warm embedding cache"),
            }
            // Warm full KB search cache (all categories)
            match emb_state.store.kb_load_all_embeddings().await {
                Ok(all) => {
                    let mut guard = emb_state.kb_search_cache.write().await;
                    *guard = all;
                    info!(
                        count = guard.len(),
                        "KB search cache warmed (all categories)"
                    );
                }
                Err(e) => warn!(error = %e, "Failed to warm KB search cache"),
            }
            // Warm AST embedding cache (P3: code prefetch hybrid search)
            match emb_state.store.ast_load_all_embeddings().await {
                Ok(all) => {
                    let mut guard = emb_state.ast_embedding_cache.write().await;
                    *guard = all;
                    info!(count = guard.len(), "AST embedding cache warmed");
                }
                Err(e) => warn!(error = %e, "Failed to warm AST embedding cache"),
            }
        });
    }

    // Warm embedding caches (one-shot) — TopicCache removed in P3 (pgvector replaces in-memory)
    {
        let conv_state = state.clone();
        tokio::spawn(async move {
            // Skill topic embedding cache (still in-memory, not migrated to pgvector)
            match conv_state.store.skill_load_topic_embeddings().await {
                Ok(all) => {
                    let mut guard = conv_state.skill_embedding_cache.write().await;
                    *guard = all;
                    info!(count = guard.len(), "Skill embedding cache warmed");
                }
                Err(e) => warn!(error = %e, "Failed to warm skill embedding cache"),
            }
        });
    }

    // One-time startup: trigger full backfill (covers timeline build + stale embeds)
    {
        let tx = state.embedding_tx.clone();
        tokio::spawn(async move {
            // Delay slightly to let caches warm first
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let _ = tx.try_send(EmbeddingTask::BackfillAll);
            info!("Startup BackfillAll triggered");
        });
    }

    // --- Background Workers (unified lifecycle via BackgroundWorker trait) ---
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Embedding Worker: event-driven actor (KB/Skill/Conv/AST embeddings + backfill)
    workers::spawn_worker(
        workers::sonnet::embedding_worker::EmbeddingLoopWorker { rx: embedding_rx },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // Gemini request log subscriber (Timeline → DB persistence)
    workers::spawn_worker(
        workers::local::gemini_logger::GeminiLoggerWorker {
            timeline_rx: timeline_broadcast_tx.subscribe(),
        },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    workers::spawn_worker(
        vision_worker::VisionWorker,
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );
    workers::spawn_worker(
        step_narrator::StepNarratorWorker,
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );
    if state.sonnet.is_some() {
        workers::spawn_worker(
            briefing_worker::BriefingWorker,
            Arc::new(state.clone()),
            shutdown_rx.clone(),
        );
        workers::spawn_worker(
            translation_worker::TranslationWorker {
                timeline_rx: timeline_broadcast_tx.subscribe(),
            },
            Arc::new(state.clone()),
            shutdown_rx.clone(),
        );
    } else {
        warn!("MinimaxGateway not available, briefing and translation workers disabled");
    }

    // --- P0: IPC listener in dedicated task (never starved by other work) ---
    // Previously inside the main select! loop — a single slow branch (e.g. 120s PTY spawn)
    // would starve accept(), causing Board 502 timeouts. Now fully isolated.
    {
        let ipc_state = state.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok(stream) => {
                        let reader = BufReader::new(stream);
                        let conn_state = ipc_state.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_ipc_connection(conn_state, reader).await {
                                warn!(error = %e, "IPC connection error");
                            }
                        });
                    }
                    Err(e) => {
                        // Previously used `result?` which killed the entire event loop on
                        // transient errors (e.g. fd exhaustion). Now we log and retry.
                        warn!(error = %e, "IPC accept error, retrying in 100ms");
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        });
    }
    info!("IPC listener started (isolated task)");

    // --- P1: Autopilot in dedicated task (long operations won't starve IPC) ---
    // autopilot_tick() calls check_slot_context_levels() which can block up to
    // N × 123s (3s kill wait + 120s spawn timeout per slot). Running in its own
    // task ensures the event-driven loop below remains responsive.
    {
        let auto_state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                // Wait for either: 60s timer tick OR slot-became-idle signal
                tokio::select! {
                    _ = interval.tick() => {
                        // Full autopilot tick: maintenance + dispatch
                        if let Err(e) = autopilot_tick(&auto_state).await {
                            warn!(error = %e, "Autopilot tick failed");
                        }
                    }
                    _ = auto_state.board_dispatch_notify.notified() => {
                        // Slot became idle — run board dispatch immediately (skip maintenance)
                        if let Err(e) = autopilot::dispatch_board_tasks(&auto_state).await {
                            warn!(error = %e, "Board dispatch (idle-triggered) failed");
                        }
                    }
                }
            }
        });
    }
    info!("Autopilot scheduler started (60s interval + idle-triggered, isolated task)");

    // --- AST Embedding Health Monitor (periodic self-healing, Gemini-reviewed) ---
    // Every 15 min: check AST coverage, trigger BackfillAll if gaps detected.
    {
        let health_state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                match health_state.store.ast_stats().await {
                    Ok(ast) if ast.total_nodes > 0 => {
                        let coverage = ast.embedded_nodes as f64 / ast.total_nodes as f64;
                        if ast.embedded_nodes < ast.total_nodes {
                            let gap = ast.total_nodes - ast.embedded_nodes;
                            info!(
                                coverage = %format!("{:.1}%", coverage * 100.0),
                                gap,
                                total = ast.total_nodes,
                                "AST embedding gap detected, triggering backfill"
                            );
                            let _ = health_state
                                .embedding_tx
                                .try_send(EmbeddingTask::BackfillAll);
                        } else {
                            debug!(
                                coverage = "100.0%",
                                total = ast.total_nodes,
                                "AST embedding health OK"
                            );
                        }
                    }
                    _ => {}
                }
            }
        });
    }
    info!("AST embedding health monitor started (15min interval)");

    // --- P3: Event-driven handlers via EventBus (Phase 2) ---
    // Extracted to event_router.rs to prevent main.rs from becoming a God Module (R4).
    event_router::start_event_consumers(&state, &timeline_broadcast_tx, shutdown_rx.clone());

    // --- Phase 6: Timeline Writer Task ---
    // MPSC → SQLite batch INSERT (get seq) → broadcast<TimelineEvent> + broadcast<String> (WS)
    {
        let rx = timeline_mpsc_rx;
        let timeline_tx = timeline_broadcast_tx.clone();
        let ws_tx = frontend_events_tx.clone();
        let store_clone = Arc::clone(&state.store);
        tokio::spawn(async move {
            run_timeline_writer(rx, store_clone, timeline_tx, ws_tx).await;
        });
    }

    // --- AST Sync Worker (P2 HCE) ---
    // BackgroundWorker: unified lifecycle + ControlTree pause/resume
    workers::spawn_worker(
        workers::local::ast_sync_worker::AstSyncWorker { rx: ast_sync_rx },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    {
        // Full sync at startup: trigger for all repos after delay
        let ast_tx2 = state.ast_sync_tx.clone();
        let slot_cwds2: Vec<String> = state
            .mission
            .list_slots()
            .into_iter()
            .filter_map(|s| s.config.cwd)
            .collect();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let repos = ast_sync_worker::collect_repo_roots(&slot_cwds2);
            for repo in repos {
                let name = repo
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if let Err(e) = ast_tx2
                    .send(ast_sync_worker::AstSyncTask::FullSync {
                        repo_path: repo,
                        repo_name: name,
                    })
                    .await
                {
                    tracing::warn!(err = %e, "AST: failed to queue full sync");
                }
            }
        });
    }

    // Health snapshot injector (synthetic, not persisted to timeline)
    {
        let ws_tx = frontend_events_tx.clone();
        let stats = Arc::clone(&state.stats);
        let s = state.clone();
        tokio::spawn(async move {
            let mut snapshot_interval = tokio::time::interval(std::time::Duration::from_secs(5));
            snapshot_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                snapshot_interval.tick().await;
                if ws_tx.receiver_count() == 0 {
                    continue;
                }
                let snap = stats.snapshot();
                let publish_count = s
                    .event_bus
                    .publish_count
                    .load(std::sync::atomic::Ordering::Relaxed);
                let memory_paused = s
                    .control_manager
                    .current()
                    .is_domain_paused(crate::control_tree::CtlDomain::Memory);
                let fast_lane = {
                    let es = s.extraction_state.read().await;
                    serde_json::json!({ "phase": format!("{:?}", es.phase), "type": es.active_type })
                };
                let slow_lane = {
                    let es = s.slow_extraction_state.read().await;
                    serde_json::json!({ "phase": format!("{:?}", es.phase), "type": es.active_type })
                };
                let payload = serde_json::json!({
                    "type": "health_snapshot",
                    "ts": chrono::Utc::now().timestamp_millis(),
                    "seq": -1,
                    "payload": {
                        "stats": snap,
                        "event_bus": { "publish_count": publish_count },
                        "memory": {
                            "paused": memory_paused,
                            "fast_lane": fast_lane,
                            "slow_lane": slow_lane,
                        },
                    }
                });
                let _ = ws_tx.send(payload.to_string());
            }
        });
    }
    info!(
        "Timeline Writer + Health snapshot started (ws://*:{}/events)",
        ws_port
    );

    // Timeline TTL cleanup on startup
    {
        let store = Arc::clone(&state.store);
        tokio::spawn(async move {
            match store.cleanup_timeline_ttl(7).await {
                Ok(deleted) if deleted > 0 => {
                    info!(deleted, "Timeline: cleaned up old entries (>7 days)")
                }
                _ => {}
            }
        });
    }

    // AIOps Reactor: consume incident events, debounce, triage, create board tasks
    {
        let s = state.clone();
        let mut rx = incident_rx;
        tokio::spawn(async move {
            while let Some(incident) = rx.recv().await {
                process_incident(&s, incident).await;
            }
        });
    }

    // AIOps CronSensor: health scan every 5 minutes
    {
        let s = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                health_scan(&s).await;
            }
        });
        info!("AIOps health scanner started (300s interval)");
    }

    // Conversation logger event stream
    workers::spawn_worker(
        workers::local::conversation_logger::ConversationLoggerWorker { conv_logger_rx },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // Startup catchup: MUST run AFTER ConversationLoggerWorker is spawned.
    // Worker is now actively consuming from broadcast channel, so catchup messages
    // (anchor_check + gap recovery) will be processed instead of silently lost.
    {
        let catchup_cc = Arc::clone(&state.cc_tasks);
        let catchup_gemini = Arc::clone(&gemini_tasks);
        tokio::spawn(async move {
            // Yield to let ConversationLoggerWorker's run_loop reach its first recv()
            tokio::task::yield_now().await;
            catchup_cc.lock().await.run_startup_catchup().await;
            catchup_gemini.lock().await.run_startup_catchup().await;
        });
    }

    // PTY manager event stream (state changes, confirm, exit, MCP errors)
    workers::spawn_worker(
        workers::local::pty_event_worker::PtyEventWorker {
            pty_rx: pty_logger_rx,
        },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // Retro Worker — Notify-driven session retrospective (Sonnet)
    workers::spawn_worker(
        workers::sonnet::retro_worker::RetroWorker {
            notify: Arc::clone(&state.retro_notify),
        },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // Architecture maintenance worker — auto-updates YAML manifests on structural code changes
    workers::spawn_worker(
        workers::sonnet::arch_maintenance_worker::ArchMaintenanceWorker {
            timeline_rx: timeline_broadcast_tx.subscribe(),
        },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // Strategy Worker — Notify-driven strategic analysis (Gemini CLI)
    workers::spawn_worker(
        workers::gemini::strategy_worker::StrategyWorker {
            notify: Arc::clone(&state.strategy_notify),
        },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // Daily reconcile worker — JSONL-to-DB integrity checker (safety net for missed FSEvents)
    workers::spawn_worker(
        workers::local::reconcile_worker::ReconcileWorker,
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // Gemini CLI reconcile worker — ~/.gemini/tmp/*/chats/*.json integrity checker
    workers::spawn_worker(
        workers::local::gemini_reconcile_worker::GeminiReconcileWorker,
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // Conversation Organizer — Stage 2 of Cognitive Pipeline
    // Repairs parent links, splices compaction fragments, emits SessionOrganized
    workers::spawn_worker(
        workers::local::conversation_organizer::ConversationOrganizerWorker {
            timeline_rx: timeline_broadcast_tx.subscribe(),
        },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // Tagger & Chunker — Stage 3 of Cognitive Pipeline
    // Extracts structured Turns from flat messages, applies noise labels
    workers::spawn_worker(
        workers::local::tagger_chunker::TaggerChunkerWorker {
            timeline_rx: timeline_broadcast_tx.subscribe(),
        },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    info!("All event handlers started (isolated tasks)");

    // ---- Slots hot-reload: SIGHUP + fsnotify ----

    // Helper: reload slots and auto-start newly added ones
    async fn handle_slots_reload(state: &AppState) {
        match state.mission.reload_slots_config() {
            Ok(result) => {
                if result.has_changes() {
                    // Kill PTY sessions for removed slots (prevent orphan processes)
                    for slot_id in &result.removed {
                        info!(slot_id = %slot_id, "Killing PTY for removed slot");
                        if let Err(e) = state.pty.kill(slot_id).await {
                            warn!(slot_id = %slot_id, error = %e, "Failed to kill removed slot PTY");
                        }
                    }

                    // Auto-start newly added persistent slots (via PTY)
                    for slot_id in &result.added {
                        if let Some(slot) = state.mission.get_slot(slot_id) {
                            if slot.config.is_persistent() {
                                info!(slot_id = %slot_id, "Auto-starting newly added slot via PTY");
                                let pty_slot = missiond_core::PTYSlot {
                                    id: slot.config.id.clone(),
                                    role: slot.config.role.clone(),
                                    cwd: slot.config.cwd.as_deref().map(std::path::PathBuf::from),
                                    engine: slot.config.engine,
                                };
                                let mcp_config =
                                    slot.config.mcp_config.clone().map(std::path::PathBuf::from);
                                match crate::slot_orchestrator::spawner::spawn_tracked_slot(
                                    &state.pty,
                                    &state.store,
                                    &state.pty_session_uuids,
                                    &pty_slot,
                                    missiond_core::PTYSpawnOptions {
                                        auto_restart: true,
                                        wait_for_idle: false,
                                        timeout_secs: None,
                                        mcp_config,
                                        dangerously_skip_permissions: slot
                                            .config
                                            .dangerously_skip_permissions
                                            .unwrap_or(false),
                                        model: slot.config.model.clone(),
                                        extra_env: std::collections::HashMap::new(),
                                        initial_prompt: slot.config.initial_prompt.clone(),
                                    },
                                    slot.config.env.as_ref(),
                                )
                                .await
                                {
                                    Ok(_) => {
                                        info!(slot_id = %slot_id, "Auto-started new slot PTY");
                                    }
                                    Err(e) => {
                                        warn!(slot_id = %slot_id, error = %e, "Failed to auto-start new slot PTY")
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to reload slots.yaml");
            }
        }
    }

    // SIGHUP handler (Unix only)
    #[cfg(unix)]
    {
        let sig_state = state.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sighup =
                signal(SignalKind::hangup()).expect("Failed to register SIGHUP handler");
            loop {
                sighup.recv().await;
                info!("SIGHUP received, reloading slots.yaml and servers.yaml");
                handle_slots_reload(&sig_state).await;
                // Also reload infra config
                let new_infra = InfraConfig::load(&sig_state.infra_path);
                info!(count = new_infra.servers.len(), "Infra registry reloaded");
                *sig_state.infra.write().unwrap() = new_infra;
            }
        });
    }

    // Perception Layer: config file watcher (slots.yaml + servers.yaml + prompts + MCP config)
    {
        let watch_state = state.clone();
        let watch_path = slots_path.clone();
        let mission_home = helpers::default_mission_home();
        tokio::spawn(async move {
            use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

            let (tx, mut rx) = tokio::sync::mpsc::channel::<Event>(32);

            let mut watcher = match RecommendedWatcher::new(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(event) = res {
                        let _ = tx.blocking_send(event);
                    }
                },
                Config::default(),
            ) {
                Ok(w) => w,
                Err(e) => {
                    error!(error = %e, "Failed to create config file watcher");
                    return;
                }
            };

            // Watch the mission home directory (non-recursive) for config files
            let watch_dir = watch_path.parent().unwrap_or(&watch_path);
            if let Err(e) = watcher.watch(watch_dir, RecursiveMode::NonRecursive) {
                error!(error = %e, path = %watch_dir.display(), "Failed to watch config directory");
                return;
            }

            // Watch prompts directory (recursive) if it exists
            let prompts_dir = mission_home.join("prompts");
            if prompts_dir.exists() {
                if let Err(e) = watcher.watch(&prompts_dir, RecursiveMode::Recursive) {
                    warn!(error = %e, "Failed to watch prompts directory");
                }
            }

            // Monitored config files (emit Timeline events on change)
            let monitored_files: std::collections::HashSet<&str> =
                ["slots.yaml", "servers.yaml", "xjp-mcp-config.json"]
                    .into_iter()
                    .collect();

            info!(path = %watch_dir.display(), "Perception: watching config files for changes");

            // Debounce per file: wait 500ms after last event
            let mut debounce_slots: Option<tokio::time::Instant> = None;
            let mut pending_config_events: Vec<(String, String)> = Vec::new();
            let mut debounce_config: Option<tokio::time::Instant> = None;

            loop {
                tokio::select! {
                    Some(event) = rx.recv() => {
                        let is_relevant = matches!(
                            event.kind,
                            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                        );
                        if !is_relevant { continue; }

                        let kind = match event.kind {
                            EventKind::Remove(_) => "deleted",
                            EventKind::Create(_) => "created",
                            _ => "modified",
                        };

                        for path in &event.paths {
                            let file_name = path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("");

                            // slots.yaml: special handling (reload slots)
                            if file_name == "slots.yaml" && kind != "deleted" {
                                debounce_slots = Some(tokio::time::Instant::now() + std::time::Duration::from_millis(500));
                            }

                            // Any monitored config file OR prompts dir: emit Timeline event
                            let is_config = monitored_files.contains(file_name);
                            let is_prompt = path.starts_with(&prompts_dir);
                            if is_config || is_prompt {
                                let display = path.display().to_string();
                                pending_config_events.push((display, kind.to_string()));
                                debounce_config = Some(tokio::time::Instant::now() + std::time::Duration::from_millis(500));
                            }
                        }
                    }
                    _ = async {
                        // Pick the earliest deadline
                        match (debounce_slots, debounce_config) {
                            (Some(a), Some(b)) => tokio::time::sleep_until(a.min(b)).await,
                            (Some(a), None) => tokio::time::sleep_until(a).await,
                            (None, Some(b)) => tokio::time::sleep_until(b).await,
                            (None, None) => std::future::pending().await,
                        }
                    } => {
                        let now = tokio::time::Instant::now();

                        // Handle slots.yaml reload
                        if let Some(deadline) = debounce_slots {
                            if now >= deadline {
                                debounce_slots = None;
                                info!("slots.yaml changed on disk, reloading");
                                handle_slots_reload(&watch_state).await;
                            }
                        }

                        // Handle config change events
                        if let Some(deadline) = debounce_config {
                            if now >= deadline {
                                debounce_config = None;
                                let events = std::mem::take(&mut pending_config_events);
                                for (path, kind) in events {
                                    info!(path = %path, kind = %kind, "Config file changed");
                                    watch_state.event_bus.publish(
                                        event_bus::DaemonEvent::ConfigFileChanged {
                                            path: path.clone(),
                                            kind: kind.clone(),
                                        },
                                    );

                                    // Hot-reload prompts if a prompt file changed
                                    if path.contains("/prompts/") {
                                        watch_state.prompts.reload();
                                        info!("Prompts hot-reloaded from file change");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    // Keep main alive — all work is in spawned tasks above.
    // Ctrl+C or SIGTERM triggers graceful shutdown.
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to register SIGTERM");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Received SIGINT (Ctrl+C), starting graceful shutdown");
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM, starting graceful shutdown");
            }
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.ok();
        info!("Received shutdown signal, starting graceful shutdown");
    }

    // Hard deadline: if graceful shutdown takes too long, force exit.
    // spawn_blocking(child.wait()) can wedge the tokio runtime, making the
    // process unkillable (UE state). This watchdog guarantees we always exit.
    let sock_for_watchdog = endpoint.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(8));
        // Clean up socket before forced exit
        let _ = std::fs::remove_file(&sock_for_watchdog);
        eprintln!("Shutdown watchdog: graceful shutdown exceeded 8s, forcing exit");
        std::process::exit(1);
    });

    // Phase 1: Notify workers to stop
    let _ = shutdown_tx.send(true);

    // Phase 2: Gracefully shut down all PTY sessions
    // This sends /exit to each Claude Code instance, waits 3s, then force-kills
    info!("Shutting down PTY sessions...");
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        state.pty.shutdown(),
    ).await {
        Ok(()) => info!("PTY sessions shut down cleanly"),
        Err(_) => warn!("PTY shutdown timed out after 5s, proceeding with cleanup"),
    }

    // Phase 3: Clean up IPC socket
    let sock_path = endpoint.clone();
    if std::path::Path::new(&sock_path).exists() {
        let _ = std::fs::remove_file(&sock_path);
        debug!("Removed IPC socket: {}", sock_path);
    }

    info!("Graceful shutdown complete");
    Ok(())
}
mod services;
