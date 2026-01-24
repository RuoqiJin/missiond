//! Claude CLI Runner implementation
//!
//! Wraps the `claude` CLI with stream-json output parsing.

use crate::types::*;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, warn};

/// Claude Code CLI Runner
///
/// Wraps `claude --print --output-format stream-json` execution.
pub struct ClaudeRunner {
    /// Running process handle
    process: Arc<Mutex<Option<Child>>>,
    /// Cancellation flag
    cancelled: Arc<AtomicBool>,
}

impl Default for ClaudeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeRunner {
    /// Create a new runner
    pub fn new() -> Self {
        Self {
            process: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Execute Claude CLI
    pub async fn run(&self, options: RunOptions) -> Result<RunResult, RunnerError> {
        let RunOptions {
            prompt,
            cwd,
            session_id,
            mcp_config,
            timeout,
            on_progress,
        } = options;

        // Reset cancellation flag
        self.cancelled.store(false, Ordering::SeqCst);

        // Build command arguments
        let mut args = vec![
            "--print".to_string(),
            prompt,
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
        ];

        if let Some(sid) = session_id {
            args.push("--resume".to_string());
            args.push(sid);
        }

        if let Some(mcp) = mcp_config {
            args.push("--mcp-config".to_string());
            args.push(mcp.to_string_lossy().to_string());
        }

        // Build command
        let mut cmd = Command::new("claude");
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        debug!(?args, "Starting Claude CLI");

        // Spawn process
        let mut child = cmd.spawn()?;

        // Get stdout/stderr handles
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RunnerError::CliFailed("Failed to capture stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| RunnerError::CliFailed("Failed to capture stderr".into()))?;

        // Store child for potential cancellation
        *self.process.lock().await = Some(child);

        // Parse stream-json output
        let mut result_event: Option<ResultEvent> = None;
        let mut errors: Vec<String> = Vec::new();

        // Read stdout lines
        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr);

        // Spawn stderr reader
        let errors_handle = {
            let errors = Arc::new(Mutex::new(errors.clone()));
            let errors_ref = errors.clone();
            tokio::spawn(async move {
                let mut buf = String::new();
                loop {
                    buf.clear();
                    match tokio::io::AsyncBufReadExt::read_line(&mut stderr_reader, &mut buf).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let line = buf.trim().to_string();
                            if !line.is_empty() {
                                errors_ref.lock().await.push(line);
                            }
                        }
                        Err(_) => break,
                    }
                }
                errors_ref.lock().await.clone()
            })
        };

        // Process stdout with timeout
        let cancelled = self.cancelled.clone();
        let process_result = tokio::time::timeout(timeout, async {
            while let Some(line) = stdout_reader.next_line().await? {
                // Check cancellation
                if cancelled.load(Ordering::SeqCst) {
                    return Err(RunnerError::Cancelled);
                }

                if line.trim().is_empty() {
                    continue;
                }

                // Parse JSON line
                match serde_json::from_str::<serde_json::Value>(&line) {
                    Ok(value) => {
                        // Try to parse as StreamEvent
                        if let Some(event_type) = value.get("type").and_then(|v| v.as_str()) {
                            match event_type {
                                "result" => {
                                    if let Ok(evt) = serde_json::from_value::<ResultEvent>(value.clone()) {
                                        result_event = Some(evt.clone());
                                        if let Some(ref cb) = on_progress {
                                            cb(StreamEvent::Result(evt));
                                        }
                                    }
                                }
                                "assistant" => {
                                    if let Ok(evt) = serde_json::from_value::<AssistantEvent>(value.clone()) {
                                        if let Some(ref cb) = on_progress {
                                            cb(StreamEvent::Assistant(evt));
                                        }
                                    }
                                }
                                "user" => {
                                    if let Ok(evt) = serde_json::from_value::<UserEvent>(value.clone()) {
                                        if let Some(ref cb) = on_progress {
                                            cb(StreamEvent::User(evt));
                                        }
                                    }
                                }
                                "system" => {
                                    if let Ok(evt) = serde_json::from_value::<SystemEvent>(value.clone()) {
                                        if let Some(ref cb) = on_progress {
                                            cb(StreamEvent::System(evt));
                                        }
                                    }
                                }
                                _ => {
                                    debug!(?event_type, "Unknown event type");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        // Non-JSON line, might be debug output
                        debug!(?line, ?e, "Non-JSON line from Claude CLI");
                    }
                }
            }
            Ok::<_, RunnerError>(())
        })
        .await;

        // Wait for stderr reader
        errors = errors_handle.await.unwrap_or_default();

        // Wait for process to complete
        let mut proc_guard = self.process.lock().await;
        if let Some(mut child) = proc_guard.take() {
            let _ = child.wait().await;
        }

        // Handle timeout
        match process_result {
            Err(_) => {
                return Err(RunnerError::Timeout(timeout));
            }
            Ok(Err(e)) => {
                return Err(e);
            }
            Ok(Ok(())) => {}
        }

        // Check for result
        let result_event = result_event.ok_or_else(|| {
            RunnerError::NoResult(errors.join("\n"))
        })?;

        // Convert to RunResult
        Ok(RunResult {
            session_id: result_event.session_id,
            result: result_event.result,
            is_error: result_event.is_error,
            duration_ms: result_event.duration_ms,
            duration_api_ms: result_event.duration_api_ms,
            num_turns: result_event.num_turns,
            total_cost_usd: result_event.total_cost_usd,
            usage: result_event.usage.map(|u| Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
            }),
        })
    }

    /// Cancel the running process
    pub async fn cancel(&self) {
        // Set cancellation flag
        self.cancelled.store(true, Ordering::SeqCst);

        // Try to kill the process
        let mut proc = self.process.lock().await;
        if let Some(ref mut child) = *proc {
            // Send SIGTERM first
            if let Err(e) = child.kill().await {
                warn!(?e, "Failed to kill Claude CLI process");
            }
        }
    }

    /// Check if a process is running
    pub async fn is_running(&self) -> bool {
        let proc = self.process.lock().await;
        proc.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_result_event() {
        let json = r#"{
            "type": "result",
            "subtype": "success",
            "session_id": "abc123",
            "result": "Hello, world!",
            "is_error": false,
            "duration_ms": 1000,
            "duration_api_ms": 800,
            "num_turns": 1,
            "total_cost_usd": 0.001,
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50
            }
        }"#;

        let event: ResultEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.session_id, "abc123");
        assert_eq!(event.result, "Hello, world!");
        assert!(!event.is_error);
        assert_eq!(event.num_turns, 1);
    }

    #[test]
    fn test_parse_assistant_event() {
        let json = r#"{
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "Hello!"},
                    {"type": "tool_use", "id": "tool1", "name": "bash", "input": {"command": "ls"}}
                ]
            }
        }"#;

        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let event: AssistantEvent = serde_json::from_value(value).unwrap();
        assert_eq!(event.message.content.len(), 2);
    }
}
