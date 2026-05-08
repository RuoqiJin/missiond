use anyhow::{anyhow, Result};
use missiond_core::types::KnowledgeReviewInput;
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};

use crate::state::AppState;

use super::args::KBReviewArgs;

const VALID_REVIEW_STATES: &[&str] = &[
    "active",
    "superseded-by-lisp",
    "superseded-by-code",
    "historical-evidence",
    "duplicate",
    "wrong-or-stale",
    "delete-candidate",
    "needs-human",
];

fn validate_state(state: &str) -> Result<()> {
    if VALID_REVIEW_STATES.contains(&state) {
        Ok(())
    } else {
        Err(anyhow!(
            "invalid review state: {} (allowed: {})",
            state,
            VALID_REVIEW_STATES.join(", ")
        ))
    }
}

pub(super) async fn handle_kb_review(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: KBReviewArgs = serde_json::from_value(args)?;
    match args.action.as_str() {
        "upsert" => handle_review_upsert(state, args).await,
        "get" => handle_review_get(state, args).await,
        "stats" => handle_review_stats(state).await,
        other => Ok(ToolResult::error(format!(
            "unknown mission_kb_review action: {other}"
        ))),
    }
}

async fn resolve_knowledge_id(state: &AppState, args: &KBReviewArgs) -> Result<String> {
    if let Some(id) = args.knowledge_id.as_ref().filter(|s| !s.is_empty()) {
        return Ok(id.clone());
    }
    if let Some(key) = args.key.as_ref().filter(|s| !s.is_empty()) {
        return state
            .store
            .kb_get_id_by_key(key)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
            .ok_or_else(|| anyhow!("knowledge key not found: {key}"));
    }
    Err(anyhow!("review action requires key or knowledge_id"))
}

async fn handle_review_upsert(state: &AppState, args: KBReviewArgs) -> Result<ToolResult> {
    let knowledge_id = resolve_knowledge_id(state, &args).await?;
    let review_state = args
        .state
        .clone()
        .ok_or_else(|| anyhow!("review upsert requires state"))?;
    validate_state(&review_state)?;

    let confidence = args.confidence.unwrap_or(0.8);
    if !(0.0..=1.0).contains(&confidence) {
        return Err(anyhow!("confidence must be in [0.0, 1.0]"));
    }

    let input = KnowledgeReviewInput {
        knowledge_id,
        state: review_state,
        batch_id: args.batch_id.unwrap_or_else(|| "manual-review".to_string()),
        reviewer: args.reviewer.unwrap_or_else(|| "missiond".to_string()),
        rationale: args
            .rationale
            .ok_or_else(|| anyhow!("review upsert requires rationale"))?,
        evidence_refs: args.evidence_refs.unwrap_or_else(|| json!([])),
        superseded_by: args.superseded_by,
        confidence,
        applied_at: args.applied_at,
    };

    let row = state
        .store
        .kb_review_upsert(&input)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "ok": true,
        "review": row,
        "non_destructive": true,
    })))
}

async fn handle_review_get(state: &AppState, args: KBReviewArgs) -> Result<ToolResult> {
    if let Some(key) = args.key.as_ref().filter(|s| !s.is_empty()) {
        let review = state
            .store
            .kb_review_get_by_key(key)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        return Ok(ToolResult::json_pretty(&json!({
            "key": key,
            "review": review,
        })));
    }

    let knowledge_id = resolve_knowledge_id(state, &args).await?;
    let reviews = state
        .store
        .kb_review_current_for_ids(&[knowledge_id.clone()])
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "knowledge_id": knowledge_id,
        "review": reviews.get(&knowledge_id),
    })))
}

async fn handle_review_stats(state: &AppState) -> Result<ToolResult> {
    let stats = state
        .store
        .kb_review_stats()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&stats))
}
