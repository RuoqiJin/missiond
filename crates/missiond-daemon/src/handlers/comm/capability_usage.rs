//! mission_capability_usage — read-model + governance review for tool/flow
//! call hotness. Emits evidence and review candidates only; no destructive
//! action against tool registry or flow catalog.
//!
//! Lisp authority:
//!   - intent-memory.lisp :: system-support :: capability-usage-read-model (v0.5.4)
//!   - intent-flow.lisp :: F-capability-usage-monitoring
//!   - intent-intent-layer.lisp :: capability-evolution-governance
//!   - intent-tools.lisp :: future-surface mission_capability_usage
//!
//! ObservabilityEvent::CapabilityUsageSnapshot is emitted by snapshot/report/
//! candidates after the read-model finishes computing. CapabilityStaleCandidate
//! fans out per non-active, non-protected row from `action=candidates` so
//! review notifiers can react without scanning the full snapshot. Both events
//! are ephemeral (per frozen lisp `:ephemeral-default true`); durable evidence
//! remains in `conversation_tool_calls` / `board_tasks` and the JSON sidecar.
//!
//! Persistence model (mark / ack):
//!   intent-memory.lisp suggests `daemon_state capability_usage_snapshot/v1` as
//!   storage. The actual `daemon_state` trait stores `i64` only — not JSON. To
//!   avoid adding a migration this batch, mark/ack writes a JSON sidecar at
//!   `<project_root>/.missiond/v2/capability-usage-review.json` (file-based,
//!   no DB schema). Snapshot/report/candidates do not persist; each call
//!   re-aggregates from `conversation_tool_calls` + `board_tasks.flow_*`.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use missiond_core::event::events::ObservabilityEvent;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use tracing::warn;

use crate::state::AppState;

/// Forward an `ObservabilityEvent` and log (but never propagate) publish
/// failures. Capability usage events are ephemeral observability — losing one
/// must not block snapshot computation.
async fn emit_observability_event(state: &AppState, ev: ObservabilityEvent) {
    if let Err(e) = state.bus.publish_observability(ev).await {
        warn!(error = %e, "failed to publish ObservabilityEvent (snapshot already returned)");
    }
}

const REVIEW_FILE: &str = ".missiond/v2/capability-usage-review.json";

/// Capabilities that must NOT enter delete/deprecate candidates regardless of
/// usage counts. Mirrors intent-intent-layer.lisp :: capability-evolution-governance
/// :: policy :: protected-capabilities.
const PROTECTED_TOOL_PATTERNS: &[&str] = &[
    "mission_execution",
    "mission_intent",
    "mission_forge_",
    // daemon bootstrap / repair surfaces
    "mission_sys_",
    "mission_daemon_update",
    "mission_health",
    "mission_power_control",
    // memory/event-bus repair (KB ops + audit are repair surfaces)
    "mission_kb_ops",
    "mission_audit",
    // manual recovery
    "mission_pty_signal",
    "mission_pty_confirm",
    "mission_incident",
];

const PROTECTED_FLOW_PATTERNS: &[&str] = &[
    // engineering / recovery flows kept indefinitely
    "engineering",
    "F-execution-log-governance",
    "F-incident-reaction",
    "F-capability-usage-monitoring",
];

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.to_string(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "mission_capability_usage requires `action`",
                )
                .with_suggestion("actions: snapshot|report|candidates|mark|ack"),
            ))
        }
    };

    match action.as_str() {
        "snapshot" => action_snapshot(state, &args).await,
        "report" => action_report(state, &args).await,
        "candidates" => action_candidates(state, &args).await,
        "mark" => action_mark(state, &args).await,
        "ack" => action_ack(state, &args).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_capability_usage action `{}`", other),
            )
            .with_suggestion("valid: snapshot|report|candidates|mark|ack"),
        )),
    }
}

