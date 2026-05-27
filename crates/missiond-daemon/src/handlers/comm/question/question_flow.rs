use anyhow::{anyhow, Result};
use missiond_core::event::events::{QuestionEvent, TaskEvent};
use missiond_core::types::{AddBoardTaskNoteInput, AgentQuestion, AgentQuestionStatus};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};

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
    let mut questions = state
        .store
        .list_agent_questions(status.as_deref(), target.as_deref(), limit)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    if revalidate_questions_before_display(state, &questions).await? {
        questions = state
            .store
            .list_agent_questions(status.as_deref(), target.as_deref(), limit)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
    }
    annotate_questions_before_display(state, &mut questions).await;
    Ok(ToolResult::json_pretty(&questions))
}

async fn handle_get(state: &AppState, args: Value) -> Result<ToolResult> {
    let QuestionIdArgs { id } = serde_json::from_value(args)?;
    let mut question = match state
        .store
        .get_agent_question(&id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(q) => q,
        None => return Ok(ToolResult::error("Question not found")),
    };
    if revalidate_questions_before_display(state, std::slice::from_ref(&question)).await? {
        question = match state
            .store
            .get_agent_question(&id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
        {
            Some(q) => q,
            None => return Ok(ToolResult::error("Question not found")),
        };
    }
    let mut questions = vec![question];
    annotate_questions_before_display(state, &mut questions).await;
    Ok(ToolResult::json_pretty(&questions[0]))
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

async fn revalidate_questions_before_display(
    state: &AppState,
    questions: &[AgentQuestion],
) -> Result<bool> {
    let mut changed = false;
    let mut lisp_code_sync_snapshot: Option<Value> = None;

    for question in questions {
        if !is_lisp_code_sync_stale_decision_candidate(question) {
            continue;
        }
        let snapshot = match &lisp_code_sync_snapshot {
            Some(snapshot) => snapshot.clone(),
            None => {
                let snapshot =
                    crate::engine::lisp_code_sync::status_snapshot_for_state(state).await;
                lisp_code_sync_snapshot = Some(snapshot.clone());
                snapshot
            }
        };
        if !lisp_code_sync_evidence_is_resolved(&snapshot) {
            continue;
        }
        let answer = lisp_code_sync_stale_answer(&snapshot);
        if state
            .store
            .answer_agent_question(&question.id, &answer)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
            .is_some()
        {
            close_stale_lisp_code_sync_linked_task(state, question, &answer).await;
            let _ = state
                .bus
                .publish_question(QuestionEvent::Resolved {
                    question_id: question.id.clone(),
                    resolution: "stale_evidence".to_string(),
                })
                .await;
            changed = true;
        }
    }

    Ok(changed)
}

async fn annotate_questions_before_display(state: &AppState, questions: &mut [AgentQuestion]) {
    let has_lisp_sync_candidate = questions
        .iter()
        .any(is_lisp_code_sync_stale_decision_candidate);
    if !has_lisp_sync_candidate {
        return;
    }
    let snapshot = crate::engine::lisp_code_sync::status_snapshot_for_state(state).await;
    let evidence_fresh_at = chrono::Utc::now().to_rfc3339();
    let resolved = lisp_code_sync_evidence_is_resolved(&snapshot);
    let reason = lisp_code_sync_revalidation_reason(&snapshot);
    for question in questions {
        if !is_lisp_code_sync_stale_decision_candidate(question) {
            continue;
        }
        question.evidence_fresh_at = Some(evidence_fresh_at.clone());
        question.revalidation_status = Some(if resolved {
            "stale_evidence".to_string()
        } else {
            "still_valid".to_string()
        });
        question.stale_reason = Some(reason.clone());
    }
}

async fn close_stale_lisp_code_sync_linked_task(
    state: &AppState,
    question: &AgentQuestion,
    answer: &str,
) {
    let Some(task_id) = question.task_id.as_deref() else {
        return;
    };
    let old_status = match state.store.get_board_task(task_id).await {
        Ok(Some(task)) => format!("{:?}", task.status),
        _ => "unknown".to_string(),
    };
    let note = format!(
        "resolved_by_runtime_fix / stale_evidence: linked decision {} was auto-answered during Decision Inbox revalidation before display.\n\n{}",
        question.id, answer
    );
    let _ = state
        .store
        .add_board_task_note(&AddBoardTaskNoteInput {
            task_id: task_id.to_string(),
            content: note.clone(),
            note_type: Some("summary".to_string()),
            author: Some("decision-inbox-revalidation".to_string()),
        })
        .await;
    if let Err(err) = crate::engine::control_plane_kernel::ControlPlaneKernel::new(state)
        .complete_system_task(
            crate::engine::control_plane_kernel::SystemTaskCompletionInput {
                task_id: task_id.to_string(),
                project_id: None,
                producer_id: "decision_inbox_revalidation".to_string(),
                summary: format!(
                    "Linked decision {} was resolved by runtime revalidation.",
                    question.id
                ),
                content: Some(note),
                raw_evidence: json!({
                    "kind": "decision_inbox_revalidation",
                    "question_id": question.id,
                    "answer": answer,
                    "old_status": old_status
                }),
                evidence_refs: vec![json!({
                    "kind": "agent_question",
                    "question_id": question.id
                })],
                result_status: "completed".to_string(),
                metadata: json!({
                    "question_id": question.id,
                    "source": "decision_inbox_revalidation"
                }),
            },
        )
        .await
    {
        warn!(task_id, error = %err, "decision revalidation failed to settle linked BoardTask");
    }
}

fn is_lisp_code_sync_stale_decision_candidate(question: &AgentQuestion) -> bool {
    if question.status != AgentQuestionStatus::Pending {
        return false;
    }
    if !question
        .question
        .to_ascii_lowercase()
        .contains("lisp-code-sync")
    {
        return false;
    }
    let haystack = format!(
        "{}\n{}\n{}",
        question.question, question.context, question.decision_type
    )
    .to_ascii_lowercase();
    haystack.contains("lisp-code-sync")
        && (haystack.contains("自循环")
            || haystack.contains("runtime/lisp-code-sync")
            || haystack.contains("report 风暴")
            || haystack.contains("report storm")
            || haystack.contains("storm"))
}

fn lisp_code_sync_evidence_is_resolved(snapshot: &Value) -> bool {
    let report_dirs = &snapshot["reportDirs"];
    let over_limit_empty = report_dirs["overLimitProjects"]
        .as_array()
        .map(|items| items.is_empty())
        .unwrap_or(false);
    let recent_sync_task_creations = snapshot["recentSyncTaskCreations"].as_u64().unwrap_or(0);
    let storm_circuit_hits = snapshot["stormCircuitHits"].as_u64().unwrap_or(0);

    // A deploy/restart can legitimately write fresh sync reports. The stale
    // decision is about a self-amplifying BoardTask storm, so require the task
    // creation and storm-circuit counters to be quiet rather than requiring
    // zero recent report files.
    over_limit_empty && recent_sync_task_creations == 0 && storm_circuit_hits == 0
}

fn lisp_code_sync_stale_answer(snapshot: &Value) -> String {
    let reason = lisp_code_sync_revalidation_reason(snapshot);
    format!(
        "stale_evidence/resolved_by_runtime_fix: lisp-code-sync self-loop evidence is no longer current. {reason}; pending operator decision was auto-resolved before display."
    )
}

fn lisp_code_sync_revalidation_reason(snapshot: &Value) -> String {
    let report_dirs = &snapshot["reportDirs"];
    let total = report_dirs["totalReports"].as_u64().unwrap_or(0);
    let recent = report_dirs["recentReports5m"].as_u64().unwrap_or(0);
    let max = report_dirs["maxReportsPerProject"].as_u64().unwrap_or(0);
    let recent_tasks = snapshot["recentSyncTaskCreations"].as_u64().unwrap_or(0);
    let storm_hits = snapshot["stormCircuitHits"].as_u64().unwrap_or(0);
    format!(
        "runtime report dirs: totalReports={total}, recentReports5m={recent}, maxReportsPerProject={max}; recentSyncTaskCreations={recent_tasks}, stormCircuitHits={storm_hits}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str, context: &str) -> AgentQuestion {
        AgentQuestion {
            id: "q-1".to_string(),
            task_id: None,
            slot_id: None,
            session_id: None,
            question: text.to_string(),
            context: context.to_string(),
            status: AgentQuestionStatus::Pending,
            answer: None,
            target: "user".to_string(),
            options: None,
            decision_type: "debug".to_string(),
            retry_count: 0,
            routing_trace: None,
            revalidation_status: None,
            stale_reason: None,
            evidence_fresh_at: None,
            created_at: "2026-05-09T00:00:00Z".to_string(),
            updated_at: "2026-05-09T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn stale_candidate_matches_lisp_code_sync_storm_question() {
        assert!(is_lisp_code_sync_stale_decision_candidate(&q(
            "lisp-code-sync 已进入自循环",
            "runtime/lisp-code-sync reports growing"
        )));
        assert!(!is_lisp_code_sync_stale_decision_candidate(&q(
            "unrelated deploy question",
            "runtime/lisp-code-sync reports growing"
        )));
    }

    #[test]
    fn resolved_lisp_code_sync_evidence_requires_no_task_storm_or_over_limit_reports() {
        let resolved = serde_json::json!({
            "recentSyncTaskCreations": 0,
            "stormCircuitHits": 0,
            "reportDirs": {
                "recentReports5m": 3,
                "overLimitProjects": [],
                "totalReports": 200,
                "maxReportsPerProject": 200
            }
        });
        assert!(lisp_code_sync_evidence_is_resolved(&resolved));

        let active = serde_json::json!({
            "recentSyncTaskCreations": 1,
            "stormCircuitHits": 0,
            "reportDirs": {
                "recentReports5m": 3,
                "overLimitProjects": [],
            }
        });
        assert!(!lisp_code_sync_evidence_is_resolved(&active));

        let over_limit = serde_json::json!({
            "recentSyncTaskCreations": 0,
            "stormCircuitHits": 0,
            "reportDirs": {
                "recentReports5m": 0,
                "overLimitProjects": ["missiond"],
            }
        });
        assert!(!lisp_code_sync_evidence_is_resolved(&over_limit));
    }
}
