use std::sync::Arc;

use anyhow::{anyhow, Result};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::decision_harvest::harvest_decisions_for_task;
use crate::lenient;
use crate::llm::gemini_client::current_session_id;
use crate::state::{AppState, SessionTaskBinding};
use missiond_core::event::events::{BoardEvent, SlotEvent};

mod claim;
mod create;
mod decompose;
mod delete;
mod events;
mod note;
mod query;
mod retry;
mod session;
mod update;

use events::{publish_board_created, publish_board_status_changed, publish_board_update};
use session::record_session_task_binding;

pub(super) fn normalize_board_args(args: Value) -> Value {
    let Value::Object(mut map) = args else {
        return args;
    };
    copy_alias(&mut map, "taskId", &["task_id"]);
    copy_alias(&mut map, "noteType", &["note_type"]);
    copy_alias(
        &mut map,
        "parentId",
        &["parent_id", "parentTaskId", "parent_task_id"],
    );
    copy_alias(&mut map, "autoExecute", &["auto_execute"]);
    copy_alias(&mut map, "promptTemplate", &["prompt_template"]);
    copy_alias(&mut map, "flowTemplate", &["flow_template"]);
    copy_alias(&mut map, "dependsOn", &["depends_on"]);
    copy_alias(&mut map, "timeoutSecs", &["timeout_secs"]);
    copy_alias(&mut map, "contextIntent", &["context_intent"]);
    copy_alias(&mut map, "dueDate", &["due_date"]);
    copy_alias(&mut map, "order_idx", &["orderIdx"]);
    Value::Object(map)
}

fn copy_alias(map: &mut Map<String, Value>, canonical: &str, aliases: &[&str]) {
    if map.contains_key(canonical) {
        return;
    }
    for alias in aliases {
        if let Some(value) = map.get(*alias).cloned() {
            map.insert(canonical.to_string(), value);
            return;
        }
    }
}

pub(super) fn invalid_board_args(tool: &str, err: impl std::fmt::Display) -> ToolResult {
    ToolResult::structured_error(
        ToolError::new(
            error_codes::INVALID_PARAM,
            format!("{tool} invalid arguments: {err}"),
        )
        .with_suggestion(
            "use the documented board schema; common aliases such as task_id/taskId, note_type/noteType, parent_id/parentId, timeout_secs/timeoutSecs are accepted",
        ),
    )
}

pub(super) fn board_store_error(tool: &str, err: missiond_core::db::error::DbError) -> ToolResult {
    let (code, suggestion) = match &err {
        missiond_core::db::error::DbError::NotFound { .. } => (
            error_codes::NOT_FOUND,
            "verify the BoardTask id; short ids are accepted only when they resolve uniquely",
        ),
        missiond_core::db::error::DbError::Constraint(_) => (
            error_codes::INVALID_PARAM,
            "check field values and retry with a smaller, schema-valid payload",
        ),
        _ => (
            error_codes::DB_ERROR,
            "retry once; if it repeats, add a diagnostic BoardTask with the error_code and reason",
        ),
    };
    ToolResult::structured_error(
        ToolError::new(code, format!("{tool} store error: {err}")).with_suggestion(suggestion),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_board_args_accepts_snake_case_aliases() {
        let normalized = normalize_board_args(json!({
            "task_id": "task-1",
            "note_type": "summary",
            "parent_id": "parent-1",
            "auto_execute": true,
            "timeout_secs": 600,
            "context_intent": "code"
        }));
        assert_eq!(normalized["taskId"], "task-1");
        assert_eq!(normalized["noteType"], "summary");
        assert_eq!(normalized["parentId"], "parent-1");
        assert_eq!(normalized["autoExecute"], true);
        assert_eq!(normalized["timeoutSecs"], 600);
        assert_eq!(normalized["contextIntent"], "code");
    }
}

// @beacon: board
pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Consolidated Query =====
        "mission_board_query"
        | "mission_board_list"
        | "mission_board_get"
        | "mission_board_search"
        | "mission_board_summary" => query::handle_query(state, name, args).await,
        "mission_board_create" => create::handle_create(state, args).await,
        // ===== Unified Update (single + batch, absorbs toggle) =====
        "mission_board_update" | "mission_board_batch_update" | "mission_board_toggle" => {
            update::handle_update(state, name, args).await
        }
        "mission_board_delete" => delete::handle_delete(state, args).await,
        "mission_board_claim" => claim::handle_claim(state, args).await,
        "mission_board_note_add" => note::handle_note_add(state, args).await,
        "mission_board_decompose" => decompose::handle_decompose(state, args).await,
        "mission_board_retry" => retry::handle_retry(state, args).await,
        _ => Err(anyhow!("Unknown board tool: {name}")),
    }
}