// ───────────────────────────────────────────────────────────────────────
// Snapshot computation — pure aggregation from existing data sources
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct CapabilityRow {
    capability_id: String,
    kind: String, // "tool" | "flow"
    counts_by_window: HashMap<String, i64>,
    last_used_at: Option<String>,
    success_count: i64,
    failure_count: i64,
    success_rate: Option<f64>,
    registered: bool,
    protected: bool,
    classification: String,
    evidence: Vec<String>,
}

#[derive(Debug, Clone)]
struct WindowSpec {
    label: String,
    since: Option<DateTime<Utc>>,
}

fn parse_window(input: Option<&str>) -> Result<Vec<WindowSpec>, ToolResult> {
    // We always compute three nested windows + all-time; `window` arg controls
    // which is the *primary* one used for stale/active classification.
    let primary = input.unwrap_or("30d");
    let primary_dur = match primary {
        "7d" => Some(Duration::days(7)),
        "30d" => Some(Duration::days(30)),
        "90d" => Some(Duration::days(90)),
        "all" => None,
        other => {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("window must be 7d|30d|90d|all, got `{}`", other),
                ),
            ))
        }
    };
    let now = Utc::now();
    let mk = |label: &str, dur: Option<Duration>| WindowSpec {
        label: label.to_string(),
        since: dur.map(|d| now - d),
    };
    let _ = primary_dur; // primary is one of the four below
    Ok(vec![
        mk("7d", Some(Duration::days(7))),
        mk("30d", Some(Duration::days(30))),
        mk("90d", Some(Duration::days(90))),
        mk("all", None),
    ])
}

fn primary_label(input: Option<&str>) -> &'static str {
    match input.unwrap_or("30d") {
        "7d" => "7d",
        "90d" => "90d",
        "all" => "all",
        _ => "30d",
    }
}

