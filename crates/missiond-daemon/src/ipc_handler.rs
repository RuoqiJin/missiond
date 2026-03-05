use anyhow::Result;
use missiond_core::ipc::{IpcListener, IpcStream};
use missiond_mcp::protocol::{self, Request, RequestId, Response, RpcError};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::gemini_client::REQUEST_SESSION_ID;
use crate::state::AppState;
use serde_json::Value;

pub(crate) async fn handle_ipc_connection(state: AppState, mut reader: BufReader<IpcStream>) -> Result<()> {
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).await?;
    if bytes == 0 {
        return Ok(());
    }

    let message = line.trim();
    let request = match protocol::parse_request_str(message) {
        Ok(r) => r,
        Err(e) => {
            let resp = Response::from_error(RequestId::Null, e);
            let json = protocol::serialize_response_string(&resp)?;
            reader.get_mut().write_all(json.as_bytes()).await?;
            reader.get_mut().write_all(b"\n").await?;
            reader.get_mut().flush().await?;
            return Ok(());
        }
    };

    // Extract session_id from request params meta (if provided by MCP proxy)
    let session_id = request.params.as_ref()
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get("session_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    // Wrap handler in task_local scope so all downstream code can read session_id
    let resp = REQUEST_SESSION_ID.scope(session_id, async {
        handle_ipc_request(state, request).await
    }).await;

    let json = protocol::serialize_response_string(&resp)?;
    reader.get_mut().write_all(json.as_bytes()).await?;
    reader.get_mut().write_all(b"\n").await?;
    reader.get_mut().flush().await?;
    Ok(())
}

pub(crate) async fn handle_ipc_request(state: AppState, request: Request) -> Response {
    let id = request.id.clone().unwrap_or(RequestId::Null);
    let method = request.method.as_str();
    let params = request.params.unwrap_or(Value::Null);

    match method {
        "ping" => Response::success(id, serde_json::json!({})),
        "kb/summary" => {
            let db = state.mission.db();
            let instructions = match db.kb_summary() {
                Ok(counts) => {
                    if counts.is_empty() {
                        "MissionD KB is empty. Use mission_kb_remember when you learn new facts. Use mission_kb_search before guessing.".to_string()
                    } else {
                        let parts: Vec<String> = counts
                            .iter()
                            .map(|(cat, n)| format!("{} {}", n, cat))
                            .collect();
                        format!(
                            "[MissionD] KB: {}. Use mission_kb_search before guessing. Use mission_kb_remember when learning.",
                            parts.join(", ")
                        )
                    }
                }
                Err(_) => String::new(),
            };
            Response::success(id, serde_json::json!({ "instructions": instructions }))
        }
        "tools/call" => {
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

            let tool_res = state.call_tool(&name, arguments).await;
            Response::success(id, serde_json::to_value(tool_res).unwrap_or(Value::Null))
        }
        _ => Response::from_error(id, RpcError::MethodNotFound(method.to_string())),
    }
}

pub(crate) async fn bind_ipc_listener(endpoint: &str) -> Result<IpcListener> {
    IpcListener::bind(endpoint).await
}
