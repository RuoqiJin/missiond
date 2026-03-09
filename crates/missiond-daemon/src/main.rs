//! missiond - singleton daemon for missiond
//!
//! Responsibilities:
//! - Own the global state (DB, slot/process/task/inbox, PTY sessions, CC tasks watcher)
//! - Provide a stable WebSocket endpoint for attach + tasks events
//! - Expose an IPC JSON-RPC endpoint for MCP proxy processes

mod events_sync;
mod lenient;
mod state;
mod mcp_client;
mod helpers;
mod handlers;
mod aiops;
mod decision_engine;
mod decision_harvest;
mod autopilot;
mod flow_engine;
mod llm_gateway;
mod event_bus;
mod event_router;
mod extraction;
mod supervisor;
mod memory_scheduler;
mod embedding_worker;
mod vision_worker;
mod slot_env;
mod claude_md_sync;
mod context_budget;
mod codex_cli;
mod gemini_client;
mod gemini_cli;
mod message_handler;
mod ipc_handler;
mod daemon_stats;
mod prompts;
mod session_util;
mod timeline_analyst;
mod git_watcher;
mod ast_sync_worker;
mod minimax_client;
mod briefing_worker;
mod translation_worker;
mod step_narrator;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use missiond_core::{
    MissionControl, MissionControlOptions, PermissionPolicy,
    PTYManager, PTYWebSocketServer, WSServerOptions, SkillIndex, InfraConfig,
};
use missiond_core::SessionState;
use missiond_core::{CCTasksWatcher, CCTasksWatcherOptions, WatcherEvent};
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
use embedding_worker::{init_embedding_provider, generate_and_store_conv_embedding};
use autopilot::autopilot_tick;
use session_util::detect_compaction;
use state::{MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use supervisor::get_task_jsonl_path;
use aiops::{process_incident, health_scan};
use message_handler::{handle_new_messages, handle_pty_text_complete};

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

    /// Execute a skill workflow: load workflow block, run MCP tools sequentially
    pub(crate) fn execute_workflow<'a>(
        &'a self,
        skill_name: &'a str,
        action_id: &'a str,
        dry_run: bool,
        param_overrides: Option<Value>,
        depth: u32,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<missiond_core::WorkflowResult>> + Send + 'a>> {
        const MAX_DEPTH: u32 = 3;
        const STEP_TIMEOUT_SECS: u64 = 30;

        Box::pin(async move {
        use missiond_core::{WorkflowStepPreview, WorkflowStepResult, WorkflowResult, parse_workflow_blocks, resolve_vars};

        // Guard: prevent recursive workflow bombs
        if depth > MAX_DEPTH {
            return Err(anyhow!("Workflow recursion depth exceeded (max {}). Skill '{}' action '{}'", MAX_DEPTH, skill_name, action_id));
        }
        let db = self.mission.db();

        // Guard: prevent concurrent execution of same action
        if !dry_run {
            if let Ok(true) = db.skill_execution_is_running(skill_name, action_id) {
                return Err(anyhow!("Action '{}' on skill '{}' is already running", action_id, skill_name));
            }
        }

        // Step 1: Load skill content from file
        let topic = db.skill_topic_get(skill_name)
            .map_err(|e| anyhow!("DB: {}", e))?
            .ok_or_else(|| anyhow!("Skill '{}' not found", skill_name))?;

        let content = std::fs::read_to_string(&topic.file_path)
            .map_err(|e| anyhow!("Failed to read skill file {}: {}", topic.file_path, e))?;

        // Step 2: Parse workflow blocks from skill content
        let workflows = parse_workflow_blocks(&content);
        let workflow = workflows.iter()
            .find(|w| w.id == action_id)
            .ok_or_else(|| anyhow!("Workflow '{}' not found in skill '{}'", action_id, skill_name))?;

        // Step 3: Check requires_approval from frontmatter actions
        let actions_json = topic.actions_json.as_deref().unwrap_or("[]");
        let actions: Vec<missiond_core::SkillAction> = serde_json::from_str(actions_json).unwrap_or_default();
        let action_meta = actions.iter().find(|a| a.id == action_id);
        let requires_approval = action_meta.map(|a| a.requires_approval).unwrap_or(false);

        if requires_approval && !dry_run {
            return Ok(WorkflowResult::PendingApproval {
                action_id: action_id.to_string(),
                skill: skill_name.to_string(),
            });
        }

        // Step 4: Dry-run → return preview only
        if dry_run {
            let steps: Vec<WorkflowStepPreview> = workflow.steps.iter().map(|s| {
                WorkflowStepPreview {
                    name: s.name.clone(),
                    tool: s.tool.clone(),
                    params: s.params.clone(),
                }
            }).collect();
            return Ok(WorkflowResult::Preview { steps });
        }

        // Step 5: Create execution log
        let exec_id = uuid::Uuid::new_v4().to_string();
        let _ = db.skill_execution_insert(
            &exec_id, skill_name, action_id,
            workflow.steps.len() as i32, "manual",
        );
        let exec_start = std::time::Instant::now();

        // Step 5b: Execute context hooks (pre-flight probes, best-effort)
        let mut context: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        if let Some(ref hooks_json) = topic.context_hooks_json {
            if let Ok(hooks) = serde_json::from_str::<Vec<missiond_core::ContextHook>>(hooks_json) {
                for hook in &hooks {
                    let hook_result = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        self.call_tool(&hook.tool, hook.params.clone()),
                    ).await;
                    match hook_result {
                        Ok(result) => {
                            let output = result.content.first()
                                .map(|c| match c { missiond_mcp::ToolContent::Text { text } => text.clone() })
                                .unwrap_or_default();
                            // Escape ${...} in hook output to prevent injection into resolve_vars()
                            let safe_output = output.replace("${", "$\\{");
                            context.insert(hook.save_as.clone(), safe_output);
                            debug!(hook = %hook.tool, save_as = %hook.save_as, "Context hook completed");
                        }
                        Err(_) => {
                            warn!(hook = %hook.tool, "Context hook timed out (10s), skipping");
                        }
                    }
                }
            }
        }

        // Step 6: Sequential execution

        // Apply param_overrides to context
        if let Some(overrides) = param_overrides {
            if let Value::Object(map) = overrides {
                for (k, v) in map {
                    context.insert(k, v.as_str().unwrap_or(&v.to_string()).to_string());
                }
            }
        }

        let mut results: Vec<WorkflowStepResult> = Vec::new();
        let mut i = 0usize;
        let mut visit_counts: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
        const MAX_STEP_VISITS: u32 = 5; // absolute ceiling per step

        while i < workflow.steps.len() {
            let step = &workflow.steps[i];

            // Guard: prevent infinite fallback loops
            let visits = visit_counts.entry(i).or_insert(0);
            *visits += 1;
            if *visits > MAX_STEP_VISITS {
                let duration_ms = exec_start.elapsed().as_millis() as i64;
                let err_msg = format!("Step {} ('{}') visited {} times — infinite loop detected", i, step.tool, visits);
                warn!(%err_msg);
                let _ = db.skill_execution_update_with_duration(
                    &exec_id, "failed", (i + 1) as i32,
                    Some(&serde_json::to_string(&context).unwrap_or_default()),
                    Some(&err_msg),
                    Some(duration_ms),
                );
                return Ok(WorkflowResult::Failed {
                    steps_completed: i,
                    error_step: i,
                    error: err_msg,
                    results,
                });
            }

            info!(exec_id = %exec_id, step = i, tool = %step.tool, "Executing workflow step");

            // Resolve ${var} references in params
            let resolved_params = resolve_vars(&step.params, &context);

            // Call the MCP tool with timeout
            let tool_result = match tokio::time::timeout(
                std::time::Duration::from_secs(STEP_TIMEOUT_SECS),
                self.call_tool(&step.tool, resolved_params),
            ).await {
                Ok(result) => result,
                Err(_) => {
                    let mut res = ToolResult::text(format!("Step timed out after {}s: {}", STEP_TIMEOUT_SECS, step.tool));
                    res.is_error = Some(true);
                    res
                }
            };
            let is_error = tool_result.is_error.unwrap_or(false);
            let output = tool_result.content.first()
                .map(|c| match c { missiond_mcp::ToolContent::Text { text } => text.clone() })
                .unwrap_or_default();

            // Save result to context if save_as is specified
            if let Some(ref key) = step.save_as {
                context.insert(key.clone(), output.clone());
            }

            results.push(WorkflowStepResult {
                name: step.name.clone(),
                tool: step.tool.clone(),
                success: !is_error,
                output: output.clone(),
            });

            // Update progress
            let _ = db.skill_execution_update(
                &exec_id, "running", (i + 1) as i32, None, None,
            );

            // Error handling
            if is_error {
                let on_error = step.on_error.as_str();
                match on_error {
                    "skip" => {
                        warn!(step = i, tool = %step.tool, "Step failed, skipping");
                    }
                    "retry" => {
                        let max = step.max_retries.max(1);
                        let mut succeeded = false;
                        for attempt in 1..=max {
                            let backoff_secs = 1u64 << (attempt - 1).min(4);
                            warn!(step = i, tool = %step.tool, attempt, max, backoff_secs, "Retrying step");
                            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                            let retry_params = resolve_vars(&step.params, &context);
                            let retry_result = match tokio::time::timeout(
                                std::time::Duration::from_secs(STEP_TIMEOUT_SECS),
                                self.call_tool(&step.tool, retry_params),
                            ).await {
                                Ok(r) => r,
                                Err(_) => {
                                    let mut r = ToolResult::text("Retry timed out".to_string());
                                    r.is_error = Some(true);
                                    r
                                }
                            };
                            if !retry_result.is_error.unwrap_or(false) {
                                let retry_output = retry_result.content.first()
                                    .map(|c| match c { missiond_mcp::ToolContent::Text { text } => text.clone() })
                                    .unwrap_or_default();
                                if let Some(ref key) = step.save_as {
                                    context.insert(key.clone(), retry_output.clone());
                                }
                                if let Some(last) = results.last_mut() {
                                    last.success = true;
                                    last.output = retry_output;
                                }
                                succeeded = true;
                                break;
                            }
                        }
                        if !succeeded {
                            let duration_ms = exec_start.elapsed().as_millis() as i64;
                            let _ = db.skill_execution_update_with_duration(
                                &exec_id, "failed", (i + 1) as i32,
                                Some(&serde_json::to_string(&context).unwrap_or_default()),
                                Some(&format!("Failed after {} retries: {}", max, output)),
                                Some(duration_ms),
                            );
                            return Ok(WorkflowResult::Failed {
                                steps_completed: i + 1,
                                error_step: i,
                                error: format!("Failed after {} retries: {}", max, output),
                                results,
                            });
                        }
                    }
                    s if s.starts_with("fallback:") => {
                        let target_id = &s["fallback:".len()..];
                        if let Some(target_idx) = workflow.steps.iter().position(|st| st.id.as_deref() == Some(target_id)) {
                            warn!(step = i, tool = %step.tool, target = target_id, target_idx, "Falling back");
                            i = target_idx;
                            continue; // Jump without incrementing
                        } else {
                            let duration_ms = exec_start.elapsed().as_millis() as i64;
                            let err_msg = format!("Fallback target '{}' not found", target_id);
                            let _ = db.skill_execution_update_with_duration(
                                &exec_id, "failed", (i + 1) as i32,
                                Some(&serde_json::to_string(&context).unwrap_or_default()),
                                Some(&err_msg),
                                Some(duration_ms),
                            );
                            return Ok(WorkflowResult::Failed {
                                steps_completed: i + 1,
                                error_step: i,
                                error: err_msg,
                                results,
                            });
                        }
                    }
                    _ => {
                        // "stop" (default)
                        let duration_ms = exec_start.elapsed().as_millis() as i64;
                        let _ = db.skill_execution_update_with_duration(
                            &exec_id, "failed", (i + 1) as i32,
                            Some(&serde_json::to_string(&context).unwrap_or_default()),
                            Some(&output),
                            Some(duration_ms),
                        );
                        return Ok(WorkflowResult::Failed {
                            steps_completed: i + 1,
                            error_step: i,
                            error: output,
                            results,
                        });
                    }
                }
            }
            i += 1;
        }

        // Success
        let duration_ms = exec_start.elapsed().as_millis() as i64;
        let _ = db.skill_execution_update_with_duration(
            &exec_id, "success", workflow.steps.len() as i32,
            Some(&serde_json::to_string(&context).unwrap_or_default()),
            None,
            Some(duration_ms),
        );

        Ok(WorkflowResult::Success {
            steps_completed: workflow.steps.len(),
            results,
        })
        }) // Box::pin(async move)
    }

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
    let permission = Arc::new(PermissionPolicy::new(&permission_config_path));

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
        };
        pty.init_slot(&pty_slot).await;
    }

    // CC tasks watcher
    let mut cc = CCTasksWatcher::new(CCTasksWatcherOptions::default());
    cc.start().await?;
    let cc_tasks = Arc::new(Mutex::new(cc));

    // Conversation logger: subscribe to watcher events (processed in main select loop)
    let mut conv_logger_rx = cc_tasks.lock().await.subscribe();
    // PTY conversation logger: subscribe to manager events
    let mut pty_logger_rx = pty.subscribe();

    // AIOps: incident event bus (capacity 100, try_send only — "宁丢不阻塞")
    let (incident_tx, incident_rx) = tokio::sync::mpsc::channel::<missiond_core::types::MissionIncident>(100);

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
    let mut ws_server = PTYWebSocketServer::new(WSServerOptions {
        port: ws_port,
        pty_manager: Some(Arc::clone(&pty)),
        cc_tasks_watcher: Some(Arc::clone(&cc_tasks)),
        screenshot_broker: Some(Arc::clone(&screenshot_broker)),
        incident_tx: Some(incident_tx.clone()),
        frontend_events_tx: Some(frontend_events_tx.clone()),
        db: Some(mission.db_arc()),
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
        slot_fail_counts: Arc::new(std::sync::Mutex::new(HashMap::new())),
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
    };

    // Auto-spawn slots with auto_start: true
    {
        let slots = state.mission.list_slots();
        for slot in &slots {
            if slot.config.auto_start == Some(true) {
                info!(slot_id = %slot.config.id, "Auto-starting slot on daemon boot");
                match state.mission.spawn_agent(
                    &slot.config.id,
                    Some(missiond_core::SpawnOptions {
                        visible: false,
                        auto_restart: true,
                    }),
                ).await {
                    Ok(_) => info!(slot_id = %slot.config.id, "Auto-started slot"),
                    Err(e) => warn!(slot_id = %slot.config.id, error = %e, "Failed to auto-start slot"),
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

    // Embedding Worker: event-driven actor (sleeps until triggered, 0 CPU idle)
    {
        let worker_state = state.clone();
        let mut rx = embedding_rx;
        tokio::spawn(async move {
            info!("Embedding worker started (event-driven)");
            while let Some(task) = rx.recv().await {
                let db = worker_state.mission.db();
                let provider_id = worker_state.embedding_service.as_ref()
                    .map(|svc| svc.provider_id().to_string())
                    .unwrap_or_else(|| missiond_core::embedding::FASTEMBED_PROVIDER_ID.to_string());

                match task {
                    EmbeddingTask::ProcessSession(session_id) => {
                        if tokio::time::timeout(
                            std::time::Duration::from_secs(60),
                            generate_and_store_conv_embedding(&worker_state, &session_id),
                        ).await.is_err() {
                            warn!(session = %session_id, "Embedding generation timed out (60s)");
                        }
                    }
                    EmbeddingTask::ProcessKBEntry(id) => {
                        if let Some(ref emb_svc) = worker_state.embedding_service {
                            if let Ok(Some(entry)) = db.kb_get_by_id(&id) {
                                let detail_text = entry.detail.as_ref()
                                    .map(|d| serde_json::to_string(d).unwrap_or_default())
                                    .unwrap_or_default();
                                let embed_text = format!("知识条目：{}\n详情：{}", entry.summary, detail_text);
                                let svc = Arc::clone(emb_svc);
                                if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                    std::time::Duration::from_secs(30),
                                    tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                                ).await {
                                    let _ = db.kb_set_embedding(&id, &vec, &provider_id);
                                    // Update policy:decision cache (Decision Engine T1)
                                    if entry.category.starts_with("policy:decision") {
                                        let mut guard = worker_state.embedding_cache.write().await;
                                        guard.retain(|(eid, _)| eid != &id);
                                        guard.push((id.clone(), vec.clone()));
                                    }
                                    // Update full KB search cache
                                    {
                                        let mut guard = worker_state.kb_search_cache.write().await;
                                        guard.retain(|(eid, _)| eid != &id);
                                        guard.push((id.clone(), vec));
                                    }
                                    debug!(kb_id = %id, "KB entry embedding updated");
                                }
                            }
                        }
                    }
                    EmbeddingTask::ProcessSkillTopic(topic) => {
                        if let Some(ref emb_svc) = worker_state.embedding_service {
                            // Build embed text: topic + description + all active blocks
                            if let Ok(missing) = db.skill_topics_missing_embedding(1) {
                                // If not in missing list, build text from DB directly
                                let embed_text = if let Some((_, text)) = missing.iter().find(|(t, _)| t == &topic) {
                                    text.clone()
                                } else {
                                    // Topic already has embedding but was re-upserted — rebuild text
                                    format!("技能主题：{}", topic) // minimal fallback
                                };
                                let svc = Arc::clone(emb_svc);
                                if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                    std::time::Duration::from_secs(30),
                                    tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                                ).await {
                                    let _ = db.skill_set_topic_embedding(&topic, &vec, &provider_id);
                                    let mut cache = worker_state.skill_embedding_cache.write().await;
                                    cache.retain(|(t, _)| t != &topic);
                                    cache.push((topic.clone(), vec));
                                    debug!(topic = %topic, "Skill topic embedding updated");
                                }
                            }
                        }
                    }
                    EmbeddingTask::BackfillAll => {
                        info!("Full embedding backfill triggered");

                        if let Some(ref emb_svc) = worker_state.embedding_service {
                            // ── Phase 1: KB stale re-embed ──
                            loop {
                                let stale = db.kb_entries_stale_embedding(&provider_id, 20).unwrap_or_default();
                                if stale.is_empty() { break; }
                                info!(count = stale.len(), "KB stale re-embedding");
                                for (id, summary, detail) in &stale {
                                    let embed_text = format!("知识条目：{}\n详情：{}", summary, detail);
                                    let svc = Arc::clone(emb_svc);
                                    if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                        std::time::Duration::from_secs(30),
                                        tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                                    ).await {
                                        let _ = db.kb_set_embedding(id, &vec, &provider_id);
                                    }
                                }
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            }

                            // ── Phase 2: KB missing embed ──
                            loop {
                                let missing = db.kb_entries_missing_embedding(None).unwrap_or_default();
                                if missing.is_empty() { break; }
                                info!(count = missing.len(), "KB missing embedding backfill");
                                for (id, summary, detail) in &missing {
                                    let embed_text = format!("知识条目：{}\n详情：{}", summary, detail);
                                    let svc = Arc::clone(emb_svc);
                                    if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                        std::time::Duration::from_secs(30),
                                        tokio::task::spawn_blocking(move || svc.embed(&embed_text)),
                                    ).await {
                                        let _ = db.kb_set_embedding(id, &vec, &provider_id);
                                    }
                                }
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            }

                            // Warm KB caches after backfill
                            if let Ok(all) = db.kb_load_embeddings("policy:decision") {
                                let mut guard = worker_state.embedding_cache.write().await;
                                *guard = all;
                                info!(count = guard.len(), "KB policy cache refreshed after backfill");
                            }
                            if let Ok(all) = db.kb_load_all_embeddings() {
                                let mut guard = worker_state.kb_search_cache.write().await;
                                *guard = all;
                                info!(count = guard.len(), "KB search cache refreshed after backfill");
                            }

                            // ── Phase 3: Skill stale + missing ──
                            loop {
                                let stale = db.skill_topics_stale_embedding(&provider_id, 20).unwrap_or_default();
                                if stale.is_empty() { break; }
                                info!(count = stale.len(), "Skill stale re-embedding");
                                for (topic, embed_text) in &stale {
                                    let svc = Arc::clone(emb_svc);
                                    let text = embed_text.clone();
                                    if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                        std::time::Duration::from_secs(30),
                                        tokio::task::spawn_blocking(move || svc.embed(&text)),
                                    ).await {
                                        let _ = db.skill_set_topic_embedding(topic, &vec, &provider_id);
                                    }
                                }
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            }
                            loop {
                                let missing = db.skill_topics_missing_embedding(20).unwrap_or_default();
                                if missing.is_empty() { break; }
                                info!(count = missing.len(), "Skill missing embedding backfill");
                                for (topic, embed_text) in &missing {
                                    let svc = Arc::clone(emb_svc);
                                    let text = embed_text.clone();
                                    if let Ok(Ok(Some(vec))) = tokio::time::timeout(
                                        std::time::Duration::from_secs(30),
                                        tokio::task::spawn_blocking(move || svc.embed(&text)),
                                    ).await {
                                        let _ = db.skill_set_topic_embedding(topic, &vec, &provider_id);
                                    }
                                }
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            }

                            // Warm Skill cache after backfill
                            if let Ok(all) = db.skill_load_topic_embeddings() {
                                let mut guard = worker_state.skill_embedding_cache.write().await;
                                *guard = all;
                                info!(count = guard.len(), "Skill embedding cache refreshed after backfill");
                            }

                            // ── Phase 4: Conversation topic vector backfill ──
                            // Re-process sessions that have summaries but no topic vectors yet.
                            loop {
                                let needing = db.conversations_needing_topic_vectors(&provider_id, 20).unwrap_or_default();
                                if needing.is_empty() { break; }
                                info!(count = needing.len(), "Conv topic vector backfill");
                                for session_id in &needing {
                                    if tokio::time::timeout(
                                        std::time::Duration::from_secs(90),
                                        generate_and_store_conv_embedding(&worker_state, session_id),
                                    ).await.is_err() {
                                        warn!(session = %session_id, "Topic vector backfill timed out");
                                    }
                                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                }
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            }
                        }

                        // ── Phase 4.5: Build session timelines for compaction fragments ──
                        // (no embedding needed, runs unconditionally)
                        {
                            let needing = db.conversations_needing_timeline(50).unwrap_or_default();
                            if !needing.is_empty() {
                                info!(count = needing.len(), "Building session timelines for compaction parents");
                            }
                            for parent_id in &needing {
                                let fragments = match db.get_compaction_fragments(parent_id) {
                                    Ok(f) => f,
                                    Err(e) => {
                                        warn!(parent = %parent_id, error = %e, "Failed to get compaction fragments");
                                        continue;
                                    }
                                };
                                if fragments.is_empty() { continue; }

                                let mut timeline_entries = Vec::new();
                                for (idx, (frag_id, started_at, msg_count)) in fragments.iter().enumerate() {
                                    let summary = db.get_last_assistant_content(frag_id)
                                        .unwrap_or(None);
                                    let summary_tokens = summary.as_ref()
                                        .map(|s| s.len() / 4) // rough token estimate
                                        .unwrap_or(0);
                                    timeline_entries.push(serde_json::json!({
                                        "fragment_id": frag_id,
                                        "shard_index": idx,
                                        "started_at": started_at,
                                        "message_count": msg_count,
                                        "summary_tokens": summary_tokens,
                                        "summary": summary,
                                        "segment_embedding_id": null,
                                    }));
                                }

                                let timeline_json = serde_json::to_string(&timeline_entries)
                                    .unwrap_or_else(|_| "[]".to_string());

                                match db.set_session_timeline(parent_id, &timeline_json) {
                                    Ok(true) => {
                                        // Clear old summary so Phase 5 regenerates with timeline context
                                        let _ = db.clear_conversation_summary(parent_id);
                                        info!(
                                            parent = %parent_id,
                                            fragments = fragments.len(),
                                            "Session timeline built, summary cleared for regeneration"
                                        );
                                    }
                                    Ok(false) => {
                                        // CAS failed — another thread already built it
                                    }
                                    Err(e) => {
                                        warn!(parent = %parent_id, error = %e, "Failed to set session timeline");
                                    }
                                }
                            }
                        }

                        // ── Phase 5: Conversation missing summary+embed ──
                        loop {
                            let missing = db.conversations_missing_summary(20).unwrap_or_default();
                            if missing.is_empty() { break; }
                            info!(count = missing.len(), "Conv backfill batch");
                            for (idx, session_id) in missing.iter().enumerate() {
                                info!(session = %session_id, idx = idx + 1, total = missing.len(), "Processing session");
                                match tokio::time::timeout(
                                    std::time::Duration::from_secs(60),
                                    generate_and_store_conv_embedding(&worker_state, session_id),
                                ).await {
                                    Ok(()) => {
                                        info!(session = %session_id, idx = idx + 1, "Session done");
                                    }
                                    Err(_) => {
                                        warn!(session = %session_id, idx = idx + 1, "Conv embedding timed out (60s), skipping");
                                        let _ = db.set_conversation_summary(session_id, "[timeout]");
                                    }
                                }
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }

                        info!("Full embedding backfill complete");
                    }
                }
            }
            warn!("Embedding worker channel closed");
        });
    }

    // --- Gemini request log subscriber (Timeline → DB persistence) ---
    // Two-step: insert on started (with prompt_text), update on completed (with response_text).
    // Full content lives in gemini_requests table; timeline only stores request_id references.
    {
        let mut rx = timeline_broadcast_tx.subscribe();
        let log_db = Arc::clone(&state.mission);
        tokio::spawn(async move {
            // Startup cleanup: remove logs older than 7 days
            if let Ok(deleted) = log_db.db().gemini_log_cleanup(7) {
                if deleted > 0 {
                    info!(deleted, "Gemini log: cleaned up old entries");
                }
            }
            loop {
                match rx.recv().await {
                    Ok(te) => match &te.event {
                        event_bus::DaemonEvent::GeminiRequestStarted {
                            request_id, caller, session_id, model,
                            prompt_chars, prompt_text,
                        } => {
                            if let Err(e) = log_db.db().gemini_log_insert_started(
                                request_id, caller, session_id.as_deref(),
                                model, *prompt_chars as i64,
                                prompt_text.as_deref(),
                            ) {
                                warn!(error = %e, "Gemini log: failed to insert started");
                            }
                        }
                        event_bus::DaemonEvent::GeminiRequestCompleted {
                            request_id, api_mode,
                            response_chars, queue_wait_ms, duration_ms,
                            retry_count, status, error_msg, response_text, ..
                        } => {
                            if let Err(e) = log_db.db().gemini_log_update_completed(
                                request_id, api_mode,
                                *response_chars as i64, *queue_wait_ms as i64,
                                *duration_ms as i64, *retry_count as i64,
                                status, error_msg.as_deref(),
                                response_text.as_deref(),
                            ) {
                                warn!(error = %e, "Gemini log: failed to update completed");
                            }
                        }
                        // Codex CLI events — reuse gemini_requests table
                        event_bus::DaemonEvent::CodexRequestStarted {
                            request_id, caller, model, prompt_chars, prompt_text, ..
                        } => {
                            if let Err(e) = log_db.db().gemini_log_insert_started(
                                request_id, caller, None,
                                model, *prompt_chars as i64,
                                prompt_text.as_deref(),
                            ) {
                                warn!(error = %e, "Codex log: failed to insert started");
                            }
                        }
                        event_bus::DaemonEvent::CodexRequestCompleted {
                            request_id, response_chars, duration_ms,
                            status, error_msg, response_text, ..
                        } => {
                            if let Err(e) = log_db.db().gemini_log_update_completed(
                                request_id, "codex-cli",
                                *response_chars as i64, 0,
                                *duration_ms as i64, 0,
                                status, error_msg.as_deref(),
                                response_text.as_deref(),
                            ) {
                                warn!(error = %e, "Codex log: failed to update completed");
                            }
                        }
                        _ => {}
                    },
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "Gemini log subscriber lagged, some requests not logged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Gemini log subscriber: event bus closed");
                        break;
                    }
                }
            }
        });
    }

    // --- Vision Worker: async image understanding pipeline ---
    vision_worker::spawn_vision_worker(Arc::new(state.clone()));

    // --- Briefing Worker: async semantic summarization via MiniMax M2.5 ---
    briefing_worker::spawn_briefing_worker(Arc::new(state.clone()));

    // --- Step Narrator: async GPT-5.4 conversation step explanation ---
    step_narrator::spawn_step_narrator(Arc::new(state.clone()));

    // --- Translation Worker: async thinking→Chinese translation via MiniMax ---
    translation_worker::spawn_translation_worker(
        Arc::new(state.clone()),
        timeline_broadcast_tx.subscribe(),
    );

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
                interval.tick().await;
                if let Err(e) = autopilot_tick(&auto_state).await {
                    warn!(error = %e, "Autopilot tick failed");
                }
            }
        });
    }
    info!("Autopilot scheduler started (60s interval, isolated task)");

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
            tokio::spawn(async move {
                git_watcher::run_git_watcher(repos, event_tx).await;
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
    {
        let s = state.clone();
        tokio::spawn(async move {
            loop {
                match conv_logger_rx.recv().await {
                    Ok(WatcherEvent::NewMessages { session_id, project_path, jsonl_path, messages }) => {
                        let mut is_pty = s.pty_session_uuids.read().await.contains(&session_id);

                        // Compaction detection
                        let mut compaction_task_id: Option<String> = None;
                        if !is_pty {
                            if let Some((slot_id, old_uuid, old_task_id)) = detect_compaction(&s, &session_id, &project_path) {
                                info!(
                                    slot_id = %slot_id,
                                    old_session = %old_uuid,
                                    new_session = %session_id,
                                    "Compaction detected: session replaced by context compaction"
                                );
                                let db = s.mission.db();
                                let _ = db.mark_conversation_compacted(&old_uuid);
                                let _ = db.set_slot_session(&slot_id, &session_id);
                                s.pty_session_uuids.write().await.remove(&old_uuid);
                                s.pty_session_uuids.write().await.insert(session_id.clone());
                                compaction_task_id = old_task_id;
                                is_pty = true;
                            }
                        }

                        // Progress tracking
                        if is_pty {
                            if let Ok(Some(slot_id)) = s.mission.db().get_slot_for_session(&session_id) {
                                let mut progress = s.slot_progress.write().await;
                                let sp = progress.entry(slot_id).or_default();
                                if sp.session_id != session_id {
                                    *sp = SlotProgress { session_id: session_id.clone(), ..Default::default() };
                                }
                                for msg in &messages {
                                    if let Some(blocks) = msg.message.content.as_array() {
                                        for block in blocks {
                                            match block.get("type").and_then(|t| t.as_str()) {
                                                Some("tool_use") => {
                                                    let name = block.get("name")
                                                        .and_then(|n| n.as_str())
                                                        .unwrap_or("unknown")
                                                        .to_string();
                                                    *sp.tool_counts.entry(name.clone()).or_insert(0) += 1;
                                                    sp.total_calls += 1;
                                                    sp.current_tool = Some(CurrentToolInfo {
                                                        name,
                                                        started_at: msg.timestamp.clone(),
                                                    });
                                                    sp.last_activity = Some(msg.timestamp.clone());
                                                }
                                                Some("tool_result") => {
                                                    sp.total_results += 1;
                                                    sp.current_tool = None;
                                                    if block.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false) {
                                                        sp.error_count += 1;
                                                    }
                                                    sp.last_activity = Some(msg.timestamp.clone());
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        let db_messages: Vec<_> = messages.into_iter()
                            .filter(|m| m.message_type != "tool_use")
                            .collect();
                        handle_new_messages(&s, session_id.clone(), project_path, jsonl_path, db_messages, is_pty);

                        if let Some(tid) = compaction_task_id {
                            let _ = s.mission.db().set_conversation_task_id(&session_id, &tid);
                        }
                    }
                    Ok(WatcherEvent::NewEvents { session_id, events }) => {
                        events_sync::handle_new_events(s.mission.db(), session_id, events);
                    }
                    Ok(WatcherEvent::SessionInactive(session)) => {
                        if let Ok(Some(conv)) = s.mission.db().get_conversation(&session.session_id) {
                            if conv.status == "compacted" {
                                debug!(session = %session.session_id, "Skipping inactive check for compacted session");
                                continue;
                            }
                        }
                        if let Err(e) = s.mission.db().complete_conversation(&session.session_id) {
                            warn!(session = %session.session_id, error = %e, "Failed to complete conversation");
                        } else {
                            info!(session = %session.session_id, "Conversation marked completed");
                            // Build session timeline if this parent has compaction fragments
                            {
                                let db = s.mission.db();
                                let sid = &session.session_id;
                                let frags = db.get_compaction_fragments(sid).unwrap_or_default();
                                if !frags.is_empty() {
                                    let mut entries = Vec::new();
                                    for (idx, (frag_id, started_at, msg_count)) in frags.iter().enumerate() {
                                        let summary = db.get_last_assistant_content(frag_id).unwrap_or(None);
                                        let summary_tokens = summary.as_ref().map(|s| s.len() / 4).unwrap_or(0);
                                        entries.push(serde_json::json!({
                                            "fragment_id": frag_id,
                                            "shard_index": idx,
                                            "started_at": started_at,
                                            "message_count": msg_count,
                                            "summary_tokens": summary_tokens,
                                            "summary": summary,
                                            "segment_embedding_id": null,
                                        }));
                                    }
                                    if let Ok(json) = serde_json::to_string(&entries) {
                                        match db.set_session_timeline(sid, &json) {
                                            Ok(true) => info!(session = %sid, fragments = frags.len(), "Session timeline built"),
                                            Ok(false) => debug!(session = %sid, "Session timeline already exists"),
                                            Err(e) => warn!(session = %sid, error = %e, "Failed to build session timeline"),
                                        }
                                    }
                                }
                            }
                            // Trigger embedding worker (event-driven, no spawn)
                            let _ = s.embedding_tx.try_send(EmbeddingTask::ProcessSession(session.session_id.clone()));
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "Conversation logger lagged — triggering reconciliation");
                        // Reconcile: re-scan active sessions' JSONL to recover lost messages
                        let reconcile_state = s.clone();
                        tokio::spawn(async move {
                            let db = reconcile_state.mission.db();
                            let convs = db.list_conversations(Some("active"), 100, Some("all"), None)
                                .unwrap_or_default();
                            let mut reconciled = 0usize;
                            for conv in &convs {
                                if let Some(ref path) = conv.jsonl_path {
                                    events_sync::reconcile_conversation_messages(db, &conv.id, path).await;
                                    reconciled += 1;
                                }
                            }
                            if reconciled > 0 {
                                info!(reconciled, "Lag reconciliation complete");
                            }
                        });
                    }
                    Err(_) => {}
                }
            }
        });
    }

    // PTY manager event stream
    {
        let s = state.clone();
        tokio::spawn(async move {
            loop {
                match pty_logger_rx.recv().await {
                    Ok(missiond_core::ManagerEvent::TextComplete { slot_id, turn_id, content, timestamp }) => {
                        if !content.is_empty() {
                            s.slot_last_responses.write().await.insert(slot_id.clone(), content.clone());
                        }
                        handle_pty_text_complete(&s, slot_id, turn_id, content, timestamp);
                    }
                    Ok(missiond_core::ManagerEvent::Exited { slot_id, exit_code }) => {
                        info!(slot_id = %slot_id, exit_code = exit_code, "PTY session exited");
                        let old_uuid = s.mission.db().get_slot_session(&slot_id).unwrap_or(None);
                        if let Some(ref uuid) = old_uuid {
                            let _ = s.mission.db().complete_conversation(uuid);
                            // Trigger embedding worker (event-driven)
                            let _ = s.embedding_tx.try_send(EmbeddingTask::ProcessSession(uuid.clone()));
                            s.pty_session_uuids.write().await.remove(uuid);
                        }
                        s.mission.db().clear_slot_session(&slot_id);
                    }
                    Ok(missiond_core::ManagerEvent::StateChange { ref slot_id, new_state, prev_state }) => {
                        // Publish slot state change with trace context (Phase 6c)
                        let trace_id = s.mission.db().get_slot_session(slot_id).ok().flatten();
                        s.event_bus.publish_traced(
                            event_bus::DaemonEvent::SlotStateChanged {
                                slot_id: slot_id.to_string(),
                                new_state: format!("{:?}", new_state),
                                prev_state: format!("{:?}", prev_state),
                            },
                            event_bus::TraceContext {
                                trace_id,
                                summary: Some(format!("{}: {:?} → {:?}", slot_id, prev_state, new_state)),
                                ..Default::default()
                            },
                        );
                        // Route memory slot state changes to the correct lane
                        let lane = if slot_id == MEMORY_SLOT_ID {
                            Some(("fast", &s.extraction_state, &s.memory_slot_busy_since))
                        } else if slot_id == MEMORY_SLOW_SLOT_ID {
                            Some(("slow", &s.slow_extraction_state, &s.slow_slot_busy_since))
                        } else {
                            None
                        };
                        if let Some((lane_name, es_lock, busy_since)) = lane {
                            if new_state == SessionState::Idle {
                                busy_since.store(0, std::sync::atomic::Ordering::SeqCst);
                                let mut es = es_lock.write().await;
                                if es.phase == ExtractionPhase::WaitingForSlotIdle || es.phase == ExtractionPhase::Sending {
                                    let phase_age = chrono::Utc::now().timestamp() - es.phase_started_at;
                                    if phase_age < 3 {
                                        debug!(lane = lane_name, phase_age, "Ignoring early Idle transition (likely spawn init)");
                                    } else {
                                        let is_realtime = matches!(es.active_type, Some("realtime"));
                                        info!(
                                            lane = lane_name,
                                            extraction_type = ?es.active_type,
                                            phase_age,
                                            "Extraction complete: slot returned to Idle"
                                        );
                                        if is_realtime {
                                            if !es.watermark_targets.is_empty() {
                                                let db = s.mission.db();
                                                for (session_id, timestamp) in &es.watermark_targets {
                                                    let _ = db.update_realtime_forwarded_at(session_id, timestamp);
                                                }
                                                info!(sessions = es.watermark_targets.len(), "Realtime: advanced watermarks");
                                                es.watermark_targets.clear();
                                            }
                                        }
                                        if matches!(es.active_type, Some("deep_analysis")) {
                                            if let Some(conv_id) = es.current_deep_conv_id.take() {
                                                if es.is_checkpoint {
                                                    if let Some(msg_id) = es.checkpoint_message_id.take() {
                                                        if let Err(e) = s.mission.db().update_deep_checkpoint(&conv_id, msg_id) {
                                                            warn!(conv_id = %conv_id, error = %e, "Failed to advance checkpoint watermark");
                                                        } else {
                                                            info!(conv_id = %conv_id, msg_id, "Deep analysis checkpoint: advanced watermark");
                                                        }
                                                    }
                                                } else {
                                                    if let Err(e) = s.mission.db().mark_analysis_complete(&conv_id, CURRENT_ANALYSIS_VERSION) {
                                                        warn!(conv_id = %conv_id, error = %e, "Failed to mark analysis complete");
                                                    } else {
                                                        info!(conv_id = %conv_id, version = CURRENT_ANALYSIS_VERSION, "Deep analysis: marked complete");
                                                    }
                                                }
                                            }
                                        }
                                        if let Some(ref st_id) = es.current_slot_task_id {
                                            let _ = s.mission.db().slot_task_set_completed(st_id, 0);
                                        }
                                        let mem_trace_id = es.current_deep_conv_id.clone()
                                            .or_else(|| es.current_task_id.clone());
                                        s.event_bus.publish_traced(
                                            event_bus::DaemonEvent::MemoryPhaseChanged {
                                                slot_id: slot_id.to_string(),
                                                phase: "Idle".to_string(),
                                                active_type: es.active_type.map(|s| s.to_string()),
                                            },
                                            event_bus::TraceContext {
                                                trace_id: mem_trace_id,
                                                summary: Some(format!("{}: {:?} → Idle", slot_id, es.active_type)),
                                                ..Default::default()
                                            },
                                        );
                                        es.phase = ExtractionPhase::Idle;
                                        es.active_type = None;
                                        es.current_task_id = None;
                                        es.current_slot_task_id = None;
                                        es.is_checkpoint = false;
                                        es.checkpoint_message_id = None;
                                        s.event_bus.publish(event_bus::DaemonEvent::SlotBecameIdle { slot_id: slot_id.to_string() });
                                    }
                                }
                            } else if prev_state == SessionState::Idle {
                                busy_since.store(
                                    chrono::Utc::now().timestamp(),
                                    std::sync::atomic::Ordering::SeqCst,
                                );
                            }
                        }

                        // Close Running submit tasks when slot returns to Idle
                        if new_state == SessionState::Idle && prev_state != SessionState::Idle {
                            if let Ok(running_tasks) = s.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Running) {
                                let now = chrono::Utc::now().timestamp_millis();
                                const MIN_EXECUTION_MS: i64 = 5_000;
                                const MIN_JSONL_EXECUTION_MS: i64 = 3_000;
                                let pty_resp = s.slot_last_responses.write().await.remove(slot_id.as_str());
                                let jsonl_resp = match s.mission.db().get_slot_session(slot_id.as_str()) {
                                    Ok(Some(session_uuid)) => {
                                        match s.mission.db().get_conversation(&session_uuid) {
                                            Ok(Some(conv)) => {
                                                if let Some(ref jsonl_path) = conv.jsonl_path {
                                                    missiond_core::extract_last_assistant_text(std::path::Path::new(jsonl_path)).await
                                                } else { None }
                                            }
                                            _ => None,
                                        }
                                    }
                                    _ => None,
                                };
                                let jsonl_confirmed = if let Some(jsonl_path) = get_task_jsonl_path(&s, &missiond_core::types::Task {
                                    id: String::new(), role: String::new(), prompt: String::new(),
                                    status: missiond_core::types::TaskStatus::Running,
                                    slot_id: Some(slot_id.to_string()), session_id: None,
                                    result: None, error: None, created_at: 0, started_at: None, finished_at: None,
                                }) {
                                    missiond_core::jsonl_has_completed_turn(std::path::Path::new(&jsonl_path)).await
                                } else { false };
                                for task in &running_tasks {
                                    if task.slot_id.as_deref() == Some(slot_id.as_str()) {
                                        let started = task.started_at.unwrap_or(task.created_at);
                                        let elapsed = now - started;
                                        if elapsed < MIN_JSONL_EXECUTION_MS {
                                            debug!(
                                                task_id = %task.id, slot_id = %slot_id, elapsed_ms = elapsed,
                                                "Submit task NOT closed: too short even for JSONL ({elapsed}ms < {MIN_JSONL_EXECUTION_MS}ms)"
                                            );
                                            continue;
                                        }
                                        if elapsed < MIN_EXECUTION_MS && !jsonl_confirmed {
                                            debug!(
                                                task_id = %task.id, slot_id = %slot_id, elapsed_ms = elapsed,
                                                "Submit task NOT closed: execution too short ({elapsed}ms < {MIN_EXECUTION_MS}ms) and no JSONL confirmation"
                                            );
                                            continue;
                                        }
                                        let result_text = jsonl_resp.clone()
                                            .or_else(|| {
                                                if pty_resp.is_some() {
                                                    warn!(task_id = %task.id, "JSONL result unavailable, falling back to PTY");
                                                }
                                                pty_resp.clone()
                                            })
                                            .unwrap_or_else(|| "completed".to_string());
                                        let result_text = if result_text.len() > 4096 {
                                            let mut end = 4096;
                                            while !result_text.is_char_boundary(end) && end > 0 { end -= 1; }
                                            format!("{}...(truncated)", &result_text[..end])
                                        } else {
                                            result_text
                                        };
                                        let _ = s.mission.db().update_task(
                                            &task.id,
                                            &missiond_core::types::TaskUpdate {
                                                status: Some(missiond_core::types::TaskStatus::Done),
                                                finished_at: Some(now),
                                                result: Some(result_text.clone()),
                                                ..Default::default()
                                            },
                                        );
                                        if let Ok(true) = s.mission.db().kb_ops_complete_by_task_id(&task.id, "done", Some(&result_text)) {
                                            info!(task_id = %task.id, "KB operation marked done via task completion");
                                        }
                                        info!(task_id = %task.id, slot_id = %slot_id, elapsed_ms = elapsed,
                                            jsonl_result = jsonl_resp.is_some(),
                                            "Submit task closed: slot returned to Idle");
                                        s.event_bus.publish_traced(
                                            event_bus::DaemonEvent::TaskCompleted { task_id: task.id.clone() },
                                            event_bus::TraceContext {
                                                trace_id: Some(task.id.clone()),
                                                summary: Some(format!("Task completed on {}", slot_id)),
                                                ..Default::default()
                                            },
                                        );
                                    }
                                }
                            }
                            // Always signal submit dispatcher when any slot becomes Idle
                            s.event_bus.publish(event_bus::DaemonEvent::TaskCompleted { task_id: String::new() });
                        }
                    }
                    Ok(missiond_core::ManagerEvent::ConfirmRequired { slot_id, prompt: _, tool_info }) => {
                        // Auto-confirm safe tools for PTY slots
                        let tool_name = tool_info.as_ref()
                            .and_then(|info| info.tool.as_ref())
                            .map(|t| t.name.as_str());
                        let mcp_server = tool_info.as_ref()
                            .and_then(|info| info.tool.as_ref())
                            .and_then(|t| t.mcp_server.as_deref());

                        let should_auto_approve = match (tool_name, mcp_server) {
                            // MissionD's own MCP tools — always safe
                            (Some(name), Some("missiond")) | (Some(name), Some("mission")) => {
                                info!(slot_id = %slot_id, tool = name, "Auto-confirming MissionD MCP tool");
                                true
                            }
                            // Read-only tools — always safe
                            (Some("Read" | "Glob" | "Grep" | "LSP"), _) => {
                                info!(slot_id = %slot_id, tool = tool_name.unwrap(), "Auto-confirming read-only tool");
                                true
                            }
                            // Code editing tools — auto-approve for worker slots
                            (Some("Write" | "Edit" | "NotebookEdit"), _) => {
                                info!(slot_id = %slot_id, tool = tool_name.unwrap(), "Auto-confirming edit tool for worker slot");
                                true
                            }
                            // Bash — auto-approve for worker slots
                            (Some("Bash"), _) => {
                                info!(slot_id = %slot_id, tool = "Bash", "Auto-confirming Bash for worker slot");
                                true
                            }
                            // Other MCP tools (xjp-mcp etc) — auto-approve
                            (Some(name), Some(_server)) => {
                                info!(slot_id = %slot_id, tool = name, server = _server, "Auto-confirming MCP tool");
                                true
                            }
                            // Unknown tool — still approve (slots are trusted workers)
                            (Some(name), None) => {
                                warn!(slot_id = %slot_id, tool = name, "Auto-confirming unknown tool (no MCP server info)");
                                true
                            }
                            // No tool info at all — approve to unblock
                            (None, _) => {
                                warn!(slot_id = %slot_id, "Auto-confirming with no tool info");
                                true
                            }
                        };

                        if should_auto_approve {
                            let pty = s.pty.clone();
                            let sid = slot_id.clone();
                            tokio::spawn(async move {
                                if let Err(e) = pty.confirm(&sid, missiond_core::ConfirmResponse::Yes).await {
                                    warn!(slot_id = %sid, error = %e, "Failed to auto-confirm tool");
                                }
                            });
                        }
                    }
                    Ok(missiond_core::ManagerEvent::McpToolError { slot_id, tool_name, error }) => {
                        warn!(slot_id = %slot_id, tool = %tool_name, "MCP tool error detected, creating incident");
                        let incident = missiond_core::types::MissionIncident {
                            id: uuid::Uuid::new_v4().to_string(),
                            severity: missiond_core::types::IncidentSeverity::High,
                            source: missiond_core::types::IncidentSource::PtySlot,
                            title: format!("MCP 工具不可用: {} ({})", tool_name, slot_id),
                            description: format!(
                                "工位 `{}` 调用 MCP 工具 `{}` 失败。\n\n错误信息:\n```\n{}\n```\n\n建议操作: 重启工位或检查 MCP 服务器配置。",
                                slot_id, tool_name, error
                            ),
                            server_id: None,
                            raw_payload: serde_json::json!({
                                "slot_id": slot_id,
                                "tool_name": tool_name,
                                "error": error,
                            }),
                            created_at: chrono::Utc::now().to_rfc3339(),
                        };
                        if let Err(e) = s.incident_tx.try_send(incident) {
                            warn!("Incident channel full, dropping MCP error incident: {}", e);
                        }
                    }
                    Ok(missiond_core::ManagerEvent::Spawned { .. }) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "PTY logger lagged");
                    }
                    Err(_) => {}
                }
            }
        });
    }

    info!("All event handlers started (isolated tasks)");

    // ---- Slots hot-reload: SIGHUP + fsnotify ----

    // Helper: reload slots and auto-start newly added ones
    async fn handle_slots_reload(state: &AppState) {
        match state.mission.reload_slots_config() {
            Ok(result) => {
                if result.has_changes() {
                    // Auto-start newly added slots with auto_start: true
                    for slot_id in &result.added {
                        if let Some(slot) = state.mission.get_slot(slot_id) {
                            if slot.config.auto_start == Some(true) {
                                info!(slot_id = %slot_id, "Auto-starting newly added slot");
                                match state.mission.spawn_agent(
                                    slot_id,
                                    Some(missiond_core::SpawnOptions {
                                        visible: false,
                                        auto_restart: true,
                                    }),
                                ).await {
                                    Ok(_) => info!(slot_id = %slot_id, "Auto-started new slot"),
                                    Err(e) => warn!(slot_id = %slot_id, error = %e, "Failed to auto-start new slot"),
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

    // fsnotify watcher for slots.yaml
    {
        let watch_state = state.clone();
        let watch_path = slots_path.clone();
        tokio::spawn(async move {
            use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

            let (tx, mut rx) = tokio::sync::mpsc::channel::<Event>(16);

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
                    error!(error = %e, "Failed to create slots.yaml file watcher");
                    return;
                }
            };

            // Watch the parent directory (NonRecursive) to catch file renames/creates
            let watch_dir = watch_path.parent().unwrap_or(&watch_path);
            if let Err(e) = watcher.watch(watch_dir, RecursiveMode::NonRecursive) {
                error!(error = %e, path = %watch_dir.display(), "Failed to watch slots.yaml directory");
                return;
            }

            info!(path = %watch_path.display(), "Watching slots.yaml for changes");

            // Debounce: wait 500ms after last event before reloading
            let mut debounce_timer: Option<tokio::time::Instant> = None;

            loop {
                tokio::select! {
                    Some(event) = rx.recv() => {
                        // Only react to modifications/creates of the actual slots.yaml file
                        let is_slots_file = event.paths.iter().any(|p| {
                            p.file_name().map(|n| n == "slots.yaml").unwrap_or(false)
                        });
                        let is_relevant = matches!(
                            event.kind,
                            EventKind::Modify(_) | EventKind::Create(_)
                        );
                        if is_slots_file && is_relevant {
                            debounce_timer = Some(tokio::time::Instant::now() + std::time::Duration::from_millis(500));
                        }
                    }
                    _ = async {
                        match debounce_timer {
                            Some(deadline) => tokio::time::sleep_until(deadline).await,
                            None => std::future::pending().await,
                        }
                    } => {
                        debounce_timer = None;
                        info!("slots.yaml changed on disk, reloading");
                        handle_slots_reload(&watch_state).await;
                    }
                }
            }
        });
    }

    // Keep main alive — all work is in spawned tasks above.
    // Ctrl+C or SIGTERM triggers graceful shutdown.
    tokio::signal::ctrl_c().await.ok();
    info!("Received shutdown signal, exiting");
    Ok(())
}
