use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::context_budget::format_tool_call_trace;
use crate::lenient;
use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tool: mission_audit
    if name == "mission_audit" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("trace");
        return match action {
            "trace" => handle_inner(state, "mission_audit_trace", args).await,
            "detail" => handle_inner(state, "mission_audit_detail", args).await,
            "stats" => handle_inner(state, "mission_audit_stats", args).await,
            "export" => handle_inner(state, "mission_audit_export", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    handle_inner(state, name, args).await
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Audit (Conversation Tool Call Analysis) =====
        "mission_audit_trace" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                session_id: String,
                tool_filter: Option<Vec<String>>,
                #[serde(default, deserialize_with = "lenient::option_bool")]
                include_reasoning: Option<bool>,
            }
            let Args {
                session_id,
                tool_filter,
                include_reasoning,
            } = serde_json::from_value(args)?;
            let conv = state
                .store
                .get_conversation(&session_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let calls = state
                .store
                .get_tool_calls_by_session(&session_id, tool_filter.as_deref(), 10000)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            // Build Markdown trace
            let mut md = String::new();
            let slot_id = conv
                .as_ref()
                .and_then(|c| c.slot_id.as_deref())
                .unwrap_or("N/A");
            let model = conv
                .as_ref()
                .and_then(|c| c.model.as_deref())
                .unwrap_or("N/A");
            let success_count = calls.iter().filter(|c| c.status == "success").count();
            let error_count = calls.iter().filter(|c| c.status == "error").count();
            let pending_count = calls.iter().filter(|c| c.status == "pending").count();

            md.push_str(&format!("# Audit Trace: {session_id}\n"));
            md.push_str(&format!("Slot: {slot_id} | Model: {model}\n"));
            md.push_str(&format!("Tool Calls: {} (Success: {success_count}, Error: {error_count}, Pending: {pending_count})\n\n---\n\n", calls.len()));

            // Optionally interleave reasoning from conversation_messages
            if include_reasoning.unwrap_or(false) {
                let msgs = state
                    .store
                    .get_conversation_messages(&session_id, None, 10000)
                    .await
                    .unwrap_or_default();
                // Build timeline: messages + tool calls sorted by timestamp
                let mut msg_idx = 0;
                for tc in &calls {
                    // Print assistant reasoning before this tool call
                    while msg_idx < msgs.len() && msgs[msg_idx].timestamp <= tc.timestamp {
                        let m = &msgs[msg_idx];
                        if m.role == "assistant"
                            && !m.content.starts_with("[Tool:")
                            && !m.content.starts_with("[thinking]")
                        {
                            let content = if m.content.len() > 200 {
                                format!(
                                    "{}...",
                                    &m.content[..m
                                        .content
                                        .char_indices()
                                        .nth(200)
                                        .map(|(i, _)| i)
                                        .unwrap_or(m.content.len())]
                                )
                            } else {
                                m.content.clone()
                            };
                            md.push_str(&format!(
                                "[{}] 💭 {}\n\n",
                                &tc.timestamp[11..19.min(tc.timestamp.len())],
                                content
                            ));
                        }
                        msg_idx += 1;
                    }
                    format_tool_call_trace(&mut md, tc);
                }
            } else {
                for tc in &calls {
                    format_tool_call_trace(&mut md, tc);
                }
            }

            if !calls.is_empty() {
                md.push_str(
                    "\n💡 Use mission_audit_detail(toolId: \"<id>\") for full I/O payload.\n",
                );
            }

            Ok(ToolResult::text(&md))
        }
        "mission_audit_detail" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                tool_id: String,
            }
            let Args { tool_id } = serde_json::from_value(args)?;
            let tc = state
                .store
                .get_tool_call_by_id(&tool_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?
                .ok_or_else(|| anyhow!("Tool call not found: {}", tool_id))?;

            // Parse raw_input/raw_output back to JSON for clean display
            let input: serde_json::Value = tc
                .raw_input
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::Value::Null);
            let output: serde_json::Value = tc
                .raw_output
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::Value::Null);

            Ok(ToolResult::json(&serde_json::json!({
                "id": tc.id,
                "toolName": tc.tool_name,
                "timestamp": tc.timestamp,
                "status": tc.status,
                "input": input,
                "output": output,
                "inputSummary": tc.input_summary,
                "outputSummary": tc.output_summary,
            })))
        }
        "mission_audit_stats" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                session_id: String,
            }
            let Args { session_id } = serde_json::from_value(args)?;
            let stats = state
                .store
                .get_tool_call_stats(&session_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            let mut by_tool = serde_json::Map::new();
            let mut total = 0i64;
            let mut total_success = 0i64;
            let mut total_error = 0i64;
            for (name, count, success, error) in &stats {
                by_tool.insert(
                    name.clone(),
                    serde_json::json!({
                        "count": count,
                        "success": success,
                        "error": error,
                    }),
                );
                total += count;
                total_success += success;
                total_error += error;
            }

            // Get first/last timestamps
            let calls = state
                .store
                .get_tool_calls_by_session(&session_id, None, 10000)
                .await
                .unwrap_or_default();
            let first_ts = calls.first().map(|c| c.timestamp.as_str()).unwrap_or("N/A");
            let last_ts = calls.last().map(|c| c.timestamp.as_str()).unwrap_or("N/A");

            Ok(ToolResult::json(&serde_json::json!({
                "sessionId": session_id,
                "totalCalls": total,
                "byTool": by_tool,
                "byStatus": {
                    "success": total_success,
                    "error": total_error,
                    "pending": total - total_success - total_error,
                },
                "firstCall": first_ts,
                "lastCall": last_ts,
            })))
        }

        "mission_audit_export" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                task_id: String,
                #[serde(default, deserialize_with = "lenient::option_bool")]
                include_messages: Option<bool>,
            }
            let Args {
                task_id,
                include_messages,
            } = serde_json::from_value(args)?;
            let include_msgs = include_messages.unwrap_or(true);

            // 1. Get board task with notes
            let task = state
                .store
                .get_board_task(&task_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?
                .ok_or_else(|| anyhow!("Task not found: {}", task_id))?;
            let notes = state
                .store
                .get_board_task_notes(&task_id)
                .await
                .unwrap_or_default();

            // 2. Find all conversations linked to this task
            let linked_convs = state
                .store
                .list_conversations(None, 100, Some("all"), Some(&task_id), None, None, None)
                .await
                .unwrap_or_default();

            // 3. Build export document
            let mut md = String::new();
            md.push_str(&format!("# Audit Export: {}\n\n", task.title));
            md.push_str(&format!("**Task ID**: `{}`\n", task.id));
            md.push_str(&format!(
                "**Status**: {} | **Priority**: {}\n",
                task.status.as_str(),
                task.priority
            ));
            md.push_str(&format!(
                "**Project**: {} | **Assignee**: {}\n",
                task.project.as_deref().unwrap_or("N/A"),
                task.assignee.as_deref().unwrap_or("N/A")
            ));
            md.push_str(&format!(
                "**Created**: {} | **Updated**: {}\n",
                task.created_at, task.updated_at
            ));
            if let Some(ref fp) = task.flow_phase {
                md.push_str(&format!(
                    "**Flow Phase**: {} | **Template**: {}\n",
                    fp,
                    task.flow_template.as_deref().unwrap_or("N/A")
                ));
            }
            md.push_str(&format!("\n## Description\n\n{}\n", task.description));

            // 4. FlowContext
            if let Some(ref fc_str) = task.flow_context {
                md.push_str("\n---\n\n## FlowContext\n\n```json\n");
                // Pretty-print the JSON
                if let Ok(fc_val) = serde_json::from_str::<serde_json::Value>(fc_str) {
                    md.push_str(
                        &serde_json::to_string_pretty(&fc_val).unwrap_or_else(|_| fc_str.clone()),
                    );
                } else {
                    md.push_str(fc_str);
                }
                md.push_str("\n```\n");
            }

            // 5. Board Notes
            if !notes.is_empty() {
                md.push_str("\n---\n\n## Board Notes\n\n");
                for note in &notes {
                    md.push_str(&format!(
                        "### [{}] `{}` ({}, {})\n\n",
                        &note.created_at[..19.min(note.created_at.len())],
                        &note.id[..8.min(note.id.len())],
                        note.author.as_deref().unwrap_or("unknown"),
                        note.note_type.as_str(),
                    ));
                    md.push_str(&note.content);
                    md.push_str("\n\n");
                }
            }

            // 6. Linked Conversations
            md.push_str("\n---\n\n## Linked Conversations\n\n");
            if linked_convs.is_empty() {
                md.push_str("(no conversations linked to this task)\n");
            } else {
                md.push_str(&format!(
                    "Found {} conversation(s):\n\n",
                    linked_convs.len()
                ));
                for conv in &linked_convs {
                    md.push_str(&format!("### Session `{}`\n\n", conv.id));
                    md.push_str(&format!(
                        "- **Type**: {} / {}\n",
                        conv.chat_type.as_deref().unwrap_or("N/A"),
                        conv.conversation_type
                    ));
                    md.push_str(&format!(
                        "- **Slot**: {}\n",
                        conv.slot_id.as_deref().unwrap_or("N/A")
                    ));
                    md.push_str(&format!(
                        "- **Model**: {}\n",
                        conv.model.as_deref().unwrap_or("N/A")
                    ));
                    md.push_str(&format!("- **Messages**: {}\n", conv.message_count));
                    md.push_str(&format!(
                        "- **Started**: {} | **Ended**: {}\n",
                        conv.started_at,
                        conv.ended_at.as_deref().unwrap_or("(active)")
                    ));

                    if include_msgs {
                        let msgs = state
                            .store
                            .get_conversation_messages(&conv.id, None, 500)
                            .await
                            .unwrap_or_default();
                        if !msgs.is_empty() {
                            md.push_str("\n#### Messages\n\n");
                            for m in &msgs {
                                let ts = &m.timestamp[11..19.min(m.timestamp.len())];
                                let content_preview = if m.content.len() > 500 {
                                    let end = m
                                        .content
                                        .char_indices()
                                        .nth(500)
                                        .map(|(i, _)| i)
                                        .unwrap_or(m.content.len());
                                    format!(
                                        "{}... ({} chars total)",
                                        &m.content[..end],
                                        m.content.len()
                                    )
                                } else {
                                    m.content.clone()
                                };
                                md.push_str(&format!(
                                    "**[{}] {}**{}\n\n{}\n\n",
                                    ts,
                                    m.role,
                                    m.model
                                        .as_deref()
                                        .map(|model| format!(" ({})", model))
                                        .unwrap_or_default(),
                                    content_preview,
                                ));
                            }
                        }
                    }

                    // Include child sessions (subagents)
                    let children = state
                        .store
                        .get_child_conversations(&conv.id)
                        .await
                        .unwrap_or_default();
                    if !children.is_empty() {
                        md.push_str(&format!("\n#### Subagents ({})\n\n", children.len()));
                        for child in &children {
                            md.push_str(&format!(
                                "- `{}` — {} msgs, {}\n",
                                child.id, child.message_count, child.status
                            ));
                        }
                    }
                    md.push_str("\n");
                }
            }

            Ok(ToolResult::text(&md))
        }

        _ => Err(anyhow!("Unknown audit tool: {name}")),
    }
}
