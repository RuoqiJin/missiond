//! JSON-RPC 2.0 protocol implementation for MCP
//!
//! This module implements the JSON-RPC 2.0 protocol used by MCP (Model Context Protocol).
//! The implementation is self-contained without external JSON-RPC libraries.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 version constant
pub const JSONRPC_VERSION: &str = "2.0";

/// A JSON-RPC 2.0 request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Must be "2.0"
    pub jsonrpc: String,
    /// Method name
    pub method: String,
    /// Request parameters (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Request ID (can be string, number, or null)
    pub id: RequestId,
}

/// JSON-RPC request ID
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum RequestId {
    /// Numeric ID
    Number(i64),
    /// String ID
    String(String),
    /// Null ID (for notifications, though we don't use them)
    Null,
}

impl From<i64> for RequestId {
    fn from(n: i64) -> Self {
        RequestId::Number(n)
    }
}

impl From<String> for RequestId {
    fn from(s: String) -> Self {
        RequestId::String(s)
    }
}

impl From<&str> for RequestId {
    fn from(s: &str) -> Self {
        RequestId::String(s.to_string())
    }
}

/// A JSON-RPC 2.0 response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Must be "2.0"
    pub jsonrpc: String,
    /// Result (present on success)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error (present on failure)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
    /// Request ID that this response corresponds to
    pub id: RequestId,
}

impl Response {
    /// Create a successful response
    pub fn success(id: RequestId, result: Value) -> Self {
        Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response
    pub fn error(id: RequestId, error: ErrorObject) -> Self {
        Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }

    /// Create an error response from RpcError
    pub fn from_error(id: RequestId, err: RpcError) -> Self {
        Response::error(id, err.into())
    }
}

/// JSON-RPC 2.0 error object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorObject {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl From<RpcError> for ErrorObject {
    fn from(err: RpcError) -> Self {
        ErrorObject {
            code: err.code(),
            message: err.message(),
            data: err.data(),
        }
    }
}

/// Standard JSON-RPC 2.0 error codes
#[derive(Debug, Clone)]
pub enum RpcError {
    /// Invalid JSON was received
    ParseError(Option<String>),
    /// The JSON sent is not a valid Request object
    InvalidRequest(Option<String>),
    /// The method does not exist / is not available
    MethodNotFound(String),
    /// Invalid method parameter(s)
    InvalidParams(String),
    /// Internal JSON-RPC error
    InternalError(String),
    /// Application-specific error
    ApplicationError { code: i32, message: String, data: Option<Value> },
}

impl RpcError {
    /// Get the error code
    pub fn code(&self) -> i32 {
        match self {
            RpcError::ParseError(_) => -32700,
            RpcError::InvalidRequest(_) => -32600,
            RpcError::MethodNotFound(_) => -32601,
            RpcError::InvalidParams(_) => -32602,
            RpcError::InternalError(_) => -32603,
            RpcError::ApplicationError { code, .. } => *code,
        }
    }

    /// Get the error message
    pub fn message(&self) -> String {
        match self {
            RpcError::ParseError(Some(msg)) => format!("Parse error: {}", msg),
            RpcError::ParseError(None) => "Parse error".to_string(),
            RpcError::InvalidRequest(Some(msg)) => format!("Invalid Request: {}", msg),
            RpcError::InvalidRequest(None) => "Invalid Request".to_string(),
            RpcError::MethodNotFound(method) => format!("Method not found: {}", method),
            RpcError::InvalidParams(msg) => format!("Invalid params: {}", msg),
            RpcError::InternalError(msg) => format!("Internal error: {}", msg),
            RpcError::ApplicationError { message, .. } => message.clone(),
        }
    }

    /// Get the optional error data
    pub fn data(&self) -> Option<Value> {
        match self {
            RpcError::ApplicationError { data, .. } => data.clone(),
            _ => None,
        }
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for RpcError {}

/// Parse a JSON-RPC request from bytes
pub fn parse_request(data: &[u8]) -> Result<Request, RpcError> {
    let text = std::str::from_utf8(data)
        .map_err(|e| RpcError::ParseError(Some(e.to_string())))?;
    parse_request_str(text)
}

/// Parse a JSON-RPC request from a string
pub fn parse_request_str(text: &str) -> Result<Request, RpcError> {
    let request: Request = serde_json::from_str(text)
        .map_err(|e| RpcError::ParseError(Some(e.to_string())))?;

    // Validate JSON-RPC version
    if request.jsonrpc != JSONRPC_VERSION {
        return Err(RpcError::InvalidRequest(Some(format!(
            "Expected jsonrpc version '{}', got '{}'",
            JSONRPC_VERSION, request.jsonrpc
        ))));
    }

    Ok(request)
}

/// Serialize a response to JSON bytes
pub fn serialize_response(response: &Response) -> Result<Vec<u8>, RpcError> {
    serde_json::to_vec(response)
        .map_err(|e| RpcError::InternalError(e.to_string()))
}

/// Serialize a response to a JSON string
pub fn serialize_response_string(response: &Response) -> Result<String, RpcError> {
    serde_json::to_string(response)
        .map_err(|e| RpcError::InternalError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_request() {
        let json = r#"{"jsonrpc":"2.0","method":"test","id":1}"#;
        let req = parse_request_str(json).unwrap();
        assert_eq!(req.method, "test");
        assert_eq!(req.id, RequestId::Number(1));
    }

    #[test]
    fn test_parse_request_with_params() {
        let json = r#"{"jsonrpc":"2.0","method":"test","params":{"key":"value"},"id":"abc"}"#;
        let req = parse_request_str(json).unwrap();
        assert_eq!(req.method, "test");
        assert_eq!(req.id, RequestId::String("abc".to_string()));
        assert!(req.params.is_some());
    }

    #[test]
    fn test_response_success() {
        let resp = Response::success(RequestId::Number(1), serde_json::json!({"result": "ok"}));
        let json = serialize_response_string(&resp).unwrap();
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_response_error() {
        let resp = Response::from_error(
            RequestId::Number(1),
            RpcError::MethodNotFound("unknown".to_string()),
        );
        let json = serialize_response_string(&resp).unwrap();
        assert!(json.contains("\"error\""));
        assert!(json.contains("-32601"));
    }
}
