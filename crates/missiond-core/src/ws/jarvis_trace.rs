//! Jarvis request tracing — structured log for end-to-end debugging
//!
//! Stores recent Jarvis chat completion requests in a ring buffer
//! for MCP tool querying. No external DB needed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Maximum number of traces to keep in memory
const MAX_TRACES: usize = 100;

/// A single Jarvis request trace record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JarvisTrace {
    /// Unique trace ID (matches chat_id)
    pub trace_id: String,
    /// Timestamp of request arrival
    pub started_at: DateTime<Utc>,
    /// Timestamp of request completion
    pub finished_at: Option<DateTime<Utc>>,
    /// Source address (Router's IP)
    pub source_addr: String,
    /// Slot used
    pub slot_id: String,
    /// User message (truncated to 200 chars for display)
    pub user_message: String,
    /// Full user message length
    pub user_message_len: usize,
    /// Response text (truncated to 500 chars for display)
    pub response_preview: String,
    /// Full response length
    pub response_len: usize,
    /// Duration in milliseconds
    pub duration_ms: Option<u64>,
    /// Final status
    pub status: TraceStatus,
    /// Error message if failed
    pub error: Option<String>,
    /// State transitions observed during this request
    pub state_transitions: Vec<String>,
    /// Router trace_id from X-Trace-Id header (if provided)
    pub router_trace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    /// Request in progress
    InProgress,
    /// Completed successfully
    Success,
    /// Empty response (content extraction failed)
    EmptyResponse,
    /// Error occurred
    Error,
    /// Slot not available
    SlotUnavailable,
}

impl std::fmt::Display for TraceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TraceStatus::InProgress => write!(f, "in_progress"),
            TraceStatus::Success => write!(f, "success"),
            TraceStatus::EmptyResponse => write!(f, "empty_response"),
            TraceStatus::Error => write!(f, "error"),
            TraceStatus::SlotUnavailable => write!(f, "slot_unavailable"),
        }
    }
}

/// Thread-safe trace store
#[derive(Clone)]
pub struct JarvisTraceStore {
    traces: Arc<Mutex<VecDeque<JarvisTrace>>>,
}

impl JarvisTraceStore {
    pub fn new() -> Self {
        Self {
            traces: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_TRACES))),
        }
    }

    /// Record a new trace (returns trace_id for later updates)
    pub async fn start_trace(
        &self,
        trace_id: String,
        source_addr: SocketAddr,
        slot_id: &str,
        user_message: &str,
        router_trace_id: Option<String>,
    ) -> String {
        let trace = JarvisTrace {
            trace_id: trace_id.clone(),
            started_at: Utc::now(),
            finished_at: None,
            source_addr: source_addr.to_string(),
            slot_id: slot_id.to_string(),
            user_message: truncate(user_message, 200),
            user_message_len: user_message.len(),
            response_preview: String::new(),
            response_len: 0,
            duration_ms: None,
            status: TraceStatus::InProgress,
            error: None,
            state_transitions: Vec::new(),
            router_trace_id,
        };

        let mut traces = self.traces.lock().await;
        if traces.len() >= MAX_TRACES {
            traces.pop_front();
        }
        traces.push_back(trace);
        trace_id
    }

    /// Complete a trace with success
    pub async fn complete_trace(
        &self,
        trace_id: &str,
        response: &str,
        duration_ms: u64,
    ) {
        let mut traces = self.traces.lock().await;
        if let Some(trace) = traces.iter_mut().rev().find(|t| t.trace_id == trace_id) {
            trace.finished_at = Some(Utc::now());
            trace.response_preview = truncate(response, 500);
            trace.response_len = response.len();
            trace.duration_ms = Some(duration_ms);
            trace.status = if response.is_empty() {
                TraceStatus::EmptyResponse
            } else {
                TraceStatus::Success
            };
        }
    }

    /// Complete a trace with error
    pub async fn error_trace(
        &self,
        trace_id: &str,
        error: &str,
        duration_ms: Option<u64>,
    ) {
        let mut traces = self.traces.lock().await;
        if let Some(trace) = traces.iter_mut().rev().find(|t| t.trace_id == trace_id) {
            trace.finished_at = Some(Utc::now());
            trace.duration_ms = duration_ms;
            trace.status = TraceStatus::Error;
            trace.error = Some(error.to_string());
        }
    }

    /// Mark trace as slot unavailable (without starting a real trace)
    pub async fn unavailable_trace(
        &self,
        trace_id: String,
        source_addr: SocketAddr,
        slot_id: &str,
        user_message: &str,
        error: &str,
        router_trace_id: Option<String>,
    ) {
        let trace = JarvisTrace {
            trace_id,
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            source_addr: source_addr.to_string(),
            slot_id: slot_id.to_string(),
            user_message: truncate(user_message, 200),
            user_message_len: user_message.len(),
            response_preview: String::new(),
            response_len: 0,
            duration_ms: Some(0),
            status: TraceStatus::SlotUnavailable,
            error: Some(error.to_string()),
            state_transitions: Vec::new(),
            router_trace_id,
        };

        let mut traces = self.traces.lock().await;
        if traces.len() >= MAX_TRACES {
            traces.pop_front();
        }
        traces.push_back(trace);
    }

    /// Get recent traces (newest first)
    pub async fn list_traces(&self, limit: usize) -> Vec<JarvisTrace> {
        let traces = self.traces.lock().await;
        traces.iter().rev().take(limit).cloned().collect()
    }

    /// Get a specific trace by ID
    pub async fn get_trace(&self, trace_id: &str) -> Option<JarvisTrace> {
        let traces = self.traces.lock().await;
        traces.iter().rev().find(|t| t.trace_id == trace_id).cloned()
    }

    /// Get latest trace
    pub async fn latest_trace(&self) -> Option<JarvisTrace> {
        let traces = self.traces.lock().await;
        traces.back().cloned()
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len).collect();
        format!("{}...", truncated)
    }
}
