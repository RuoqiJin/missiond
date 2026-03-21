//! PTY-based transport for Gemini CLI.
//!
//! Thin wrapper around GeminiPtyDriver that provides the call() interface
//! consumed by GeminiCli. All PTY operations are delegated to the unified
//! driver, which operates through PTYManager (no private PTYSession).
//!
//! Concurrency safety (two layers):
//! 1. `GeminiClient` semaphore (capacity=1, with acquisition timeout) — queue management
//! 2. `GeminiPtyTransport` Mutex — defense-in-depth, atomic `/clear` + `send`

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::sync::Mutex;
use tracing::info;

use super::gemini_driver::GeminiPtyDriver;

/// PTY-based Gemini CLI transport.
///
/// Delegates all PTY operations to GeminiPtyDriver (which uses PTYManager).
/// Access is serialized via Mutex — ensure_spawned + /clear + ask are atomic.
pub(crate) struct GeminiPtyTransport {
    driver: GeminiPtyDriver,
    slot_id: String,
    cwd: PathBuf,
    /// Mutex prevents concurrent requests from interleaving on the same PTY slot.
    mutex: Mutex<()>,
}

impl GeminiPtyTransport {
    pub fn new(driver: GeminiPtyDriver, slot_id: String, cwd: PathBuf) -> Self {
        Self {
            driver,
            slot_id,
            cwd,
            mutex: Mutex::new(()),
        }
    }

    /// Send a prompt via PTY and wait for Gemini's response.
    ///
    /// Flow (all under Mutex — atomic, no interleaving):
    ///   1. Ensure slot alive (auto-spawn if dead)
    ///   2. `/clear` to reset Gemini's internal context
    ///   3. Send prompt and wait for TextComplete (via Driver)
    pub async fn call(&self, prompt: &str, idle_timeout: Duration) -> Result<String> {
        if prompt.is_empty() {
            return Err(anyhow!("Empty prompt"));
        }

        info!(
            prompt_len = prompt.len(),
            timeout_secs = idle_timeout.as_secs(),
            slot_id = %self.slot_id,
            "GeminiPtyTransport: call"
        );

        let _guard = self.mutex.lock().await;

        // 1. Ensure alive (auto-spawn if dead)
        self.driver
            .ensure_spawned(&self.slot_id, &self.cwd, false, None)
            .await?;

        // 2. /clear context isolation
        self.driver.clear_context(&self.slot_id).await?;

        // 3. Send and wait for response
        let response = self.driver.ask(&self.slot_id, prompt, idle_timeout).await?;

        if response.is_empty() {
            return Err(anyhow!(
                "GeminiPtyTransport: empty response from Gemini CLI"
            ));
        }

        info!(response_len = response.len(), "GeminiPtyTransport: done");
        Ok(response)
    }
}

impl std::fmt::Debug for GeminiPtyTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiPtyTransport")
            .field("slot_id", &self.slot_id)
            .field("cwd", &self.cwd)
            .finish()
    }
}
