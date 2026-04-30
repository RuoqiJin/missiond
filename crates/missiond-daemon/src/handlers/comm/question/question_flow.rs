use anyhow::{anyhow, Result};
use missiond_core::event::events::{QuestionEvent, TaskEvent};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;
use tracing::info;

use crate::state::AppState;

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
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    options: Option<String>,
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

pub(super) async fn handle_consolidated(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("list");
    match action {
        "create" => handle_legacy(state, "mission_question_create", args).await,
        "list" => handle_legacy(state, "mission_question_list", args).await,
        "get" => handle_legacy(state, "mission_question_get", args).await,
        "answer" => handle_legacy(state, "mission_question_answer", args).await,
        "dismiss" => handle_legacy(state, "mission_question_dismiss", args).await,
        _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
    }
}

pub(super) async fn handle_legacy(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_question_create" => handle_create(state, args).await,
        "mission_question_list" => handle_list(state, args).await,
        "mission_question_get" => handle_get(state, args).await,
        "mission_question_answer" => handle_answer(state, args).await,
        "mission_question_dismiss" => handle_dismiss(state, args).await,
        _ => Err(anyhow!("Unknown question tool: {name}")),
    }
}

async fn handle_create(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: QuestionCreateArgs = serde_json::from_value(args)?;
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

async fn handle_list(state: &AppState, args: Value) -> Result<ToolResult> {
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

async fn handle_get(state: &AppState, args: Value) -> Result<ToolResult> {
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

async fn handle_answer(state: &AppState, args: Value) -> Result<ToolResult> {
    let QuestionAnswerArgs { id, answer } = serde_json::from_value(args)?;
    match state
        .store
        .answer_agent_question(&id, &answer)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(q) => {
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

async fn handle_dismiss(state: &AppState, args: Value) -> Result<ToolResult> {
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
