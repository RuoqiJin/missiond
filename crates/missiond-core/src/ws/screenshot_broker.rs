//! Screenshot Broker — coordinates browser-based PTY screenshot capture
//!
//! Flow: MCP tool → request() → broadcast to WS clients → browser captures
//! xterm.js canvas → resolve() → MCP tool gets PNG bytes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, oneshot, Mutex};
use tracing::{debug, warn};

/// Result from a browser screenshot capture
pub struct ScreenshotResult {
    pub png_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Coordinates screenshot request/response between MCP handlers and WS clients
pub struct ScreenshotBroker {
    /// request_id → oneshot sender for the result
    pending: Mutex<HashMap<String, oneshot::Sender<Result<ScreenshotResult, String>>>>,
    /// Broadcast channel to notify WS handlers of new screenshot requests
    /// Payload: (slot_id, request_id)
    request_tx: broadcast::Sender<(String, String)>,
    /// Default timeout for browser response
    pub timeout: Duration,
}

impl ScreenshotBroker {
    pub fn new(timeout: Duration) -> Arc<Self> {
        let (request_tx, _) = broadcast::channel(16);
        Arc::new(Self {
            pending: Mutex::new(HashMap::new()),
            request_tx,
            timeout,
        })
    }

    /// Register a screenshot request and broadcast to WS clients.
    /// Returns (request_id, receiver). Caller awaits the receiver with timeout.
    pub async fn request(&self, slot_id: &str) -> (String, oneshot::Receiver<Result<ScreenshotResult, String>>) {
        let request_id = format!(
            "ss-{}-{}",
            chrono::Utc::now().timestamp_millis(),
            &uuid::Uuid::new_v4().to_string()[..6]
        );

        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        // Broadcast to all WS handlers
        let _ = self.request_tx.send((slot_id.to_string(), request_id.clone()));
        debug!(slot_id, request_id, "Screenshot request broadcast");

        (request_id, rx)
    }

    /// Resolve a pending request (called when browser sends screenshot_response)
    pub async fn resolve(&self, request_id: &str, result: Result<ScreenshotResult, String>) {
        let tx = {
            let mut pending = self.pending.lock().await;
            pending.remove(request_id)
        };

        if let Some(tx) = tx {
            let _ = tx.send(result);
            debug!(request_id, "Screenshot request resolved");
        } else {
            warn!(request_id, "Screenshot response for unknown/expired request");
        }
    }

    /// Subscribe to screenshot request broadcasts (called by WS connection handlers)
    pub fn subscribe(&self) -> broadcast::Receiver<(String, String)> {
        self.request_tx.subscribe()
    }
}
