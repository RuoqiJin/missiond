//! EngineController — low-level PTY operations abstraction.
//!
//! Separates concerns:
//!   SlotManager → concurrency control (Mutex/Semaphore), lifecycle decisions
//!   Controller  → PTY interaction, event-driven state machine, result extraction
//!
//! Fixes the fatal race condition where `is_available()` returns true before
//! `agent_info` is updated by the async event loop after `send_fire_and_forget`.

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use super::types::SlotTaskRequest;

/// Engine-specific PTY operations controller.
/// Each engine (Claude Code, Gemini CLI) has a specialized implementation
/// that handles spawning, prompt delivery, event-driven completion detection,
/// and result extraction.
#[async_trait]
pub trait EngineController: Send + Sync {
    /// Check if the PTY slot is alive.
    async fn is_alive(&self, slot_id: &str) -> bool;

    /// Spawn a PTY slot and register its session in DB.
    /// Returns the session_id.
    async fn spawn_and_register(
        &self,
        slot_id: &str,
        req: &SlotTaskRequest,
        is_ephemeral: bool,
    ) -> Result<String>;

    /// Send prompt and wait for complete response.
    /// Internally: subscribe → send → wait(Thinking) → wait(TextComplete) → return content.
    /// This is race-free because subscription happens BEFORE sending.
    async fn ask(
        &self,
        slot_id: &str,
        prompt: &str,
        timeout: Duration,
    ) -> Result<String>;

    /// Clear context. No-op for Claude Code, `/clear` for Gemini CLI.
    async fn clear_context(&self, slot_id: &str) -> Result<()>;

    /// Kill the PTY slot process.
    async fn destroy(&self, slot_id: &str) -> Result<()>;
}
