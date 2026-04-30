use chrono::{DateTime, Utc};

use super::lisp_syntax::Node;
use super::log_store::{parse_kv_pairs, LogFile};

#[derive(Debug, Clone)]
pub(super) struct ClaimRecord {
    pub(super) id: String,
    pub(super) claimer: String,
    pub(super) scope: String,
    pub(super) phase: Option<String>,
    pub(super) lease_expires_at: Option<String>,
    pub(super) heartbeat_at: Option<String>,
    pub(super) status: String,
}

pub(super) fn parse_claims(file: &LogFile) -> Vec<ClaimRecord> {
    let block = match file.find_block("claims") {
        Some(b) => b,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for child in block.children().iter().skip(1) {
        let head = child.head_atom().unwrap_or("");
        let kvs = parse_kv_pairs(&file.src, child.children());
        // Two flavors: head is the id, or `:id <ID>` is inline.
        let id = if head.starts_with(['C', 'c'])
            && head.len() > 1
            && head[1..].chars().all(|c| c.is_ascii_digit())
        {
            head.to_string()
        } else if let Some(v) = kvs.get("id").or_else(|| kvs.get("claim-id")).cloned() {
            v.trim().to_string()
        } else {
            // Legacy unnumbered claim - keep but with synthetic id.
            format!("claim@{}", child.start)
        };
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| {
                if kvs.get("released-at").is_some() {
                    "released".to_string()
                } else {
                    "active".to_string()
                }
            });
        out.push(ClaimRecord {
            id,
            claimer: kvs
                .get("claimer")
                .or_else(|| kvs.get("agent"))
                .cloned()
                .unwrap_or_default(),
            scope: kvs.get("scope").cloned().unwrap_or_default(),
            phase: kvs.get("phase").cloned(),
            lease_expires_at: kvs.get("lease-expires-at").cloned(),
            heartbeat_at: kvs.get("heartbeat-at").cloned(),
            status,
        });
    }
    out
}

pub(super) fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    let t = s.trim().trim_matches('"');
    DateTime::parse_from_rfc3339(t)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

pub(super) fn find_claim_node<'a>(file: &'a LogFile, claim_id: &str) -> Option<&'a Node> {
    let block = file.find_block("claims")?;
    for child in block.children().iter().skip(1) {
        if child.head_atom() == Some(claim_id) {
            return Some(child);
        }
        let kvs = parse_kv_pairs(&file.src, child.children());
        if let Some(id) = kvs.get("id").or_else(|| kvs.get("claim-id")) {
            if id.trim().trim_matches('"') == claim_id {
                return Some(child);
            }
        }
    }
    None
}
