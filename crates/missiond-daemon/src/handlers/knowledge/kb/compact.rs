use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use tracing::info;

use crate::state::AppState;

/// Programmatic KB compaction: rule-based cleanup beyond auto_gc.
/// dry_run=true (default) previews what would be deleted.
pub(super) async fn handle_kb_compact(
    state: &AppState,
    args: serde_json::Value,
) -> Result<ToolResult> {
    let dry_run = args.get("dryRun").and_then(|v| v.as_bool()).unwrap_or(true);
    // Load all entries for rule-based filtering
    let all = state.store.kb_list(None).await?;
    let now = chrono::Utc::now();
    let mut candidates: Vec<(String, String, String, f64, &str)> = Vec::new(); // (key, category, summary, confidence, reason)

    for e in &all {
        let age_days = chrono::DateTime::parse_from_rfc3339(&e.updated_at)
            .map(|t| (now - t.with_timezone(&chrono::Utc)).num_days())
            .unwrap_or(0);

        // Exempt categories: architecture:summary, policy:decision, preference — never auto-compact
        let exempt = e.category.starts_with("architecture:summary")
            || e.category.starts_with("policy:decision")
            || e.category.starts_with("preference")
            || e.category == "infra";
        if exempt {
            continue;
        }

        // Rule 1: Low confidence (< 0.3) — feedback loop has deprioritized
        if e.confidence < 0.3 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "low_confidence",
            ));
            continue;
        }
        // Rule 2: State-type entries older than 30d with 0 access
        if e.kb_type == "state" && e.access_count == 0 && age_days > 30 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "stale_state",
            ));
            continue;
        }
        // Rule 3: memory:ops older than 7 days
        if e.category.starts_with("memory:ops") && age_days > 7 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "stale_ops",
            ));
            continue;
        }
        // Rule 4: memory:debug older than 30 days
        if e.category.starts_with("memory:debug") && age_days > 30 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "stale_debug",
            ));
            continue;
        }
        // Rule 5: memory:bugfix older than 30 days with no retrieval
        if e.category.starts_with("memory:bugfix") && e.access_count == 0 && age_days > 30 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "stale_bugfix",
            ));
            continue;
        }
        // Rule 6: Low-value facts — confidence < 0.5 and never accessed
        if e.kb_type == "fact" && e.confidence < 0.5 && e.access_count == 0 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "low_value_fact",
            ));
            continue;
        }
        // Rule 7: Expired scratchpad — Working Memory entries older than 7 days
        if e.scope_task_id.is_some() && age_days > 7 {
            candidates.push((
                e.key.clone(),
                e.category.clone(),
                e.summary.clone(),
                e.confidence,
                "expired_scratchpad",
            ));
            continue;
        }
    }

    let total = candidates.len();

    if dry_run {
        let mut by_reason: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for (_, _, _, _, reason) in &candidates {
            *by_reason.entry(reason).or_default() += 1;
        }
        let preview: Vec<_> = candidates
            .iter()
            .take(50)
            .map(|(key, cat, summary, conf, reason)| {
                serde_json::json!({
                    "key": key, "category": cat, "summary": summary,
                    "confidence": conf, "reason": reason
                })
            })
            .collect();
        Ok(ToolResult::json_pretty(&serde_json::json!({
            "dryRun": true,
            "totalEntries": all.len(),
            "totalCandidates": total,
            "byReason": by_reason,
            "candidates": preview,
            "hint": "Set dryRun=false to execute deletion."
        })))
    } else {
        let keys: Vec<String> = candidates.iter().map(|(k, _, _, _, _)| k.clone()).collect();
        let deleted = state.store.kb_batch_forget(&keys).await?;
        info!(deleted, total, "KB compact: cleaned up entries");
        Ok(ToolResult::json(&serde_json::json!({
            "dryRun": false,
            "deleted": deleted,
            "total": total
        })))
    }
}
