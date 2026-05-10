use super::*;

const COMPACT_NOTE_RESPONSE_THRESHOLD_BYTES: usize = 16_000;
const MAX_NOTE_CONTENT_BYTES: usize = 256_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoardNoteAddArgs {
    #[serde(alias = "task_id")]
    task_id: String,
    content: String,
    #[serde(default)]
    #[serde(alias = "note_type")]
    note_type: Option<String>,
    #[serde(default)]
    author: Option<String>,
}

pub(super) async fn handle_note_add(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: BoardNoteAddArgs = match serde_json::from_value(super::normalize_board_args(args)) {
        Ok(args) => args,
        Err(err) => return Ok(super::invalid_board_args("mission_board_note_add", err)),
    };
    if args.task_id.trim().is_empty() {
        return Ok(ToolResult::structured_error(
            missiond_mcp::tools::ToolError::new(
                missiond_mcp::tools::error_codes::INVALID_PARAM,
                "mission_board_note_add invalid arguments: taskId must be non-empty",
            )
            .with_suggestion("pass the BoardTask id as taskId or task_id"),
        ));
    }
    if args.content.trim().is_empty() {
        return Ok(ToolResult::structured_error(
            missiond_mcp::tools::ToolError::new(
                missiond_mcp::tools::error_codes::INVALID_PARAM,
                "mission_board_note_add invalid arguments: content must be non-empty",
            )
            .with_suggestion("write a concise summary note; large content is accepted and returns a compact receipt"),
        ));
    }
    if args.content.len() > MAX_NOTE_CONTENT_BYTES {
        return Ok(note_content_too_large_result(args.content.len()));
    }
    if let Some(note_type) = args.note_type.as_deref() {
        if missiond_core::types::BoardNoteType::from_str(note_type).is_none() {
            return Ok(ToolResult::structured_error(
                missiond_mcp::tools::ToolError::new(
                    missiond_mcp::tools::error_codes::INVALID_PARAM,
                    format!("mission_board_note_add invalid noteType: {note_type}"),
                )
                .with_suggestion("use noteType summary, progress, or note"),
            ));
        }
    }
    let task_id = args.task_id.clone();
    let content_preview: String = args.content.chars().take(80).collect();
    let is_master_control_note =
        crate::engine::master_control::is_master_control_note_author(args.author.as_deref());
    let input = missiond_core::types::AddBoardTaskNoteInput {
        task_id: args.task_id,
        content: args.content,
        note_type: args.note_type,
        author: args.author,
    };
    let note = match state.store.add_board_task_note(&input).await {
        Ok(note) => note,
        Err(err) => return Ok(super::board_store_error("mission_board_note_add", err)),
    };
    // Refresh binding: session is actively updating this task.
    if let Ok(Some(task)) = state.store.get_board_task(&task_id).await {
        super::record_session_task_binding(state, task.id.as_str(), &task.title);
    }
    let ev = BoardEvent::NoteAdded {
        task_id: task_id.clone(),
        note_id: note.id.clone(),
        content_preview: content_preview.clone(),
    };
    if !is_master_control_note {
        crate::engine::master_control::notify_board_event_direct(&ev);
    }
    let _ = state.bus.publish_board(ev).await;
    Ok(note_add_response(&note))
}

fn note_content_too_large_result(content_len: usize) -> ToolResult {
    ToolResult::structured_error(
        missiond_mcp::tools::ToolError::new(
            missiond_mcp::tools::error_codes::INVALID_PARAM,
            format!(
                "mission_board_note_add content too large: {content_len} bytes (max {MAX_NOTE_CONTENT_BYTES})"
            ),
        )
        .with_suggestion(
            "store the full artifact under .missiond/research or shared-memory artifact storage, then add a concise Board summary note with the artifact path",
        ),
    )
}

fn note_add_response(note: &missiond_core::types::BoardTaskNote) -> ToolResult {
    if note.content.len() <= COMPACT_NOTE_RESPONSE_THRESHOLD_BYTES {
        return ToolResult::json_pretty(note);
    }
    let preview: String = note.content.chars().take(500).collect();
    ToolResult::json_pretty(&serde_json::json!({
        "id": note.id,
        "taskId": note.task_id,
        "noteType": note.note_type.as_str(),
        "author": note.author,
        "createdAt": note.created_at,
        "contentLength": note.content.len(),
        "contentPreview": preview,
        "contentOmitted": true,
        "message": "note stored successfully; response omitted full content to keep MCP result compact"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use missiond_core::types::{BoardNoteType, BoardTaskNote};
    use missiond_mcp::tools::ToolContent;

    #[test]
    fn large_note_response_is_compact_receipt() {
        let note = BoardTaskNote {
            id: "note-1".to_string(),
            task_id: "task-1".to_string(),
            content: "x".repeat(COMPACT_NOTE_RESPONSE_THRESHOLD_BYTES + 1),
            note_type: BoardNoteType::Summary,
            author: Some("worker".to_string()),
            created_at: "2026-05-05T00:00:00Z".to_string(),
        };
        let response = note_add_response(&note);
        let ToolContent::Text { text } = &response.content[0];
        assert!(text.contains("\"contentOmitted\": true"));
        assert!(text.contains("\"contentLength\""));
        assert!(!text.contains(&"x".repeat(1000)));
    }

    #[test]
    fn oversized_note_returns_structured_error() {
        let response = note_content_too_large_result(MAX_NOTE_CONTENT_BYTES + 1);
        let ToolContent::Text { text } = &response.content[0];
        assert!(text.contains("content too large"));
        assert!(text.contains("shared-memory artifact storage"));
    }
}
