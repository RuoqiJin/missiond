//! Decision Harvest — extracts generalizable rules from completed Board tasks.
//!
//! After a task with master-target decisions completes, this module calls Gemini
//! to generalize the Q&A into reusable policy:decision KB entries.

use tracing::{debug, info, warn};

use crate::state::AppState;
use crate::llm_gateway::call_gemini_for_flow;
use crate::prompts;

/// Decision Engine Phase 4: Harvest successful decisions into policy:decision KB entries.
/// Triggered when a Board task is marked done — scans answered master questions,
/// calls Gemini to generalize into reusable rules, writes to KB.
pub(crate) async fn harvest_decisions_for_task(state: &AppState, task_id: &str, task_title: &str) {
    // 1. Scan answered master questions for this task
    let questions = match state.mission.db().list_questions_for_task(task_id) {
        Ok(qs) => qs,
        Err(e) => {
            warn!(task_id, error = %e, "Decision harvester: failed to query questions");
            return;
        }
    };

    // Filter to only master-targeted questions (decision engine handled)
    let master_questions: Vec<_> = questions.iter()
        .filter(|q| q.target == "master" && q.answer.is_some())
        .collect();

    if master_questions.is_empty() {
        debug!(task_id, "Decision harvester: no master decisions to harvest");
        return;
    }

    info!(task_id, count = master_questions.len(), "Decision harvester: harvesting decisions");

    // 2. Build Q&A summary for Gemini
    let qa_text: String = master_questions.iter()
        .map(|q| {
            let dt = &q.decision_type;
            let answer = q.answer.as_deref().unwrap_or("");
            format!("- [{}] Q: {}\n  A: {}", dt, q.question, answer)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // 3. Call Gemini to generalize (with Few-Shot examples for quality)
    let prompt = prompts::decision::HARVEST_TEMPLATE
        .replace("{0}", task_title)
        .replace("{1}", &qa_text);

    let gemini_response = match call_gemini_for_flow(state, task_id, &prompt).await {
        Ok(r) => r,
        Err(e) => {
            warn!(task_id, error = %e, "Decision harvester: Gemini call failed");
            return;
        }
    };

    // 4. Parse Gemini response — extract JSON array robustly
    // Gemini may wrap JSON in markdown fences or natural language text
    let json_text = if let Some(start) = gemini_response.find('[') {
        if let Some(end) = gemini_response.rfind(']') {
            &gemini_response[start..=end]
        } else {
            gemini_response.trim()
        }
    } else {
        gemini_response.trim()
    };

    let rules: Vec<serde_json::Value> = match serde_json::from_str(json_text) {
        Ok(r) => r,
        Err(e) => {
            warn!(task_id, error = %e, resp = %json_text, "Decision harvester: failed to parse Gemini JSON");
            return;
        }
    };

    let mut written = 0;
    for rule in &rules {
        let key = rule.get("key").and_then(|v| v.as_str()).unwrap_or_default();
        let summary = rule.get("summary").and_then(|v| v.as_str()).unwrap_or_default();
        let detail = rule.get("detail");

        if key.is_empty() || summary.is_empty() {
            continue;
        }

        let input = missiond_core::types::KBRememberInput {
            category: "policy:decision".to_string(),
            key: key.to_string(),
            summary: summary.to_string(),
            detail: detail.cloned(),
            source: Some("decision-harvester".to_string()),
            confidence: Some(0.8),
        };

        match state.mission.db().kb_remember(&input) {
            Ok(result) => {
                info!(key, action = %result.action, "Decision harvester: wrote policy:decision");
                written += 1;
            }
            Err(e) => {
                warn!(key, error = %e, "Decision harvester: kb_remember failed");
            }
        }
    }

    if written > 0 {
        // Write a board note about the harvest
        let _ = state.mission.db().add_board_task_note(
            &missiond_core::types::AddBoardTaskNoteInput {
                task_id: task_id.to_string(),
                content: format!("🧠 [决策引擎] 从 {} 条主控决策中提炼了 {} 条 policy:decision 规则，已写入肌肉记忆。",
                    master_questions.len(), written),
                note_type: Some("progress".to_string()),
                author: Some("decision-harvester".to_string()),
            },
        );
    }
}
