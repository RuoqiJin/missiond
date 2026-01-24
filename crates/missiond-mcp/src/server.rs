//! MCP Server implementation
//!
//! This module implements the MCP server that handles stdio-based JSON-RPC communication.

use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};
use serde_json::{json, Value};

use crate::protocol::{self, Request, RequestId, Response, RpcError};
use crate::tools::{self, ToolResult};

/// Server information
const SERVER_NAME: &str = "missiond";
const SERVER_VERSION: &str = "0.2.0";
const PROTOCOL_VERSION: &str = "2024-11-05";

/// MCP Server capabilities
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

/// Tool handler trait
///
/// Implement this trait to handle tool calls.
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    /// Handle a tool call
    async fn call(&self, name: &str, arguments: Value) -> ToolResult;
}

/// A placeholder tool handler that returns "not implemented" for all tools
pub struct PlaceholderHandler;

#[async_trait::async_trait]
impl ToolHandler for PlaceholderHandler {
    async fn call(&self, name: &str, _arguments: Value) -> ToolResult {
        ToolResult::error(format!("Tool '{}' not implemented yet", name))
    }
}

/// MCP Server
pub struct McpServer<H: ToolHandler> {
    handler: Arc<H>,
    capabilities: ServerCapabilities,
    initialized: bool,
}

impl<H: ToolHandler> McpServer<H> {
    /// Create a new MCP server with a tool handler
    pub fn new(handler: H) -> Self {
        McpServer {
            handler: Arc::new(handler),
            capabilities: ServerCapabilities::default(),
            initialized: false,
        }
    }

    /// Create a new MCP server with custom capabilities
    pub fn with_capabilities(handler: H, capabilities: ServerCapabilities) -> Self {
        McpServer {
            handler: Arc::new(handler),
            capabilities,
            initialized: false,
        }
    }

    /// Run the server on stdio
    pub async fn run(&mut self) -> anyhow::Result<()> {
        info!("Starting MCP server on stdio");

        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);

        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;

            if bytes_read == 0 {
                info!("EOF received, shutting down");
                break;
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            debug!("Received: {}", line);

            let response = self.handle_message(line).await;
            let response_json = protocol::serialize_response_string(&response)?;

            debug!("Sending: {}", response_json);

            stdout.write_all(response_json.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }

        Ok(())
    }

    /// Handle a single JSON-RPC message
    async fn handle_message(&mut self, message: &str) -> Response {
        match protocol::parse_request_str(message) {
            Ok(request) => self.handle_request(request).await,
            Err(err) => Response::from_error(RequestId::Null, err),
        }
    }

    /// Handle a parsed request
    async fn handle_request(&mut self, request: Request) -> Response {
        let id = request.id.clone();
        let method = request.method.as_str();
        let params = request.params.unwrap_or(Value::Null);

        match method {
            "initialize" => self.handle_initialize(id, params),
            "notifications/initialized" => {
                // This is a notification, we don't need to respond
                // But since we're treating everything as request/response, return success
                Response::success(id, json!({}))
            }
            "tools/list" => self.handle_tools_list(id),
            "tools/call" => self.handle_tools_call(id, params).await,
            "ping" => Response::success(id, json!({})),
            _ => {
                warn!("Unknown method: {}", method);
                Response::from_error(id, RpcError::MethodNotFound(method.to_string()))
            }
        }
    }

    /// Handle initialize request
    fn handle_initialize(&mut self, id: RequestId, _params: Value) -> Response {
        self.initialized = true;
        info!("MCP server initialized");

        let result = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": if self.capabilities.tools { json!({}) } else { Value::Null },
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION,
            }
        });

        Response::success(id, result)
    }

    /// Handle tools/list request
    fn handle_tools_list(&self, id: RequestId) -> Response {
        let tools = tools::all_tools();
        Response::success(id, json!({ "tools": tools }))
    }

    /// Handle tools/call request
    async fn handle_tools_call(&self, id: RequestId, params: Value) -> Response {
        // Extract tool name and arguments
        let name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => {
                return Response::from_error(
                    id,
                    RpcError::InvalidParams("Missing 'name' field".to_string()),
                );
            }
        };

        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(Value::Object(serde_json::Map::new()));

        debug!("Calling tool: {} with args: {:?}", name, arguments);

        // Validate tool exists
        if tools::get_tool(&name).is_none() {
            return Response::from_error(
                id,
                RpcError::InvalidParams(format!("Unknown tool: {}", name)),
            );
        }

        // Call the handler
        let result = self.handler.call(&name, arguments).await;

        Response::success(id, serde_json::to_value(result).unwrap_or(Value::Null))
    }
}

/// Create and run an MCP server with a placeholder handler
pub async fn run_placeholder_server() -> anyhow::Result<()> {
    let mut server = McpServer::new(PlaceholderHandler);
    server.run().await
}

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
        let mut server = McpServer::new(TestHandler);
        let request = Request {
            jsonrpc: "2.0".to_string(),
            method: "initialize".to_string(),
            params: Some(json!({})),
            id: RequestId::Number(1),
        };

        let response = server.handle_request(request).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "missiond");
    }

    #[tokio::test]
    async fn test_handle_tools_list() {
        let server = McpServer::new(TestHandler);
        let response = server.handle_tools_list(RequestId::Number(1));

        assert!(response.result.is_some());
        let result = response.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
    }

    #[tokio::test]
    async fn test_handle_tools_call() {
        let server = McpServer::new(TestHandler);
        let params = json!({
            "name": "mission_submit",
            "arguments": {
                "role": "test",
                "prompt": "test prompt"
            }
        });

        let response = server.handle_tools_call(RequestId::Number(1), params).await;
        assert!(response.result.is_some());
    }

    #[tokio::test]
    async fn test_handle_unknown_method() {
        let mut server = McpServer::new(TestHandler);
        let request = Request {
            jsonrpc: "2.0".to_string(),
            method: "unknown/method".to_string(),
            params: None,
            id: RequestId::Number(1),
        };

        let response = server.handle_request(request).await;
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, -32601);
    }
}
