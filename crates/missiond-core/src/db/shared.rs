//! Shared types and utilities used by both SQLite and PostgreSQL backends.
//! This module is always compiled regardless of feature flags.

use std::collections::HashSet;

// ── Timeline types ──

pub struct TimelineRow {
    pub seq: i64,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub parent_span_id: Option<String>,
    pub event_type: String,
    pub summary: Option<String>,
    pub payload: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TimelineStats {
    pub total_events: i64,
    pub by_type: Vec<(String, i64)>,
    pub traced_events: i64,
    pub unique_traces: i64,
    pub gemini_latency: Option<LatencyStats>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LatencyStats {
    pub count: i64,
    pub avg_ms: i64,
    pub p50_ms: i64,
    pub p90_ms: i64,
    pub p99_ms: i64,
}

// ── Backfill types ──

#[derive(Debug, Clone, serde::Serialize)]
pub struct BackfillPhaseStatus {
    pub phase: String,
    pub status: String,
    pub last_cursor: i64,
    pub total_estimated: i64,
    pub processed: i64,
    pub failed: i64,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MessageRoleBackfillCandidate {
    pub message_id: i64,
    pub session_id: String,
    pub conversation_type: String,
    pub slot_id: Option<String>,
    pub task_id: Option<String>,
    pub role: String,
    pub raw_role: Option<String>,
    pub timestamp: String,
    pub reason: String,
    pub content_preview: String,
}

// ── Beacon types ──

#[derive(Debug, Clone, serde::Serialize)]
pub struct BeaconInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub node_count: usize,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BeaconNode {
    pub beacon_id: String,
    pub beacon_name: String,
    pub repo: String,
    pub file_path: String,
    pub symbol_name: String,
    pub annotation: Option<String>,
    pub signature: Option<String>,
    pub stub_content: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub node_type: Option<String>,
}

// ── AST types ──

#[derive(Debug, Clone)]
pub struct AstSyncResult {
    pub inserted: usize,
    pub deleted: usize,
}

#[derive(Debug, Clone)]
pub struct AstNodeRow {
    pub id: String,
    pub repo: String,
    pub file_path: String,
    pub node: crate::ast::CodeNode,
    pub embedding_provider: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AstSearchHit {
    pub id: String,
    pub repo: String,
    pub file_path: String,
    pub name: String,
    pub node_type: String,
    pub signature: String,
    pub start_line: usize,
    pub end_line: usize,
    pub is_exported: bool,
    pub docstring: Option<String>,
    pub stub_content: Option<String>,
    pub calls: Vec<String>,
    pub rank: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AstStats {
    pub total_nodes: usize,
    pub total_files: usize,
    pub total_repos: usize,
    pub embedded_nodes: usize,
}

#[derive(Debug, Clone)]
pub struct ModuleAstSummary {
    pub module_path: String,
    pub public_types: Vec<String>,
    pub public_functions: Vec<String>,
    pub total_nodes: usize,
    pub files: Vec<String>,
    pub file_docs: Vec<(String, String)>,
}

// ── Knowledge utilities ──

fn tokenize_for_similarity(text: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    let mut ascii_word = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            ascii_word.push(ch.to_ascii_lowercase());
        } else {
            if ascii_word.len() >= 2 {
                tokens.insert(ascii_word.clone());
            }
            ascii_word.clear();
            if ch as u32 > 0x2E80 {
                tokens.insert(ch.to_string());
            }
        }
    }
    if ascii_word.len() >= 2 {
        tokens.insert(ascii_word);
    }
    tokens
}

/// Token-level Jaccard similarity (supports Chinese + English mixed text)
pub fn token_jaccard_similarity(a: &str, b: &str) -> f64 {
    let ta = tokenize_for_similarity(a);
    let tb = tokenize_for_similarity(b);
    if ta.is_empty() && tb.is_empty() { return 0.0; }
    let intersection = ta.intersection(&tb).count();
    let union = ta.union(&tb).count();
    if union == 0 { 0.0 } else { intersection as f64 / union as f64 }
}

/// Check if text contains sensitive data patterns.
pub fn contains_sensitive_data(text: &str) -> bool {
    use once_cell::sync::Lazy;
    static PATTERNS: Lazy<Vec<regex::Regex>> = Lazy::new(|| {
        [
            r"(?i)(password|passwd|pwd|secret_key)\s*[:=]\s*\S+",
            r"(?i)sshpass\s+-p\s+",
            r#"(?i)(api[_-]?key|token|secret)\s*[:=]\s*['"]?[A-Za-z0-9_\-]{20,}"#,
            r"(?i)ssh\s+\S+@\S+.*-p\s+'\S+'",
        ].iter().filter_map(|p| regex::Regex::new(p).ok()).collect()
    });
    PATTERNS.iter().any(|re| re.is_match(text))
}

/// Infer KB entry type from category string.
pub fn infer_kb_type(category: &str) -> &'static str {
    let prefix = category.split(':').next().unwrap_or(category);
    match prefix {
        "policy" | "preference" | "system_rule" | "decision" => "rule",
        "feature" | "feature_request" | "project" | "project-requirement" | "design_spec" => "goal",
        "memory" if category.contains("ops") || category.contains("debug") || category.contains("bugfix") => "state",
        "ops" => "state",
        _ => "fact",
    }
}

/// Redact sensitive patterns from text before sending to external APIs.
pub fn redact_sensitive(text: &str) -> String {
    use once_cell::sync::Lazy;
    static REDACTIONS: Lazy<Vec<(regex::Regex, &'static str)>> = Lazy::new(|| {
        vec![
            (regex::Regex::new(r"(?i)sshpass\s+-p\s+'[^']*'").unwrap(), "sshpass -p '[REDACTED]'"),
            (regex::Regex::new(r"(?i)(password|passwd|pwd|secret_key)\s*[:=]\s*\S+").unwrap(), "$1=[REDACTED]"),
            (regex::Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(:\d+)?\b").unwrap(), "[IP_REDACTED]"),
        ]
    });
    let mut result = text.to_string();
    for (re, replacement) in REDACTIONS.iter() {
        result = re.replace_all(&result, *replacement).to_string();
    }
    result
}

/// Parse relative time strings like "10min", "1h", "24h", "7d" into datetime strings.
pub fn parse_relative_time(s: &str) -> String {
    let s = s.trim();
    if let Some(mins) = s.strip_suffix("min").and_then(|v| v.parse::<i64>().ok()) {
        return format!("{}", (chrono::Utc::now() - chrono::Duration::minutes(mins)).format("%Y-%m-%d %H:%M:%S"));
    }
    if let Some(hours) = s.strip_suffix('h').and_then(|v| v.parse::<i64>().ok()) {
        return format!("{}", (chrono::Utc::now() - chrono::Duration::hours(hours)).format("%Y-%m-%d %H:%M:%S"));
    }
    if let Some(days) = s.strip_suffix('d').and_then(|v| v.parse::<i64>().ok()) {
        return format!("{}", (chrono::Utc::now() - chrono::Duration::days(days)).format("%Y-%m-%d %H:%M:%S"));
    }
    if s.contains('T') {
        return s.replace('T', " ").chars().take(19).collect();
    }
    s.to_string()
}

/// Parse time for "since" context: pure date → start of day
pub fn parse_since(s: &str) -> String {
    let ts = parse_relative_time(s);
    if ts.len() == 10 { format!("{} 00:00:00", ts) } else { ts }
}

/// Parse time for "until" context: pure date → end of day
pub fn parse_until(s: &str) -> String {
    let ts = parse_relative_time(s);
    if ts.len() == 10 { format!("{} 23:59:59", ts) } else { ts }
}