fn iso(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn project_arg(args: &Value) -> Option<&str> {
    args.get("project").and_then(|v| v.as_str())
}

async fn resolve_project_root(state: &AppState, project_id: Option<&str>) -> Result<PathBuf> {
    if let Some(id) = project_id {
        if let Some(p) = state.project_registry.read().await.get(id) {
            return Ok(PathBuf::from(&p.path));
        }
        return Err(anyhow!(
            "project '{}' not registered; run mission_project(action=\"list\")",
            id
        ));
    }
    Ok(std::env::current_dir().map_err(|e| anyhow!("cannot read CWD: {}", e))?)
}

async fn collect_tool_usage(
    state: &AppState,
    windows: &[WindowSpec],
) -> Result<HashMap<String, ToolStats>> {
    // First pass: query each window. We iterate windows so each row carries
    // counts per window. PG query is one aggregation per window — for missiond
    // local-first scale this is fine.
    let mut acc: HashMap<String, ToolStats> = HashMap::new();
    for w in windows {
        let since_iso = w.since.map(iso);
        let rows = state
            .store
            .get_tool_call_global_stats(since_iso.as_deref())
            .await
            .map_err(|e| anyhow!("tool_call stats query failed: {}", e))?;
        for (tool_name, total, last_ts, ok, err) in rows {
            let entry = acc.entry(tool_name).or_insert_with(ToolStats::default);
            entry.counts_by_window.insert(w.label.clone(), total);
            // last_used_at + success/error reflect the widest window seen.
            // We also accept smaller windows superseding when fresher.
            if w.label == "all" {
                entry.last_used_at = last_ts.clone();
                entry.success = ok;
                entry.error = err;
            }
            if entry.last_used_at.is_none() {
                entry.last_used_at = last_ts;
            }
        }
    }
    Ok(acc)
}

#[derive(Debug, Default, Clone)]
struct ToolStats {
    counts_by_window: HashMap<String, i64>,
    last_used_at: Option<String>,
    success: i64,
    error: i64,
}

async fn collect_flow_usage(state: &AppState) -> Result<HashMap<String, FlowStats>> {
    // board_tasks.flow_template carries the executable flow id when the task
    // came from F9 / autopilot / mission_flow_run. We aggregate by template name
    // across all tasks (status agnostic; success/failure derived from
    // BoardTaskStatus). This matches intent-flow.lisp F-capability-usage-monitoring
    // step s2's "board_tasks.flow_template / flow_context, workflow execution
    // stats" instruction.
    let tasks = state
        .store
        .list_board_tasks(None, true)
        .await
        .map_err(|e| anyhow!("board_tasks query failed: {}", e))?;
    let mut acc: HashMap<String, FlowStats> = HashMap::new();
    for t in tasks {
        let Some(template) = t.flow_template.clone() else {
            continue;
        };
        let entry = acc.entry(template).or_insert_with(FlowStats::default);
        entry.total += 1;
        let last = t.updated_at.clone();
        if entry.last_used_at.as_deref().map_or(true, |cur| cur < last.as_str()) {
            entry.last_used_at = Some(last);
        }
        match t.status.as_str() {
            "done" | "completed" => entry.completed += 1,
            "failed" | "blocked" => entry.failed += 1,
            "running" | "in_progress" => entry.running += 1,
            _ => {}
        }
    }
    Ok(acc)
}

#[derive(Debug, Default, Clone)]
struct FlowStats {
    total: i64,
    completed: i64,
    failed: i64,
    running: i64,
    last_used_at: Option<String>,
}

fn registered_tools() -> Vec<String> {
    missiond_mcp::tools::all_tools()
        .into_iter()
        .map(|t| t.name)
        .collect()
}

fn registered_flows() -> Vec<String> {
    crate::engine::flow::loader::list_flows().unwrap_or_default()
}

fn is_protected_tool(name: &str) -> bool {
    PROTECTED_TOOL_PATTERNS.iter().any(|p| {
        if p.ends_with('_') {
            name.starts_with(p)
        } else {
            name == *p
        }
    })
}

fn is_protected_flow(name: &str) -> bool {
    PROTECTED_FLOW_PATTERNS.iter().any(|p| name == *p || name.starts_with(p))
}

fn classify_tool(stats: &ToolStats, registered: bool, protected: bool) -> (String, Vec<String>) {
    let mut evidence = Vec::new();
    let count_30d = stats.counts_by_window.get("30d").copied().unwrap_or(0);
    let count_90d = stats.counts_by_window.get("90d").copied().unwrap_or(0);
    let count_all = stats.counts_by_window.get("all").copied().unwrap_or(0);

    if protected {
        evidence.push(format!(
            "protected by capability-evolution-governance policy (count_all={})",
            count_all
        ));
        return ("protected".to_string(), evidence);
    }
    if !registered && count_all > 0 {
        evidence.push(
            "tool fired but absent from current MCP registry (legacy alias or removed)".to_string(),
        );
        return ("shadowed-by-better-capability".to_string(), evidence);
    }
    if registered && count_all == 0 {
        evidence.push("registered but no audit record in conversation_tool_calls".to_string());
        return ("never-used".to_string(), evidence);
    }
    if count_30d > 0 {
        evidence.push(format!("count_30d={}", count_30d));
        return ("active".to_string(), evidence);
    }
    if count_90d > 0 {
        evidence.push(format!("count_30d=0, count_90d={}", count_90d));
        return ("quiet".to_string(), evidence);
    }
    evidence.push(format!("count_90d=0, count_all={}", count_all));
    ("stale".to_string(), evidence)
}

fn classify_flow(stats: &FlowStats, registered: bool, protected: bool) -> (String, Vec<String>) {
    let mut evidence = Vec::new();
    if protected {
        evidence.push(format!("protected flow (total={})", stats.total));
        return ("protected".to_string(), evidence);
    }
    if !registered && stats.total > 0 {
        evidence.push("flow_template fired but YAML missing under $MISSIOND_HOME/flows".to_string());
        return ("shadowed-by-better-capability".to_string(), evidence);
    }
    if registered && stats.total == 0 {
        evidence.push("registered YAML but never instantiated as board_task".to_string());
        return ("never-used".to_string(), evidence);
    }
    // Recency check uses last_used_at against now.
    let now = Utc::now();
    if let Some(last) = stats.last_used_at.as_deref().and_then(parse_iso_loose) {
        let age = now - last;
        if age < Duration::days(30) {
            evidence.push(format!("last_used_at={} (<30d)", last.to_rfc3339()));
            return ("active".to_string(), evidence);
        } else if age < Duration::days(90) {
            evidence.push(format!("last_used_at={} (30-90d)", last.to_rfc3339()));
            return ("quiet".to_string(), evidence);
        }
        evidence.push(format!("last_used_at={} (>90d)", last.to_rfc3339()));
        return ("stale".to_string(), evidence);
    }
    evidence.push("no parseable last_used_at".to_string());
    ("stale".to_string(), evidence)
}

fn parse_iso_loose(s: &str) -> Option<DateTime<Utc>> {
    // board_tasks.updated_at is text; PG default uses "YYYY-MM-DD HH:MM:SS"
    // (no offset). conversation_tool_calls.timestamp typically holds RFC3339.
    // Try RFC3339 first, then a relaxed parse via NaiveDateTime.
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    None
}

async fn build_capability_rows(
    state: &AppState,
    windows: &[WindowSpec],
    scope: &str,
) -> Result<(Vec<CapabilityRow>, SourceCoverage)> {
    let mut rows: Vec<CapabilityRow> = Vec::new();
    let mut coverage = SourceCoverage::default();

    if scope == "tool" || scope == "both" {
        let tool_stats = collect_tool_usage(state, windows).await?;
        let registered: BTreeSet<String> = registered_tools().into_iter().collect();
        coverage.tool_registry_count = registered.len();
        coverage.tool_observed_count = tool_stats.len();

        let mut all_ids: BTreeSet<String> = registered.clone();
        for k in tool_stats.keys() {
            all_ids.insert(k.clone());
        }
        for id in all_ids {
            let stats = tool_stats.get(&id).cloned().unwrap_or_default();
            let registered_flag = registered.contains(&id);
            let protected = is_protected_tool(&id);
            let (classification, evidence) = classify_tool(&stats, registered_flag, protected);
            let total = stats.success + stats.error;
            let success_rate = if total > 0 {
                Some(stats.success as f64 / total as f64)
            } else {
                None
            };
            rows.push(CapabilityRow {
                capability_id: id,
                kind: "tool".to_string(),
                counts_by_window: stats.counts_by_window,
                last_used_at: stats.last_used_at,
                success_count: stats.success,
                failure_count: stats.error,
                success_rate,
                registered: registered_flag,
                protected,
                classification,
                evidence,
            });
        }
    }

    if scope == "flow" || scope == "both" {
        let flow_stats = collect_flow_usage(state).await?;
        let registered_v: BTreeSet<String> = registered_flows().into_iter().collect();
        coverage.flow_registry_count = registered_v.len();
        coverage.flow_observed_count = flow_stats.len();

        let mut all_ids: BTreeSet<String> = registered_v.clone();
        for k in flow_stats.keys() {
            all_ids.insert(k.clone());
        }
        for id in all_ids {
            let stats = flow_stats.get(&id).cloned().unwrap_or_default();
            let registered_flag = registered_v.contains(&id);
            let protected = is_protected_flow(&id);
            let (classification, evidence) = classify_flow(&stats, registered_flag, protected);
            let mut counts = HashMap::new();
            counts.insert("all".to_string(), stats.total);
            counts.insert("running".to_string(), stats.running);
            let success_rate = if stats.total > 0 {
                Some(stats.completed as f64 / stats.total as f64)
            } else {
                None
            };
            rows.push(CapabilityRow {
                capability_id: id,
                kind: "flow".to_string(),
                counts_by_window: counts,
                last_used_at: stats.last_used_at,
                success_count: stats.completed,
                failure_count: stats.failed,
                success_rate,
                registered: registered_flag,
                protected,
                classification,
                evidence,
            });
        }
    }

    Ok((rows, coverage))
}

#[derive(Debug, Default, Serialize)]
struct SourceCoverage {
    tool_registry_count: usize,
    tool_observed_count: usize,
    flow_registry_count: usize,
    flow_observed_count: usize,
    persistence_mode: String,
    persistence_note: String,
}

async fn take_snapshot(
    state: &AppState,
    args: &Value,
) -> Result<(Value, Vec<CapabilityRow>, SourceCoverage)> {
    let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("both");
    if !matches!(scope, "tool" | "flow" | "both") {
        return Err(anyhow!("scope must be tool|flow|both"));
    }
    let primary = primary_label(args.get("window").and_then(|v| v.as_str()));
    let windows = match parse_window(args.get("window").and_then(|v| v.as_str())) {
        Ok(w) => w,
        Err(_) => return Err(anyhow!("invalid window — expected 7d|30d|90d|all")),
    };
    let (rows, mut coverage) = build_capability_rows(state, &windows, scope).await?;
    coverage.persistence_mode = "read-only".to_string();
    coverage.persistence_note =
        "snapshot is recomputed each call; daemon_state is i64-only so no JSON cache. \
         mark/ack writes a JSON sidecar at .missiond/v2/capability-usage-review.json"
            .to_string();

    let mut counts_by_capability = serde_json::Map::new();
    let mut last_used_at = serde_json::Map::new();
    let mut success_failure = serde_json::Map::new();
    let mut protected_ids: Vec<String> = Vec::new();
    for r in &rows {
        counts_by_capability.insert(
            r.capability_id.clone(),
            serde_json::to_value(&r.counts_by_window).unwrap_or(Value::Null),
        );
        if let Some(ts) = &r.last_used_at {
            last_used_at.insert(r.capability_id.clone(), Value::String(ts.clone()));
        }
        success_failure.insert(
            r.capability_id.clone(),
            json!({"success": r.success_count, "failure": r.failure_count, "success_rate": r.success_rate}),
        );
        if r.protected {
            protected_ids.push(r.capability_id.clone());
        }
    }

    let generated_at = iso(Utc::now());
    let protected_count = protected_ids.len() as u32;
    let candidate_count = rows
        .iter()
        .filter(|r| !r.protected && r.classification != "active" && r.classification != "protected")
        .count() as u32;
    let snapshot = json!({
        "window": primary,
        "scope": scope,
        "generated_at": generated_at,
        "counts_by_capability": counts_by_capability,
        "last_used_at": last_used_at,
        "success_failure": success_failure,
        "source_coverage": serde_json::to_value(&coverage).unwrap_or(Value::Null),
        "protected_ids": protected_ids,
    });

    emit_observability_event(
        state,
        ObservabilityEvent::CapabilityUsageSnapshot {
            window: primary.to_string(),
            scope: scope.to_string(),
            generated_at: generated_at.clone(),
            capability_count: rows.len() as u32,
            protected_count,
            candidate_count,
        },
    )
    .await;

    Ok((snapshot, rows, coverage))
}

// ───────────────────────────────────────────────────────────────────────
// snapshot
// ───────────────────────────────────────────────────────────────────────

async fn action_snapshot(state: &AppState, args: &Value) -> Result<ToolResult> {
    let (snapshot, _, _) = take_snapshot(state, args).await?;
    Ok(ToolResult::json_pretty(&snapshot))
}

// ───────────────────────────────────────────────────────────────────────
// report — snapshot + per-row narrative
// ───────────────────────────────────────────────────────────────────────

async fn action_report(state: &AppState, args: &Value) -> Result<ToolResult> {
    let (snapshot, rows, _) = take_snapshot(state, args).await?;
    let mut by_class: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for r in &rows {
        by_class
            .entry(r.classification.clone())
            .or_default()
            .push(json!({
                "capability_id": r.capability_id,
                "kind": r.kind,
                "counts_by_window": r.counts_by_window,
                "last_used_at": r.last_used_at,
                "success_count": r.success_count,
                "failure_count": r.failure_count,
                "success_rate": r.success_rate,
                "registered": r.registered,
                "protected": r.protected,
                "evidence": r.evidence,
            }));
    }
    let report = json!({
        "snapshot": snapshot,
        "rows_by_classification": by_class,
        "row_count": rows.len(),
    });
    Ok(ToolResult::json_pretty(&report))
}

// ───────────────────────────────────────────────────────────────────────
// candidates — only ranks stale / never-used / shadowed for review
// ───────────────────────────────────────────────────────────────────────

async fn action_candidates(state: &AppState, args: &Value) -> Result<ToolResult> {
    let (snapshot, rows, _) = take_snapshot(state, args).await?;
    let include_protected = args
        .get("include_protected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // Reuse the snapshot's generated_at so per-row events correlate to the
    // CapabilityUsageSnapshot emitted by take_snapshot.
    let snapshot_generated_at = snapshot
        .get("generated_at")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    // Read review state to surface "already marked / acked" candidates.
    let project_root = resolve_project_root(state, project_arg(args)).await?;
    let review = ReviewState::load(&project_root).unwrap_or_default();

    let mut buckets: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for r in &rows {
        if r.protected && !include_protected {
            continue;
        }
        if r.classification == "active" || r.classification == "protected" && !include_protected {
            continue;
        }
        let entry = review.entries.get(&r.capability_id);
        let payload = json!({
            "capability_id": r.capability_id,
            "kind": r.kind,
            "classification": r.classification,
            "counts_by_window": r.counts_by_window,
            "last_used_at": r.last_used_at,
            "evidence": r.evidence,
            "review": entry.map(|e| json!({
                "decision": e.decision,
                "decided_by": e.decided_by,
                "decided_at": e.decided_at,
                "ack_at": e.ack_at,
                "rationale": e.rationale,
            })),
        });
        buckets.entry(r.classification.clone()).or_default().push(payload);
        emit_observability_event(
            state,
            ObservabilityEvent::CapabilityStaleCandidate {
                capability_id: r.capability_id.clone(),
                kind: r.kind.clone(),
                classification: r.classification.clone(),
                last_used_at: r.last_used_at.clone(),
                generated_at: snapshot_generated_at.clone(),
            },
        )
        .await;
    }

    // Empty bucket placeholders for the 7 declared categories so callers can
    // iterate a stable shape.
    for cat in [
        "active",
        "quiet",
        "stale",
        "never-used",
        "shadowed-by-better-capability",
        "merge-candidate",
        "protected",
    ] {
        buckets.entry(cat.to_string()).or_default();
    }

    Ok(ToolResult::json_pretty(&json!({
        "generated_at": iso(Utc::now()),
        "candidates": buckets,
        "include_protected": include_protected,
        "notes": [
            "merge-candidate is currently empty: semantic-overlap detection requires parsing intent-tools/intent-flow Lisp registries (deferred).",
            "shadowed-by-better-capability covers tools fired but absent from MCP registry (legacy aliases) and flows fired but missing YAML.",
            "protected list mirrors capability-evolution-governance policy + bootstrap/repair surfaces.",
        ],
    })))
}

// ───────────────────────────────────────────────────────────────────────
// mark / ack — JSON sidecar persistence
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ReviewState {
    #[serde(default)]
    entries: BTreeMap<String, ReviewEntry>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ReviewEntry {
    decision: String,
    decided_by: String,
    decided_at: String,
    rationale: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ack_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ack_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    follow_up_ref: Option<String>,
}

impl ReviewState {
    fn path(project_root: &Path) -> PathBuf {
        project_root.join(REVIEW_FILE)
    }
    fn load(project_root: &Path) -> Result<Self> {
        let p = Self::path(project_root);
        if !p.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&p)?;
        let parsed: ReviewState = serde_json::from_str(&text)
            .map_err(|e| anyhow!("review state at {} is malformed: {}", p.display(), e))?;
        Ok(parsed)
    }
    fn save(&self, project_root: &Path) -> Result<PathBuf> {
        let p = Self::path(project_root);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(self)?;
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, body.as_bytes())?;
        std::fs::rename(&tmp, &p)?;
        Ok(p)
    }
}

async fn action_mark(state: &AppState, args: &Value) -> Result<ToolResult> {
    let candidate_id = match args.get("candidate_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::MISSING_PARAM, "mark requires `candidate_id`"),
            ))
        }
    };
    let decision = args
        .get("decision")
        .and_then(|v| v.as_str())
        .unwrap_or("review");
    if !matches!(
        decision,
        "keep" | "monitor" | "merge" | "deprecate" | "remove-after-compat-window" | "review"
    ) {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!(
                    "decision must be keep|monitor|merge|deprecate|remove-after-compat-window|review, got `{}`",
                    decision
                ),
            )
            .with_suggestion("see capability-evolution-governance lifecycle-states"),
        ));
    }
    let decided_by = args
        .get("decided_by")
        .and_then(|v| v.as_str())
        .unwrap_or("unspecified");
    let rationale = args
        .get("rationale")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Refuse to mark protected capabilities for destructive decisions.
    let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("tool");
    let protected = match kind {
        "flow" => is_protected_flow(candidate_id),
        _ => is_protected_tool(candidate_id),
    };
    if protected && matches!(decision, "deprecate" | "remove-after-compat-window" | "merge") {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "PROTECTED_CAPABILITY",
                format!(
                    "`{}` is protected by capability-evolution-governance policy and cannot be {}",
                    candidate_id, decision
                ),
            )
            .with_suggestion("only `keep` / `monitor` / `review` are valid for protected ids"),
        ));
    }

    let project_root = resolve_project_root(state, project_arg(args)).await?;
    let dry_run = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let entry = ReviewEntry {
        decision: decision.to_string(),
        decided_by: decided_by.to_string(),
        decided_at: iso(Utc::now()),
        rationale: rationale.to_string(),
        ack_at: None,
        ack_by: None,
        follow_up_ref: None,
    };

    if dry_run {
        return Ok(ToolResult::json_pretty(&json!({
            "status": "dry-run",
            "would_mark": {
                "capability_id": candidate_id,
                "kind": kind,
                "entry": entry,
            },
            "path": ReviewState::path(&project_root).display().to_string(),
            "note": "set dry_run=false to persist (writes JSON sidecar; no DB schema involved)",
        })));
    }

    let mut review = ReviewState::load(&project_root).unwrap_or_default();
    review.entries.insert(candidate_id.to_string(), entry.clone());
    let path = review.save(&project_root)?;
    Ok(ToolResult::json_pretty(&json!({
        "status": "marked",
        "capability_id": candidate_id,
        "kind": kind,
        "entry": entry,
        "path": path.display().to_string(),
    })))
}

