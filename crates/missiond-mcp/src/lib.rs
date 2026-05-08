//! MCP (Model Context Protocol) Server for missiond
//!
//! Architecture: Generation Gap Pattern
//! - gen_gateway.rs: 🤖 GENERATED — protocol types, dispatch, trait contract
//! - gateway_impl.rs: ✍️ CUSTOM — MissiondMcp trait implementation
//! - protocol.rs: ⚠️ LEGACY — retained for backward compat, delegates to gen_gateway
//! - server.rs: ⚠️ LEGACY — retained for backward compat, wraps McpGateway
//! - tools/: Tool definitions and schemas (unchanged)

pub mod gateway_impl;
pub mod gen_gateway;
pub mod protocol;
pub mod server;
pub mod tools;

// New API — Forge-generated types
pub use gateway_impl::{McpGateway, PlaceholderHandler, ToolHandler};
pub use gen_gateway::{
    dispatch_tool, error_codes, run_stdio, ErrorObject, MissiondMcp, Request, RequestId, Response,
    RpcError,
};

// Tool types
pub use tools::{all_tools, get_tool, ToolContent, ToolDefinition, ToolResult};
