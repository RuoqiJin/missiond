//! ⚠️ LEGACY — Server API now lives in gateway_impl.rs + gen_gateway.rs
//!
//! This file provides backward-compatible re-exports.
//! New code should use McpGateway directly.

pub use crate::gateway_impl::{McpGateway, PlaceholderHandler, ToolHandler};

/// Legacy type alias — use McpGateway directly for new code.
pub type McpServer<H> = McpGateway<H>;

/// Server capabilities (retained for backward compat).
#[derive(Debug, Clone)]
pub struct ServerCapabilities {
    pub tools: bool,
    pub prompts: bool,
    pub resources: bool,
}

impl Default for ServerCapabilities {
    fn default() -> Self {
        ServerCapabilities {
            tools: true,
            prompts: false,
            resources: false,
        }
    }
}

/// Legacy entry point
pub async fn run_placeholder_server() -> anyhow::Result<()> {
    let gw = McpGateway::new(PlaceholderHandler);
    gw.run().await
}
