//! Custom implementation of the MissiondMcp trait.
//!
//! This is the Generation Gap "implementation island":
//! - gen_gateway.rs provides the protocol types + dispatch + trait contract
//! - This file implements the trait, bridging to the existing ToolHandler
//!
//! Business logic lives here. Protocol boilerplate lives in gen_gateway.rs.

use std::sync::Arc;
use anyhow::Result;
use serde_json::{json, Value};
use tracing::{debug, info};

use crate::gen_gateway::*;
use crate::tools::{self, ToolResult};

/// Server constants
const SERVER_NAME: &str = "missiond";
const SERVER_VERSION: &str = "0.2.0";
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Tool handler trait — implement this to handle tool calls from daemon.
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    async fn call(&self, name: &str, arguments: Value) -> ToolResult;
}

/// MCP Gateway server — implements the generated MissiondMcp trait.
pub struct McpGateway<H: ToolHandler> {
    handler: Arc<H>,
    instructions: Option<String>,
}

impl<H: ToolHandler + 'static> McpGateway<H> {
    pub fn new(handler: H) -> Self {
        Self {
            handler: Arc::new(handler),
            instructions: None,
        }
    }

    pub fn set_instructions(&mut self, instructions: String) {
        self.instructions = Some(instructions);
    }

    /// Run on stdio using the generated transport loop.
    pub async fn run(&self) -> Result<()> {
        info!("Starting MCP gateway on stdio");
        run_stdio(self).await
    }
}

#[async_trait::async_trait]
impl<H: ToolHandler + 'static> MissiondMcp for McpGateway<H> {
    async fn handle_initialize(&self, id: RequestId, _params: Value) -> Response {
        info!("MCP gateway initialized");
        let mut result = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": {},
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION,
            }
        });
        if let Some(ref instructions) = self.instructions {
            result["instructions"] = json!(instructions);
        }
        Response::success(id, result)
    }

    async fn handle_notifications_initialized(&self, id: RequestId, _params: Value) -> Response {
        debug!("Received initialized notification");
        Response::success(id, json!({}))
    }

    async fn handle_tools_list(&self, id: RequestId, _params: Value) -> Response {
        let tools = tools::all_tools();
        Response::success(id, json!({ "tools": tools }))
    }

    async fn handle_tools_call(&self, id: RequestId, params: Value) -> Response {
        let name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => {
                return Response::from_error(
                    id,
                    RpcError::InvalidParams("Missing 'name' field".into()),
                );
            }
        };

        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(Value::Object(serde_json::Map::new()));

        debug!("Calling tool: {} with args: {:?}", name, arguments);

        if tools::get_tool(&name).is_none() {
            return Response::from_error(
                id,
                RpcError::InvalidParams(format!("Unknown tool: {}", name)),
            );
        }

        let result = self.handler.call(&name, arguments).await;
        Response::success(id, serde_json::to_value(result).unwrap_or(Value::Null))
    }

    async fn handle_ping(&self, id: RequestId, _params: Value) -> Response {
        Response::success(id, json!({}))
    }

    // ─── Tool dispatch group handlers ───────────────────────────
    // All delegate to the single ToolHandler::call() interface.
    // The dispatch_tool() in gen_gateway.rs routes by name → group,
    // but at the MCP layer all groups go through the same IPC bridge.

    async fn handle_task(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_pty(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_slot(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_cc_tasks(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_worker(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_job(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_kb(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_board(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_skill(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_memory(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_insight(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_embedding(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_conversation(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_question(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_router_chat(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_timeline(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_audit(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_retrospective(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_decision(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_infra(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_permission(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_system(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }

    async fn handle_external(&self, tool_name: &str, args: Value) -> Result<Value> {
        Ok(serde_json::to_value(self.handler.call(tool_name, args).await)?)
    }
}

/// A placeholder handler that returns errors for all tools.
pub struct PlaceholderHandler;

#[async_trait::async_trait]
impl ToolHandler for PlaceholderHandler {
    async fn call(&self, name: &str, _arguments: Value) -> ToolResult {
        ToolResult::error(format!("Tool '{}' not implemented yet", name))
    }
}

// ═══════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHandler;

    #[async_trait::async_trait]
    impl ToolHandler for TestHandler {
        async fn call(&self, name: &str, _arguments: Value) -> ToolResult {
            ToolResult::json(&json!({ "tool": name, "status": "ok" }))
        }
    }

    #[tokio::test]
    async fn test_handle_initialize() {
        let gw = McpGateway::new(TestHandler);
        let resp = gw.handle_initialize(RequestId::Number(1), json!({})).await;
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "missiond");
    }

    #[tokio::test]
    async fn test_handle_tools_list() {
        let gw = McpGateway::new(TestHandler);
        let resp = gw.handle_tools_list(RequestId::Number(1), json!({})).await;
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
    }

    #[tokio::test]
    async fn test_handle_tools_call() {
        let gw = McpGateway::new(TestHandler);
        let params = json!({
            "name": "mission_task_submit",
            "arguments": { "action": "async", "role": "test" }
        });
        let resp = gw.handle_tools_call(RequestId::Number(1), params).await;
        assert!(resp.result.is_some());
    }

    #[tokio::test]
    async fn test_handle_ping() {
        let gw = McpGateway::new(TestHandler);
        let resp = gw.handle_ping(RequestId::Number(1), json!({})).await;
        assert!(resp.result.is_some());
    }

    #[tokio::test]
    async fn test_unknown_method() {
        // The generated run_stdio() handles unknown methods via RpcError::MethodNotFound.
        // Verify the Response constructors work.
        let resp = Response::from_error(
            RequestId::Number(1),
            RpcError::MethodNotFound("unknown/method".into()),
        );
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }
}
