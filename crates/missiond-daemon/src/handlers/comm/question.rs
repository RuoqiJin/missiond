use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;
use tracing::info;

use crate::state::AppState;
use missiond_core::event::events::{QuestionEvent, TaskEvent};

#[derive(Deserialize)]
struct QuestionCreateArgs {
    question: String,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    slot_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    /// Decision target: "user" or "master"
    #[serde(default)]
    target: Option<String>,
    /// Structured options/choices
    #[serde(default)]
    options: Option<String>,
    /// Decision type: architecture/implementation/debug/investigation/risk/preference
    #[serde(default)]
    decision_type: Option<String>,
}

#[derive(Deserialize)]
struct QuestionListArgs {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct QuestionAnswerArgs {
    id: String,
    answer: String,
}

#[derive(Deserialize)]
struct QuestionIdArgs {
    id: String,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tools
    if name == "mission_question" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        return match action {
            "create" => handle_inner(state, "mission_question_create", args).await,
            "list" => handle_inner(state, "mission_question_list", args).await,
            "get" => handle_inner(state, "mission_question_get", args).await,
            "answer" => handle_inner(state, "mission_question_answer", args).await,
            "dismiss" => handle_inner(state, "mission_question_dismiss", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    if name == "mission_llm_trace" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("gemini_trace");
        return match action {
            "gemini_trace" => {
                crate::handlers::misc::handle(state, "mission_gemini_trace", args).await
            }
            "gemini_stats" => {
                crate::handlers::misc::handle(state, "mission_gemini_stats", args).await
            }
            "gemini_watch" => {
                // Map watch_action to the action field expected by the handler
                let mut args = args;
                if let Some(wa) = args.get("watch_action").cloned() {
                    args.as_object_mut()
                        .map(|m| m.insert("action".to_string(), wa));
                }
                crate::handlers::misc::handle(state, "mission_gemini_watch", args).await
            }
            // TODO: DEPRECATED — use independent mission_gemini_auth tool instead.
            // Kept for Claude Code MCP client compatibility (doesn't auto-discover new tool names).
            "gemini_auth" => {
                crate::handlers::misc::handle(state, "mission_gemini_auth", args).await
            }
            "jarvis_logs" => {
                crate::handlers::misc::handle(state, "mission_jarvis_logs", args).await
            }
            "jarvis_trace" => {
                crate::handlers::misc::handle(state, "mission_jarvis_trace", args).await
            }
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    if name == "mission_gemini_auth" {
        return crate::handlers::misc::handle(state, "mission_gemini_auth", args).await;
    }
    if name == "mission_incident" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        return match action {
            "test" => crate::handlers::misc::handle(state, "mission_incident_test", args).await,
            "list" => crate::handlers::misc::handle(state, "mission_incident_list", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    handle_inner(state, name, args).await
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Agent Questions (Pending Decisions) =====
        "mission_question_create" => {
            let args: QuestionCreateArgs = serde_json::from_value(args)?;
            // Best-effort context injection: if task_id is missing, try to infer
            // from running autopilot tasks (single running task → unambiguous)
            let inferred_task_id = if args.task_id.is_none() {
                match state.store.list_running_autopilot_tasks().await {
                    Ok(running) if running.len() == 1 => {
                        let tid = running[0].id.to_string();
                        let sid = running[0].claim_executor_id.clone();
                        info!(inferred_task_id = %tid, "question_create: auto-injected task_id from running autopilot task");
                        Some((Some(tid), sid))
                    }
                    _ => None,
                }
            } else {
                None
            };
            let (task_id, slot_id) = if let Some((tid, sid)) = inferred_task_id {
                (tid, args.slot_id.or(sid))
            } else {
                (args.task_id, args.slot_id)
            };
            let input = missiond_core::types::CreateAgentQuestionInput {
                question: args.question,
                context: args.context,
                task_id,
                slot_id,
                session_id: args.session_id,
                target: args.target,
                options: args.options,
                decision_type: args.decision_type,
            };
            let q = state
                .store
                .create_agent_question(&input)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            // Signal Decision Engine if target=master
            if q.target == "master" {
                let _ = state
                    .bus
                    .publish_question(QuestionEvent::Created {
                        question_id: q.id.clone(),
                    })
                    .await;
                info!(question_id = %q.id, "Decision Engine notified: new master question");
            }
            Ok(ToolResult::json_pretty(&q))
        }
        "mission_question_list" => {
            let QuestionListArgs {
                status,
                target,
                limit,
            } = serde_json::from_value(args).unwrap_or(QuestionListArgs {
                status: None,
                target: None,
                limit: None,
            });
            let questions = state
                .store
                .list_agent_questions(status.as_deref(), target.as_deref(), limit)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&questions))
        }
        "mission_question_get" => {
            let QuestionIdArgs { id } = serde_json::from_value(args)?;
            match state
                .store
                .get_agent_question(&id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?
            {
                Some(q) => Ok(ToolResult::json_pretty(&q)),
                None => Ok(ToolResult::error("Question not found")),
            }
        }
        "mission_question_answer" => {
            let QuestionAnswerArgs { id, answer } = serde_json::from_value(args)?;
            match state
                .store
                .answer_agent_question(&id, &answer)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?
            {
                Some(q) => {
                    // Signal scheduler for instant slot recovery after question answered
                    let _ = state
                        .bus
                        .publish_task(TaskEvent::Completed {
                            task_id: String::new(),
                        })
                        .await;
                    let _ = state
                        .bus
                        .publish_question(QuestionEvent::Resolved {
                            question_id: id.clone(),
                            resolution: "answered".to_string(),
                        })
                        .await;
                    Ok(ToolResult::json_pretty(&q))
                }
                None => Ok(ToolResult::error("Question not found")),
            }
        }
        "mission_question_dismiss" => {
            let QuestionIdArgs { id } = serde_json::from_value(args)?;
            match state
                .store
                .dismiss_agent_question(&id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?
            {
                Some(q) => {
                    let _ = state
                        .bus
                        .publish_question(QuestionEvent::Resolved {
                            question_id: id.clone(),
                            resolution: "dismissed".to_string(),
                        })
                        .await;
                    Ok(ToolResult::json_pretty(&q))
                }
                None => Ok(ToolResult::error("Question not found")),
            }
        }

        "mission_decision_stats" => {
            let hours = args.get("hours").and_then(|v| v.as_i64()).unwrap_or(24);
            let stats = state
                .store
                .decision_stats(hours)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&stats))
        }

        // ── AIOps Incidents ──
        _ => Err(anyhow!("Unknown question tool: {name}")),
    }
}
