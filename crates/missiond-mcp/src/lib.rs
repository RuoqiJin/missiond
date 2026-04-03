//! MCP (Model Context Protocol) Server for missiond
//!
//! Architecture: Generation Gap Pattern
//! - gen_gateway.rs: 🤖 GENERATED — protocol types, dispatch, trait contract
//! - gateway_impl.rs: ✍️ CUSTOM — MissiondMcp trait implementation
//! - protocol.rs: ⚠️ LEGACY — retained for backward compat, delegates to gen_gateway
//! - server.rs: ⚠️ LEGACY — retained for backward compat, wraps McpGateway
//! - tools/: Tool definitions and schemas (unchanged)

pub mod gen_gateway;
pub mod gateway_impl;
pub mod protocol;
pub mod server;
pub mod tools;

// New API — Forge-generated types
pub use gen_gateway::{
    error_codes, dispatch_tool, run_stdio,
    ErrorObject, MissiondMcp, Request, RequestId, Response, RpcError,
};
pub use gateway_impl::{McpGateway, PlaceholderHandler, ToolHandler};

// Tool types
pub use tools::{ToolContent, ToolDefinition, ToolResult, all_tools, get_tool};