async fn action_ack(state: &AppState, args: &Value) -> Result<ToolResult> {
    let candidate_id = match args.get("candidate_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::MISSING_PARAM, "ack requires `candidate_id`"),
            ))
        }
    };
    let ack_by = args
        .get("ack_by")
        .and_then(|v| v.as_str())
        .unwrap_or("unspecified");
    let follow_up_ref = args
        .get("follow_up_ref")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let project_root = resolve_project_root(state, project_arg(args)).await?;

    let mut review = ReviewState::load(&project_root).unwrap_or_default();
    let entry = match review.entries.get_mut(candidate_id) {
        Some(e) => e,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!(
                        "no marked candidate `{}` in review state",
                        candidate_id
                    ),
                )
                .with_suggestion("call action=mark first to record a decision"),
            ))
        }
    };
    if follow_up_ref.is_empty() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::MISSING_PARAM,
                "ack requires `follow_up_ref` (PLAN.lisp / mission_execution id / board task id)",
            )
            .with_suggestion(
                "intent-tools.lisp future-surface mission_capability_usage step s5: ack closes only after a follow-up exists",
            ),
        ));
    }
    entry.ack_at = Some(iso(Utc::now()));
    entry.ack_by = Some(ack_by.to_string());
    entry.follow_up_ref = Some(follow_up_ref.to_string());
    let saved = entry.clone();
    let path = review.save(&project_root)?;
    Ok(ToolResult::json_pretty(&json!({
        "status": "acked",
        "capability_id": candidate_id,
        "entry": saved,
        "path": path.display().to_string(),
    })))
}

