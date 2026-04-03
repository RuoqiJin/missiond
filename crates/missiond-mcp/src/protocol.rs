//! ⚠️ LEGACY — Protocol types now live in gen_gateway.rs
//!
//! This file re-exports from the Forge-generated module for backward compatibility.
//! New code should import directly from `crate::gen_gateway` or `crate`.

pub use crate::gen_gateway::{ErrorObject, Request, RequestId, Response, RpcError};

/// JSON-RPC 2.0 version constant
pub const JSONRPC_VERSION: &str = "2.0";

/// Parse a JSON-RPC request from a string
pub fn parse_request_str(text: &str) -> Result<Request, RpcError> {
    let request: Request = serde_json::from_str(text)
        .map_err(|e| RpcError::ParseError(Some(e.to_string())))?;
    if request.jsonrpc != JSONRPC_VERSION {
        return Err(RpcError::InvalidRequest(Some(format!(
            "Expected jsonrpc version '{}', got '{}'",
            JSONRPC_VERSION, request.jsonrpc
        ))));
    }
    Ok(request)
}

/// Parse a JSON-RPC request from bytes
pub fn parse_request(data: &[u8]) -> Result<Request, RpcError> {
    let text = std::str::from_utf8(data)
        .map_err(|e| RpcError::ParseError(Some(e.to_string())))?;
    parse_request_str(text)
}

/// Serialize a response to a JSON string
pub fn serialize_response_string(response: &Response) -> Result<String, RpcError> {
    serde_json::to_string(response)
        .map_err(|e| RpcError::InternalError(e.to_string()))
}

/// Serialize a response to JSON bytes
pub fn serialize_response(response: &Response) -> Result<Vec<u8>, RpcError> {
    serde_json::to_vec(response)
        .map_err(|e| RpcError::InternalError(e.to_string()))
}
