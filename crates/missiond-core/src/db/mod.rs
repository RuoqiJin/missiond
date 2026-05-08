//! Database operations for missiond.

pub mod conversation_query;
pub(crate) mod directive;
pub mod error;
pub(crate) mod observability;
pub(crate) mod project;
pub mod shared;
pub mod traits;

#[cfg(feature = "postgres")]
pub mod pg;

// Re-exports from shared (always available)
pub use shared::{BackfillPhaseStatus, LatencyStats, TimelineRow, TimelineStats};

/// Extract parent session ID from a subagent's jsonl_path.
/// Path pattern: .../PARENT_SESSION_ID/subagents/agent-xxx.jsonl
pub fn extract_parent_session_id(jsonl_path: &str) -> Option<String> {
    let path = std::path::Path::new(jsonl_path);
    let parent = path.parent()?; // .../PARENT_SESSION_ID/subagents/
    if parent.file_name()?.to_str()? != "subagents" {
        return None;
    }
    let grandparent = parent.parent()?; // .../PARENT_SESSION_ID/
    let session_id = grandparent.file_name()?.to_str()?.to_string();
    // Sanity: parent session ID should look like a UUID
    if session_id.contains('-') && session_id.len() > 8 {
        Some(session_id)
    } else {
        None
    }
}

/// Derive conversation_type from slot_id and session_id.
/// "meta" = memory slots, "compaction" = context compaction shards,
/// "subagent" = agent-* IDs, "worker" = other slots, "user" = direct CLI.
pub fn derive_conversation_type(
    slot_category: Option<&str>,
    slot_id: Option<&str>,
    session_id: &str,
    source: &str,
) -> String {
    // Top-level interception for all Gemini family conversations
    // Regardless of whether they have a slot or not, they belong in the Gemini tab.
    if source == "gemini_cli" || source == "router_chat" {
        return "gemini_chat".to_string();
    }

    // 1. If we have a declarative category from the slot config, use it directly (Dynamic Routing)
    if let Some(cat) = slot_category {
        return cat.to_string();
    }

    // 2. Fallback heuristics for unmanaged/orphan sessions
    match slot_id {
        Some(_) => "worker".to_string(), // Fallback if slot_id is present but config is missing
        None if session_id.contains("-acompact-") => "compaction".to_string(),
        None if session_id.starts_with("agent-") => "subagent".to_string(),
        None => "user".to_string(),
    }
}
