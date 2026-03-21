//! Slot Orchestrator — unified task routing layer above PTYManager.
//!
//! Architecture (3-layer):
//!   AgentSlotManager (L1: registry + routing)
//!     ├─ ClaudeCodeSlotManager (L2: concurrency control — Mutex/Semaphore)
//!     │     └─ ClaudeCodeController (L3: PTY operations — event-driven ask/spawn/destroy)
//!     └─ GeminiCliSlotManager  (Phase 3)
//!           └─ GeminiCliController (Phase 3)
//!
//! Result extraction: TextComplete event content (event-driven, not DB polling).
//! Session categorization: agent-persistent / agent-ephemeral (separated from user sessions).

pub mod agent;
pub mod cc_controller;
pub mod claude_code;
pub mod controller;
pub mod gemini_cli;
pub mod gemini_controller;
pub mod spawner;
pub mod types;

pub use agent::AgentSlotManager;
pub use claude_code::ClaudeCodeSlotManager;
pub use gemini_cli::GeminiCliSlotManager;
pub use types::{EngineSlotManager, EngineStatus, SlotTaskConfig, SlotTaskRequest};

use missiond_core::db::traits::MissionStore;
use missiond_core::types::Conversation;
use std::sync::Arc;
use tracing::info;

/// Register a slot's session in DB for log categorization.
/// Separates agent sessions from user sessions via conversation_type field.
pub(crate) async fn register_slot_session(
    store: &Arc<dyn MissionStore>,
    slot_id: &str,
    session_id: &str,
    _is_ephemeral: bool,
) {
    // 1. Map slot_id → session_id in slot_sessions table
    if let Err(e) = store.set_slot_session(slot_id, session_id).await {
        tracing::warn!(slot_id, session_id, "Failed to set slot session: {}", e);
        return;
    }

    // 1.5. Clean up any leftover PTY placeholder conversation
    let _ = store.cleanup_pty_placeholder(slot_id).await;

    // 2. Ensure conversation record exists — use derive_conversation_type (single source of truth)
    let category: Option<String> = None;
    let conversation_type = missiond_core::db::derive_conversation_type(category.as_deref(), Some(slot_id), session_id, "pty_jsonl");

    let conv = Conversation {
        id: session_id.to_string(),
        project: None,
        slot_id: Some(slot_id.to_string()),
        source: "pty_jsonl".to_string(),
        model: None,
        git_branch: None,
        jsonl_path: None,
        parent_session_id: None,
        task_id: None,
        message_count: 0,
        started_at: chrono::Utc::now().to_rfc3339(),
        ended_at: None,
        status: "active".to_string(),
        analyzed_at: None,
        analysis_version: 0,
        analysis_retries: 0,
        deep_analyzed_message_id: 0,
        chat_type: Some("pty".to_string()),
        conversation_type: conversation_type.clone(),
        updated_at: None,
        llm_summary: None,
        embedding_provider: None,
        session_timeline: None,
        timeline_built_at: None,
    };

    if let Err(e) = store.upsert_conversation(&conv).await {
        tracing::warn!(
            slot_id,
            session_id,
            "Failed to register conversation: {}",
            e
        );
    } else {
        info!(
            slot_id,
            session_id, conversation_type, "Slot session registered in DB"
        );
    }
}
