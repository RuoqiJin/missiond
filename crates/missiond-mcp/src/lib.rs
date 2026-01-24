//! MCP (Model Context Protocol) Server for missiond
//!
//! This crate provides a JSON-RPC 2.0 based MCP server implementation
//! for the missiond multi-agent orchestration system.
//!
//! # Features
//!
//! - Self-contained JSON-RPC 2.0 protocol implementation
//! - Async stdio-based transport using tokio
//! - 29 MCP tools for agent management:
//!   - Task operations: submit, ask, status, cancel
//!   - Process control: spawn, kill, restart, agents
//!   - PTY sessions: spawn, send, kill, screen, history, status, confirm, interrupt, logs
//!   - Permission management: get, set_role, set_slot, add_auto_allow, reload
//!   - Claude Code Tasks: cc_sessions, cc_tasks, cc_overview, cc_in_progress, cc_trigger_swarm
//!   - Query: slots, inbox
//!
//! # Example
//!
//! ```no_run
//! use missiond_mcp::{McpServer, ToolHandler, ToolResult};
//! use serde_json::Value;
//!
//! struct MyHandler;
//!
//! #[async_trait::async_trait]
//! impl ToolHandler for MyHandler {
//!     async fn call(&self, name: &str, arguments: Value) -> ToolResult {
//!         // Handle tool calls
//!         ToolResult::json(&serde_json::json!({"status": "ok"}))
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mut server = McpServer::new(MyHandler);
//!     server.run().await
//! }
//! ```

pub mod protocol;
pub mod server;
pub mod tools;

// Re-exports for convenience
pub use protocol::{Request, RequestId, Response, RpcError};
pub use server::{McpServer, PlaceholderHandler, ServerCapabilities, ToolHandler};
pub use tools::{ToolContent, ToolDefinition, ToolResult, all_tools, get_tool};
