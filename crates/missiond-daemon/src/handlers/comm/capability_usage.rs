//! Thin V3 facade for the capability-governance read model.
//!
//! The runtime module owns the existing capability usage snapshot/report,
//! review sidecar, semantic hint, and observability behaviour. Keeping this
//! facade stable lets the V3 implementation map point at the public surface
//! without changing the established governance semantics.

use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

mod runtime;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    runtime::handle(state, name, args).await
}
