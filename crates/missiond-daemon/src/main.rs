//! missiond - singleton daemon for missiond
//!
//! Responsibilities:
//! - Own the global state (DB, slot/process/task/inbox, PTY sessions, CC tasks watcher)
//! - Provide a stable WebSocket endpoint for attach + tasks events
//! - Expose an IPC JSON-RPC endpoint for MCP proxy processes

// ── Subdirectory modules ──
mod llm;
mod workers;
mod engine;
mod context;
mod infra;

// ── Root-level modules ──
mod events_sync;
mod lenient;
mod state;
mod helpers;
mod handlers;
mod event_bus;
mod event_router;
mod supervisor;
mod slot_dispatch;

// ── Re-exports for backward-compatible `use crate::xxx` paths ──
use llm::{gemini_client, gemini_cli, minimax_client, minimax_gateway, codex_cli, llm_gateway, prompts};
use workers::{embedding_worker, vision_worker, step_narrator, translation_worker, briefing_worker, code_prefetch, experience_harvester, ast_sync_worker};
use engine::{autopilot, decision_engine, decision_harvest, flow_engine, extraction, memory_scheduler};
use context::{slot_env, context_pipeline, claude_md_sync, topology_map, context_budget};
use infra::{ipc_handler, aiops, mcp_client, daemon_stats, git_watcher};

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use missiond_core::{
    MissionControl, MissionControlOptions, PermissionPolicy, LearnedPermissions,
    PTYManager, PTYWebSocketServer, WSServerOptions, SkillIndex, InfraConfig,
};
use missiond_core::{CCTasksWatcher, CCTasksWatcherOptions};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;
use tokio::io::BufReader;
use tokio::sync::{Mutex, broadcast};
use tracing::{debug, error, info, warn};

// Re-imports from extracted modules
use state::*;
use mcp_client::McpProcessClient;
use helpers::*;
use ipc_handler::{handle_ipc_connection, bind_ipc_listener};
use embedding_worker::init_embedding_provider;
use autopilot::autopilot_tick;
use aiops::{process_incident, health_scan};

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