// ───────────────────────────────────────────────────────────────────────
// tests
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_tool_pattern_matches_prefix_and_exact() {
        assert!(is_protected_tool("mission_intent"));
        assert!(is_protected_tool("mission_forge_build"));
        assert!(is_protected_tool("mission_forge_lint"));
        assert!(is_protected_tool("mission_execution"));
        assert!(is_protected_tool("mission_audit"));
        assert!(!is_protected_tool("mission_kb_query"));
        assert!(!is_protected_tool("mission_board_query"));
    }

    #[test]
    fn protected_flow_includes_governance_flows() {
        assert!(is_protected_flow("engineering"));
        assert!(is_protected_flow("F-execution-log-governance"));
        assert!(!is_protected_flow("hello-parallel"));
    }

    #[test]
    fn classify_tool_active_when_recent_calls() {
        let mut s = ToolStats::default();
        s.counts_by_window.insert("30d".to_string(), 5);
        s.counts_by_window.insert("90d".to_string(), 8);
        s.counts_by_window.insert("all".to_string(), 12);
        let (cls, _) = classify_tool(&s, true, false);
        assert_eq!(cls, "active");
    }

    #[test]
    fn classify_tool_quiet_when_only_90d() {
        let mut s = ToolStats::default();
        s.counts_by_window.insert("30d".to_string(), 0);
        s.counts_by_window.insert("90d".to_string(), 3);
        s.counts_by_window.insert("all".to_string(), 4);
        let (cls, _) = classify_tool(&s, true, false);
        assert_eq!(cls, "quiet");
    }

    #[test]
    fn classify_tool_stale_when_only_old() {
        let mut s = ToolStats::default();
        s.counts_by_window.insert("30d".to_string(), 0);
        s.counts_by_window.insert("90d".to_string(), 0);
        s.counts_by_window.insert("all".to_string(), 2);
        let (cls, _) = classify_tool(&s, true, false);
        assert_eq!(cls, "stale");
    }

    #[test]
    fn classify_tool_never_used_when_registered_zero() {
        let s = ToolStats::default();
        let (cls, _) = classify_tool(&s, true, false);
        assert_eq!(cls, "never-used");
    }

    #[test]
    fn classify_tool_shadowed_when_unregistered_but_observed() {
        let mut s = ToolStats::default();
        s.counts_by_window.insert("all".to_string(), 1);
        let (cls, _) = classify_tool(&s, false, false);
        assert_eq!(cls, "shadowed-by-better-capability");
    }

    #[test]
    fn classify_tool_protected_short_circuits() {
        let mut s = ToolStats::default();
        s.counts_by_window.insert("all".to_string(), 0);
        let (cls, _) = classify_tool(&s, true, true);
        assert_eq!(cls, "protected");
    }

    #[test]
    fn parse_iso_loose_handles_pg_naive_and_rfc3339() {
        let a = parse_iso_loose("2026-04-25T01:02:03Z").unwrap();
        let b = parse_iso_loose("2026-04-25 01:02:03").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn review_state_round_trip() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let mut s = ReviewState::default();
        s.entries.insert(
            "mission_minimax_process".to_string(),
            ReviewEntry {
                decision: "deprecate".to_string(),
                decided_by: "tester".to_string(),
                decided_at: "2026-04-25T00:00:00Z".to_string(),
                rationale: "deprecated by sonnet gateway".to_string(),
                ack_at: None,
                ack_by: None,
                follow_up_ref: None,
            },
        );
        let path = s.save(dir.path()).unwrap();
        assert!(path.exists());
        let loaded = ReviewState::load(dir.path()).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(
            loaded.entries.get("mission_minimax_process").unwrap().decision,
            "deprecate"
        );
    }
}
