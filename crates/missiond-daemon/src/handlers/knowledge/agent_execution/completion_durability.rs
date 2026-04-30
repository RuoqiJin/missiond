use serde_json::{json, Value};
use std::collections::HashMap;

use super::completion_fields::VALID_COMMIT_STATUSES;
use super::completion_records::CompletionRecord;

/// Build the dashboard-friendly `durability` projection over a slice of
/// `CompletionRecord`s. The shape stays stable across legacy + new
/// companion logs: when no completion carries scoped-commit metadata the
/// summary still surfaces zero counts plus `latest_commit_status: null`
/// so consumers do not need to special-case "old log".
pub(super) fn summarize_durability(records: &[CompletionRecord]) -> Value {
    let total = records.len();
    let mut by_status: HashMap<&str, u32> = HashMap::new();
    let mut without_status = 0u32;
    let mut with_hash = 0u32;
    let mut blocked_with_blocker = 0u32;
    let mut blocked_without_blocker = 0u32;
    for r in records {
        match r.commit_status.as_deref() {
            Some(s) => {
                *by_status.entry(canonical_status_str(s)).or_insert(0) += 1;
                if s == "blocked" {
                    if r.commit_blocker.is_some() {
                        blocked_with_blocker += 1;
                    } else {
                        blocked_without_blocker += 1;
                    }
                }
            }
            None => without_status += 1,
        }
        if r.commit_hash.is_some() {
            with_hash += 1;
        }
    }
    let mut by_status_json = serde_json::Map::new();
    for &status in VALID_COMMIT_STATUSES {
        by_status_json.insert(
            status.to_string(),
            json!(*by_status.get(status).unwrap_or(&0)),
        );
    }
    let unknown_count = *by_status.get("unknown").unwrap_or(&0);
    if unknown_count > 0 {
        by_status_json.insert("unknown".to_string(), json!(unknown_count));
    }
    let latest_status = records.iter().rev().find_map(|r| r.commit_status.clone());
    let latest_hash = records.iter().rev().find_map(|r| r.commit_hash.clone());
    json!({
        "completion_count": total,
        "without_commit_status": without_status,
        "with_commit_hash": with_hash,
        "blocked_with_blocker": blocked_with_blocker,
        "blocked_without_blocker": blocked_without_blocker,
        "by_commit_status": Value::Object(by_status_json),
        "latest_commit_status": latest_status,
        "latest_commit_hash": latest_hash,
    })
}

/// Map a raw status string back to one of `VALID_COMMIT_STATUSES`. Returns
/// `"unknown"` for anything else so we never silently drop weird tokens out
/// of the rollup. Audit still emits a finding via the strict normalize path
/// at write-time, but the dashboard shape stays predictable.
fn canonical_status_str(raw: &str) -> &'static str {
    match raw.trim() {
        "not-required" => "not-required",
        "pending" => "pending",
        "committed" => "committed",
        "blocked" => "blocked",
        "skipped" => "skipped",
        _ => "unknown",
    }
}