/// Phase 6: Timeline Writer — single consumer for MPSC, batch-writes to SQLite,
/// then broadcasts TimelineEvent to all consumers + serializes to WS String channel.
async fn run_timeline_writer(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<event_bus::TimelineEntry>,
    db: Arc<missiond_core::MissionDB>,
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

        // Batch-write to SQLite in a single transaction (spawn_blocking for sync rusqlite)
        let db_clone = Arc::clone(&db);
        let db_entries: Vec<(Option<String>, String, Option<String>, String, Option<String>, String)> = batch
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

        let seqs = tokio::task::spawn_blocking(move || {
            let params: Vec<(Option<&str>, &str, Option<&str>, &str, Option<&str>, &str)> = db_entries
                .iter()
                .map(|(t, s, p, e, sum, pay)| {
                    (t.as_deref(), s.as_str(), p.as_deref(), e.as_str(), sum.as_deref(), pay.as_str())
                })
                .collect();
            db_clone.insert_timeline_batch(&params)
        }).await;

        let seqs = match seqs {
            Ok(Ok(seqs)) => seqs,
            Ok(Err(e)) => {
                tracing::error!(error = %e, "Timeline Writer: SQLite batch insert failed");
                continue;
            }
            Err(e) => {
                tracing::error!(error = %e, "Timeline Writer: spawn_blocking panicked");
                continue;
            }
        };

        let ts = chrono::Utc::now().timestamp_millis();

        // Broadcast each event with its persistent seq
        for (entry, seq) in batch.into_iter().zip(seqs) {
            let te = TimelineEvent {
                seq,
                trace_id: entry.trace_id,
                span_id: entry.span_id,
                parent_span_id: entry.parent_span_id,
                event: entry.event,
                summary: entry.summary,
                ts,
            };

            // Forward to WS clients as JSON string
            let json_str = te.to_frontend_json();
            let _ = ws_tx.send(json_str);

            // Broadcast to internal consumers (event_router, gemini log, etc.)
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
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr),
        )
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
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        eprintln!("PANIC at {}: {}", location, payload);
        tracing::error!(location = %location, "DAEMON PANIC: {}", payload);
    }));

    // Ensure config files have restrictive permissions
    #[cfg(unix)]
    ensure_config_permissions(&home);

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
    let permission = Arc::new(PermissionPolicy::new_with_learned(&permission_config_path, learned.clone()));

    let mission = Arc::new(MissionControl::new(MissionControlOptions {
        db_path: db_path.clone(),
        slots_config_path: slots_path.clone(),
        permission_config_path: None,
        logs_dir: Some(logs_dir.clone()),
        default_mode: None,
    })?);
    mission.start().await?;

    // Startup: clean orphan slot_tasks from previous daemon instance
    match mission.db().cleanup_orphan_slot_tasks() {
        Ok(n) if n > 0 => info!(count = n, "Cleaned up orphan slot tasks from previous run"),
        Err(e) => warn!(error = %e, "Failed to cleanup orphan slot tasks"),
        _ => {}
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

    // CC tasks watcher
    let mut cc = CCTasksWatcher::new(CCTasksWatcherOptions::default());
    cc.start().await?;
    let cc_tasks = Arc::new(Mutex::new(cc));

    // Conversation logger: subscribe to watcher events (processed in main select loop)
    let conv_logger_rx = cc_tasks.lock().await.subscribe();
    // PTY conversation logger: subscribe to manager events
    let pty_logger_rx = pty.subscribe();

    // AIOps: incident event bus (capacity 500, try_send only — "宁丢不阻塞")
    // Increased from 100 to handle MCP error burst scenarios without losing incidents.
    let (incident_tx, incident_rx) = tokio::sync::mpsc::channel::<missiond_core::types::MissionIncident>(500);

    // Embedding worker channel: event-driven, 0 CPU when idle
    let (embedding_tx, embedding_rx) = tokio::sync::mpsc::channel::<EmbeddingTask>(256);

    // AST sync worker channel: code indexing pipeline (P2 HCE)
    let (ast_sync_tx, ast_sync_rx) = tokio::sync::mpsc::channel::<ast_sync_worker::AstSyncTask>(64);

    // Screenshot broker (coordinates browser-based PTY screenshots)
    let screenshot_broker = missiond_core::ws::ScreenshotBroker::new(
        std::time::Duration::from_secs(5),
    );

    // Phase 6: Timeline architecture
    // MPSC for event ingestion, broadcast<TimelineEvent> for fan-out to consumers + WS
    let (timeline_mpsc_tx, timeline_mpsc_rx) = tokio::sync::mpsc::unbounded_channel::<event_bus::TimelineEntry>();
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
        db: Some(mission.db_arc()),
        context_enricher: Arc::clone(&context_enricher_slot),
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
    let ingested = missiond_core::skill::ingest_skills(mission.db(), &skills_dir);
    info!(count = ingested, "Skill engine: ingested skills into DB");

    // Warm PTY session UUID cache from DB (activates slot_sessions table)
    let existing_slot_sessions = mission.db().get_all_slot_sessions().unwrap_or_default();
    let pty_uuids: HashSet<String> = existing_slot_sessions
        .iter()
        .map(|(_, session_id)| session_id.clone())
        .collect();
    if !pty_uuids.is_empty() {
        info!(count = pty_uuids.len(), "Loaded PTY session UUIDs from DB");
    }

    let event_bus_instance = Arc::new(event_bus::EventBus::new(timeline_mpsc_tx));
    let daemon_stats = Arc::new(daemon_stats::DaemonStats::new());
    let mut db_exec = missiond_core::DbExecutor::new(mission.db_arc());
    {
        let stats = Arc::clone(&daemon_stats);
        db_exec.set_on_run(std::sync::Arc::new(move |elapsed_us| {
            stats.db_exec_runs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            stats.db_exec_total_us.fetch_add(elapsed_us, std::sync::atomic::Ordering::Relaxed);
            stats.db_exec_latency.record(elapsed_us);
        }));
    }

    let state = AppState {
        mission,
        permission,
        pty,
        cc_tasks,
        skills,
        infra,
        infra_path: servers_path.clone(),
        pty_session_uuids: Arc::new(tokio::sync::RwLock::new(pty_uuids)),
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
        memory_paused: Arc::new(std::sync::atomic::AtomicBool::new(
            home.join("memory_paused").exists()
        )),
        memory_paused_at: Arc::new(std::sync::atomic::AtomicI64::new({
            let flag = home.join("memory_paused");
            if flag.exists() {
                std::fs::read_to_string(&flag).ok()
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .unwrap_or_else(|| chrono::Utc::now().timestamp())
            } else {
                0
            }
        })),
        global_paused: Arc::new(std::sync::atomic::AtomicBool::new(
            home.join("global_paused").exists()
        )),
        global_paused_at: Arc::new(std::sync::atomic::AtomicI64::new({
            let flag = home.join("global_paused");
            if flag.exists() {
                std::fs::read_to_string(&flag).ok()
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .unwrap_or_else(|| chrono::Utc::now().timestamp())
            } else {
                0
            }
        })),
        slot_fail_counts: Arc::new(std::sync::Mutex::new(HashMap::new())),
        task_cited_kbs: Arc::new(std::sync::Mutex::new(HashMap::new())),
        pending_compact_restart: Arc::new(std::sync::Mutex::new(HashSet::new())),
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
            let event_tx = event_bus_instance.sender();  // mpsc::UnboundedSender<TimelineEntry>
            if llm_yaml.exists() {
                if let Ok(content) = std::fs::read_to_string(&llm_yaml) {
                    if let Ok(config) = serde_yaml::from_str::<embedding_worker::LlmConfig>(&content) {
                        if config.provider == "gemini-cli" {
                            let cli_cfg = config.gemini_cli.unwrap_or_default();
                            info!(binary = %cli_cfg.binary, model = %cli_cfg.model, "LLM provider: gemini-cli");
                            gemini_client::GeminiClient::with_cli(gemini_cli::GeminiCli::new(
                                cli_cfg.binary,
                                cli_cfg.model,
                                std::time::Duration::from_secs(cli_cfg.timeout),
                            ), event_tx)
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
        db_exec,
        stats: Arc::clone(&daemon_stats),
        prompts: Arc::new(prompts::PromptStore::load()),
        briefing_notify: Arc::new(tokio::sync::Notify::new()),
        ast_sync_tx,
        ast_embedding_cache: missiond_core::embedding::new_cache(),
        last_msg_span: Arc::new(std::sync::Mutex::new(HashMap::new())),
        worker_registry: Arc::new(workers::WorkerRegistry::new()),
        slot_dispatch: Arc::new(slot_dispatch::SlotDispatchGuard::new()),
        board_dispatch_notify: Arc::new(tokio::sync::Notify::new()),
        gemini_watch_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        gemini_watch_handle: Arc::new(tokio::sync::Mutex::new(None)),
        gemini_watch_attempts: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        gemini_watch_started_at: Arc::new(std::sync::atomic::AtomicI64::new(0)),
    };

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

    // Auto-spawn persistent slots (all via PTYManager)
    {
        let slots = state.mission.list_slots();
        for slot in &slots {
            if slot.config.is_persistent() {
                info!(slot_id = %slot.config.id, role = %slot.config.role, "Auto-starting slot on daemon boot");
                let pty_slot = missiond_core::PTYSlot {
                    id: slot.config.id.clone(),
                    role: slot.config.role.clone(),
                    cwd: slot.config.cwd.as_deref().map(std::path::PathBuf::from),
                    engine: slot.config.engine,
                };
                let mcp_config = slot.config.mcp_config.clone().map(std::path::PathBuf::from);
                let (extra_env, session_file) = slot_env::build_slot_tracking_env(&slot.config.id, slot.config.env.as_ref()).await;
                match state.pty.spawn(&pty_slot, missiond_core::PTYSpawnOptions {
                    auto_restart: true,
                    wait_for_idle: false,
                    timeout_secs: None,
                    mcp_config,
                    dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
                    model: slot.config.model.clone(),
                    extra_env,
                }).await {
                    Ok(_) => {
                        slot_env::capture_slot_session_uuid(&state, &slot.config.id, &session_file).await;
                        info!(slot_id = %slot.config.id, "Auto-started PTY session");
                    }
                    Err(e) => warn!(slot_id = %slot.config.id, error = %e, "Failed to auto-start PTY"),
                }
            }
        }
    }

    // One-time backfill: populate conversation_events from historical JSONL files
    {
        let backfill_state = state.clone();
        tokio::spawn(async move {
            events_sync::backfill_conversation_events(backfill_state.mission.db()).await;
        });
    }

    // One-time backfill: populate conversation_tool_calls from existing conversation_messages
    {
        let backfill_state = state.clone();
        tokio::spawn(async move {
            events_sync::backfill_tool_calls(backfill_state.mission.db()).await;
        });
    }

    // One-time backfill: generate embeddings for policy:decision KB entries + warm cache
    if state.embedding_service.is_some() {
        let emb_state = state.clone();
        tokio::spawn(async move {
            let db = emb_state.mission.db();
            let emb_svc = emb_state.embedding_service.as_ref().unwrap();
            match db.kb_entries_missing_embedding(Some("policy:decision")) {
                Ok(missing) if !missing.is_empty() => {
                    info!(count = missing.len(), "Backfilling embeddings for policy:decision entries");
                    let mut stored = 0usize;
                    let provider_id = emb_svc.provider_id();
                    for (id, summary, detail) in &missing {
                        let text = format!("知识条目：{}\n详情：{}", summary, detail);
                        if let Some(vec) = emb_svc.embed(&text) {
                            if let Err(e) = db.kb_set_embedding(id, &vec, provider_id) {
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
            match db.kb_load_embeddings("policy:decision") {
                Ok(all) => {
                    let mut guard = emb_state.embedding_cache.write().await;
                    *guard = all;
                    info!(count = guard.len(), "Embedding cache warmed");
                }
                Err(e) => warn!(error = %e, "Failed to warm embedding cache"),
            }
            // Warm full KB search cache (all categories)
            match db.kb_load_all_embeddings() {
                Ok(all) => {
                    let mut guard = emb_state.kb_search_cache.write().await;
                    *guard = all;
                    info!(count = guard.len(), "KB search cache warmed (all categories)");
                }
                Err(e) => warn!(error = %e, "Failed to warm KB search cache"),
            }
            // Warm AST embedding cache (P3: code prefetch hybrid search)
            match db.ast_load_all_embeddings() {
                Ok(all) => {
                    let mut guard = emb_state.ast_embedding_cache.write().await;
                    *guard = all;
                    info!(count = guard.len(), "AST embedding cache warmed");
                }
                Err(e) => warn!(error = %e, "Failed to warm AST embedding cache"),
            }
        });
    }

    // Warm conversation embedding cache (one-shot)
    {
        let conv_state = state.clone();
        tokio::spawn(async move {
            let db = conv_state.mission.db();
            let provider_id = conv_state.embedding_service.as_ref()
                .map(|svc| svc.provider_id().to_string())
                .unwrap_or_else(|| missiond_core::embedding::FASTEMBED_PROVIDER_ID.to_string());

            // Load multi-topic vectors first, then backfill from old single-vec embeddings
            let mut topic_map: std::collections::HashMap<String, Vec<Vec<f32>>> =
                std::collections::HashMap::new();

            // Phase 1: Load from conversation_topic_vectors table
            match db.load_conversation_topic_vectors(&provider_id) {
                Ok(all) => {
                    for (sid, vecs) in all {
                        topic_map.insert(sid, vecs);
                    }
                    info!(count = topic_map.len(), "Topic vectors loaded");
                }
                Err(e) => warn!(error = %e, "Failed to load topic vectors"),
            }

            // Phase 2: Backfill from old single-embedding (wrap as 1-topic)
            match db.load_conversation_embeddings(&provider_id) {
                Ok(all) => {
                    let mut backfilled = 0;
                    for (sid, vec) in all {
                        if !topic_map.contains_key(&sid) {
                            topic_map.insert(sid, vec![vec]);
                            backfilled += 1;
                        }
                    }
                    if backfilled > 0 {
                        info!(backfilled, "Old single-vec embeddings loaded as fallback");
                    }
                }
                Err(e) => warn!(error = %e, "Failed to load old conversation embeddings"),
            }

            // Populate cache
            {
                let mut guard = conv_state.conversation_topic_cache.write().await;
                *guard = topic_map.into_iter().collect();
                info!(count = guard.len(), "Conversation topic cache warmed");
            }

            // Skill topic embedding cache
            match db.skill_load_topic_embeddings() {
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
        workers::embedding_worker::EmbeddingLoopWorker { rx: embedding_rx },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // Gemini request log subscriber (Timeline → DB persistence)
    workers::spawn_worker(
        workers::gemini_logger::GeminiLoggerWorker {
            timeline_rx: timeline_broadcast_tx.subscribe(),
        },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    workers::spawn_worker(
        vision_worker::VisionWorker,
        Arc::new(state.clone()), shutdown_rx.clone(),
    );
    workers::spawn_worker(
        step_narrator::StepNarratorWorker,
        Arc::new(state.clone()), shutdown_rx.clone(),
    );
    if state.minimax.is_some() {
        workers::spawn_worker(
            briefing_worker::BriefingWorker,
            Arc::new(state.clone()), shutdown_rx.clone(),
        );
        workers::spawn_worker(
            translation_worker::TranslationWorker {
                timeline_rx: timeline_broadcast_tx.subscribe(),
            },
            Arc::new(state.clone()), shutdown_rx.clone(),
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
                let db = health_state.mission.db();
                match db.ast_stats() {
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
                            let _ = health_state.embedding_tx.try_send(EmbeddingTask::BackfillAll);
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
    event_router::start_event_consumers(&state, &timeline_broadcast_tx);

    // --- Phase 6: Timeline Writer Task ---
    // MPSC → SQLite batch INSERT (get seq) → broadcast<TimelineEvent> + broadcast<String> (WS)
    {
        let rx = timeline_mpsc_rx;
        let timeline_tx = timeline_broadcast_tx.clone();
        let ws_tx = frontend_events_tx.clone();
        let db = state.mission.db_arc();
        tokio::spawn(async move {
            run_timeline_writer(rx, db, timeline_tx, ws_tx).await;
        });
    }

    // --- Git Commit Watcher ---
    // Polls monitored repos (from slot cwds) for new commits → timeline events.
    {
        let slot_cwds: Vec<String> = state.mission.list_slots()
            .into_iter()
            .filter_map(|s| s.config.cwd)
            .collect();
        let repos = git_watcher::collect_repo_roots(&slot_cwds);
        if !repos.is_empty() {
            let event_tx = state.event_bus.sender();
            let ast_tx = state.ast_sync_tx.clone();
            tokio::spawn(async move {
                git_watcher::run_git_watcher(repos, event_tx, ast_tx).await;
            });
        }
    }

    // --- AST Sync Worker (P2 HCE) ---
    // Worker loop + startup full sync for all repos
    {
        let mc = Arc::clone(&state.mission);
        let etx = state.embedding_tx.clone();
        tokio::spawn(async move {
            ast_sync_worker::run_ast_sync_worker(ast_sync_rx, mc, etx).await;
        });

        // Full sync at startup: trigger for all repos after delay
        let ast_tx2 = state.ast_sync_tx.clone();
        let slot_cwds2: Vec<String> = state.mission.list_slots()
            .into_iter()
            .filter_map(|s| s.config.cwd)
            .collect();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let repos = git_watcher::collect_repo_roots(&slot_cwds2);
            for repo in repos {
                let name = repo.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if let Err(e) = ast_tx2.send(ast_sync_worker::AstSyncTask::FullSync {
                    repo_path: repo,
                    repo_name: name,
                }).await {
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
                let publish_count = s.event_bus.publish_count.load(
                    std::sync::atomic::Ordering::Relaxed,
                );
                let memory_paused = s.memory_paused.load(
                    std::sync::atomic::Ordering::Relaxed,
                );
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
    info!("Timeline Writer + Health snapshot started (ws://*:{}/events)", ws_port);

    // Timeline TTL cleanup on startup
    {
        let db = state.mission.db_arc();
        tokio::task::spawn_blocking(move || {
            match db.cleanup_timeline_ttl(7) {
                Ok(deleted) if deleted > 0 => info!(deleted, "Timeline: cleaned up old entries (>7 days)"),
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
        workers::conversation_logger::ConversationLoggerWorker {
            conv_logger_rx,
        },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // PTY manager event stream (state changes, confirm, exit, MCP errors)
    workers::spawn_worker(
        workers::pty_event_worker::PtyEventWorker { pty_rx: pty_logger_rx },
        Arc::new(state.clone()),
        shutdown_rx.clone(),
    );

    // Retrospective worker — automatic session performance analysis
    workers::spawn_worker(
        workers::retro_worker::RetroWorker,
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
                                let mcp_config = slot.config.mcp_config.clone().map(std::path::PathBuf::from);
                                let (extra_env, session_file) = slot_env::build_slot_tracking_env(slot_id, slot.config.env.as_ref()).await;
                                match state.pty.spawn(&pty_slot, missiond_core::PTYSpawnOptions {
                                    auto_restart: true,
                                    wait_for_idle: false,
                                    timeout_secs: None,
                                    mcp_config,
                                    dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
                                    model: slot.config.model.clone(),
                                    extra_env,
                                }).await {
                                    Ok(_) => {
                                        slot_env::capture_slot_session_uuid(&state, slot_id, &session_file).await;
                                        info!(slot_id = %slot_id, "Auto-started new slot PTY");
                                    }
                                    Err(e) => warn!(slot_id = %slot_id, error = %e, "Failed to auto-start new slot PTY"),
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
            let mut sighup = signal(SignalKind::hangup()).expect("Failed to register SIGHUP handler");
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
            let monitored_files: std::collections::HashSet<&str> = [
                "slots.yaml", "servers.yaml", "xjp-mcp-config.json",
            ].into_iter().collect();

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
    tokio::signal::ctrl_c().await.ok();
    info!("Received shutdown signal, notifying workers");
    let _ = shutdown_tx.send(true);
    info!("Exiting");
    Ok(())
}
