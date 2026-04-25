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
//! v0.5.5 semantic-evidence: classify_* now carries explicit lisp hints
//! (deprecated / moved-to / replacement / supersedes) into the merge-candidate
//! bucket, plus a read-only event_log probe that surfaces flow_template
//! references in board::task_created payloads. No DB migrations; new lanes
//! are reported under `source_coverage.sources`.
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
    /// When classification == "merge-candidate", names the resolved target
    /// capability id (must exist in the registry; never the row's own id).
    #[serde(skip_serializing_if = "Option::is_none")]
    merge_target: Option<String>,
    /// Compact references to the lisp anchors that drove the classification
    /// (e.g. ["intent-tools.lisp:732 deprecated -> mission_sonnet_process"]).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    hint_evidence: Vec<String>,
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
    /// Cross-source: number of board::task_created events in event_log
    /// whose payload referenced this flow_template. Distinct from `total`
    /// (which counts board_tasks rows directly).
    event_log_observed: i64,
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
    project_root: &Path,
) -> Result<(
    Vec<CapabilityRow>,
    SourceCoverage,
    HintIndex,
    EventLogFlowProbe,
    WorkflowStatsProbe,
)> {
    let mut rows: Vec<CapabilityRow> = Vec::new();
    let mut coverage = SourceCoverage::default();
    let registered_tools_set: BTreeSet<String> = registered_tools().into_iter().collect();
    let registered_flows_set: BTreeSet<String> = registered_flows().into_iter().collect();
    let hint_index = HintIndex::from_project_root(project_root);
    coverage.lisp_files_read = hint_index.files_read.clone();

    if scope == "tool" || scope == "both" {
        let tool_stats = collect_tool_usage(state, windows).await?;
        coverage.tool_registry_count = registered_tools_set.len();
        coverage.tool_observed_count = tool_stats.len();

        let mut all_ids: BTreeSet<String> = registered_tools_set.clone();
        for k in tool_stats.keys() {
            all_ids.insert(k.clone());
        }
        for id in all_ids {
            let stats = tool_stats.get(&id).cloned().unwrap_or_default();
            let registered_flag = registered_tools_set.contains(&id);
            let protected = is_protected_tool(&id);
            let (base_class, base_evidence) = classify_tool(&stats, registered_flag, protected);
            let hints = hint_index.for_source("tool", &id);
            let (classification, evidence, merge_target, hint_evidence) = apply_hint_overlay(
                &base_class,
                base_evidence,
                &id,
                &hints,
                &registered_tools_set,
                &registered_flows_set,
            );
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
                merge_target,
                hint_evidence,
            });
        }
    }

    let event_probe = if scope == "flow" || scope == "both" {
        probe_event_log_flows(state).await
    } else {
        EventLogFlowProbe::default()
    };

    let workflow_probe = if scope == "flow" || scope == "both" {
        probe_workflow_execution_stats(state).await
    } else {
        WorkflowStatsProbe::default()
    };

    if scope == "flow" || scope == "both" {
        let mut flow_stats = collect_flow_usage(state).await?;
        for (template, count) in &event_probe.counts {
            let entry = flow_stats.entry(template.clone()).or_default();
            entry.event_log_observed += *count;
        }
        coverage.flow_registry_count = registered_flows_set.len();
        coverage.flow_observed_count = flow_stats.len();

        let mut all_ids: BTreeSet<String> = registered_flows_set.clone();
        for k in flow_stats.keys() {
            all_ids.insert(k.clone());
        }
        for id in all_ids {
            let stats = flow_stats.get(&id).cloned().unwrap_or_default();
            let registered_flag = registered_flows_set.contains(&id);
            let protected = is_protected_flow(&id);
            let (base_class, mut base_evidence) =
                classify_flow(&stats, registered_flag, protected);
            if stats.event_log_observed > 0 {
                base_evidence.push(format!(
                    "event_log_flow_events: {} board::task_created events referenced this template",
                    stats.event_log_observed
                ));
            }
            // workflow_execution_stats lane: exact-name mapping only.
            // Upgrade evidence with workflow row stats when this flow id matches a
            // workflow.name verbatim (or appears in workflow.match_rules.tool_calls
            // / .flows array). Never alters classification — the workflow table is a
            // separate distillation surface and we cannot prove a flow YAML and a
            // workflow row are the same artifact.
            if let Some(wf_stats) = workflow_probe.by_name.get(&id) {
                base_evidence.push(format!(
                    "workflow_execution_stats: name=`{}` executions={} success={} failure={} avg_cost_usd={} last_used_at={}",
                    wf_stats.name,
                    wf_stats.executions,
                    wf_stats.success_count,
                    wf_stats.failure_count,
                    wf_stats
                        .avg_cost_usd
                        .map(|c| format!("{:.4}", c))
                        .unwrap_or_else(|| "null".to_string()),
                    wf_stats
                        .last_used_at
                        .as_deref()
                        .unwrap_or("null"),
                ));
            }
            for wf_stats in workflow_probe
                .by_match_reference
                .get(&id)
                .map(|v| v.as_slice())
                .unwrap_or(&[])
            {
                base_evidence.push(format!(
                    "workflow_execution_stats: match_rules in workflow `{}` references this flow (executions={}, success={}, failure={})",
                    wf_stats.name,
                    wf_stats.executions,
                    wf_stats.success_count,
                    wf_stats.failure_count,
                ));
            }
            let hints = hint_index.for_source("flow", &id);
            let (classification, evidence, merge_target, hint_evidence) = apply_hint_overlay(
                &base_class,
                base_evidence,
                &id,
                &hints,
                &registered_tools_set,
                &registered_flows_set,
            );
            let mut counts = HashMap::new();
            counts.insert("all".to_string(), stats.total);
            counts.insert("running".to_string(), stats.running);
            counts.insert("event_log".to_string(), stats.event_log_observed);
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
                merge_target,
                hint_evidence,
            });
        }
    }

    Ok((rows, coverage, hint_index, event_probe, workflow_probe))
}

/// Per-source diagnostic entry — surfaces *why* a given lane is on or off.
#[derive(Debug, Default, Clone, Serialize)]
struct SourceLane {
    status: String,
    count: i64,
    note: String,
}

#[derive(Debug, Default, Serialize)]
struct SourceCoverage {
    tool_registry_count: usize,
    tool_observed_count: usize,
    flow_registry_count: usize,
    flow_observed_count: usize,
    persistence_mode: String,
    persistence_note: String,
    /// Five-source dictionary mirroring the v0.5 read-model contract.
    sources: BTreeMap<String, SourceLane>,
    /// Files actually read by the lisp hint parser this call.
    lisp_files_read: Vec<String>,
}

async fn take_snapshot(
    state: &AppState,
    args: &Value,
) -> Result<(Value, Vec<CapabilityRow>, SourceCoverage, HintIndex)> {
    let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("both");
    if !matches!(scope, "tool" | "flow" | "both") {
        return Err(anyhow!("scope must be tool|flow|both"));
    }
    let primary = primary_label(args.get("window").and_then(|v| v.as_str()));
    let windows = match parse_window(args.get("window").and_then(|v| v.as_str())) {
        Ok(w) => w,
        Err(_) => return Err(anyhow!("invalid window — expected 7d|30d|90d|all")),
    };
    let project_root = resolve_project_root(state, project_arg(args)).await?;
    let (rows, mut coverage, hint_index, event_probe, workflow_probe) =
        build_capability_rows(state, &windows, scope, &project_root).await?;
    coverage.persistence_mode = "read-only".to_string();
    coverage.persistence_note =
        "snapshot is recomputed each call; daemon_state is i64-only so no JSON cache. \
         mark/ack writes a JSON sidecar at .missiond/v2/capability-usage-review.json"
            .to_string();
    coverage.sources = build_source_coverage_dict(
        &rows,
        &hint_index,
        &event_probe,
        &workflow_probe,
        ReviewState::load(&project_root).map(|s| s.entries.len()).ok(),
    );

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

    Ok((snapshot, rows, coverage, hint_index))
}

/// Compose the six-source dictionary for `source_coverage.sources`.
fn build_source_coverage_dict(
    rows: &[CapabilityRow],
    hint_index: &HintIndex,
    event_probe: &EventLogFlowProbe,
    workflow_probe: &WorkflowStatsProbe,
    review_entry_count: Option<usize>,
) -> BTreeMap<String, SourceLane> {
    let mut out = BTreeMap::new();

    let tool_observed = rows
        .iter()
        .filter(|r| r.kind == "tool" && r.last_used_at.is_some())
        .count() as i64;
    out.insert(
        "conversation_tool_calls".to_string(),
        SourceLane {
            status: if tool_observed > 0 { "active".into() } else { "empty".into() },
            count: tool_observed,
            note: "aggregated via Store::get_tool_call_global_stats".to_string(),
        },
    );

    let flow_observed = rows
        .iter()
        .filter(|r| r.kind == "flow" && r.last_used_at.is_some())
        .count() as i64;
    out.insert(
        "board_tasks_flow_template".to_string(),
        SourceLane {
            status: if flow_observed > 0 { "active".into() } else { "empty".into() },
            count: flow_observed,
            note: "scanned via Store::list_board_tasks(include_hidden=true)".to_string(),
        },
    );

    let probe_status = if event_probe.error.is_some() {
        "unavailable"
    } else if event_probe.events_with_template > 0 {
        "active"
    } else {
        "empty"
    };
    let probe_note = match (&event_probe.error, event_probe.events_scanned) {
        (Some(e), _) => format!("probe failed: {}", e),
        (None, 0) => "no board::task_created events in the past 90d".to_string(),
        (None, scanned) => format!(
            "scanned {} board::task_created events; {} carried flow_template",
            scanned, event_probe.events_with_template
        ),
    };
    out.insert(
        "event_log_flow_events".to_string(),
        SourceLane {
            status: probe_status.to_string(),
            count: event_probe.events_with_template,
            note: probe_note,
        },
    );

    let hint_count = hint_index.all.len() as i64;
    let hint_note = if hint_index.files_read.is_empty() {
        "no .missiond/v2 lisp files found".to_string()
    } else {
        format!(
            "read {} files: {}",
            hint_index.files_read.len(),
            hint_index.files_read.join(", ")
        )
    };
    out.insert(
        "lisp_semantic_hints".to_string(),
        SourceLane {
            status: if hint_count > 0 { "active".into() } else { "empty".into() },
            count: hint_count,
            note: hint_note,
        },
    );

    let review_note = match review_entry_count {
        Some(n) if n > 0 => format!(
            "{} entries in .missiond/v2/capability-usage-review.json",
            n
        ),
        Some(_) => "review sidecar empty".to_string(),
        None => "review sidecar absent or unreadable".to_string(),
    };
    out.insert(
        "review_sidecar".to_string(),
        SourceLane {
            status: match review_entry_count {
                Some(n) if n > 0 => "active".into(),
                Some(_) => "empty".into(),
                None => "empty".into(),
            },
            count: review_entry_count.unwrap_or(0) as i64,
            note: review_note,
        },
    );

    let workflow_status = if workflow_probe.error.is_some() {
        "unavailable"
    } else if workflow_probe.workflows_observed > 0 {
        "active"
    } else {
        "empty"
    };
    let workflow_note = match (&workflow_probe.error, workflow_probe.workflows_observed) {
        (Some(e), _) => format!("workflow stats query failed: {}", e),
        (None, 0) => "no rows in workflow table (DirectiveLayerStore::workflow_list_top_n)".to_string(),
        (None, observed) => format!(
            "scanned {} workflow rows via DirectiveLayerStore::workflow_list_top_n; \
             upgraded evidence on {} flow ids by exact-name match and {} via match_rules \
             references (no destructive classification)",
            observed,
            workflow_probe.by_name.len(),
            workflow_probe.by_match_reference.len(),
        ),
    };
    out.insert(
        "workflow_execution_stats".to_string(),
        SourceLane {
            status: workflow_status.to_string(),
            count: workflow_probe.workflows_observed,
            note: workflow_note,
        },
    );

    out
}

// ───────────────────────────────────────────────────────────────────────
// snapshot
// ───────────────────────────────────────────────────────────────────────

async fn action_snapshot(state: &AppState, args: &Value) -> Result<ToolResult> {
    let (snapshot, _, _, _) = take_snapshot(state, args).await?;
    Ok(ToolResult::json_pretty(&snapshot))
}

async fn action_report(state: &AppState, args: &Value) -> Result<ToolResult> {
    let (snapshot, rows, _, _) = take_snapshot(state, args).await?;
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
                "merge_target": r.merge_target,
                "hint_evidence": r.hint_evidence,
            }));
    }
    let report = json!({
        "snapshot": snapshot,
        "rows_by_classification": by_class,
        "row_count": rows.len(),
    });
    Ok(ToolResult::json_pretty(&report))
}

async fn action_candidates(state: &AppState, args: &Value) -> Result<ToolResult> {
    let (snapshot, rows, _, hint_index) = take_snapshot(state, args).await?;
    let include_protected = args
        .get("include_protected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let snapshot_generated_at = snapshot
        .get("generated_at")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

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
            "merge_target": r.merge_target,
            "hint_evidence": r.hint_evidence,
            "review": entry.map(|e| json!({
                "decision": e.decision,
                "decided_by": e.decided_by,
                "decided_at": e.decided_at,
                "ack_at": e.ack_at,
                "rationale": e.rationale,
                "replacement_target": e.replacement_target,
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

    let merge_candidate_count = buckets
        .get("merge-candidate")
        .map(|v| v.len())
        .unwrap_or(0);

    Ok(ToolResult::json_pretty(&json!({
        "generated_at": iso(Utc::now()),
        "candidates": buckets,
        "include_protected": include_protected,
        "notes": [
            format!(
                "merge-candidate now sourced from explicit lisp hints (deprecated/moved-to/replacement/supersedes); {} merge-candidates this run",
                merge_candidate_count
            ),
            format!(
                "lisp hint parser read {} files and produced {} hints; merge-candidate populated only when a hint resolves a registered, non-self target",
                hint_index.files_read.len(),
                hint_index.all.len()
            ),
            "shadowed-by-better-capability covers tools fired but absent from MCP registry (legacy aliases) and flows fired but missing YAML.".to_string(),
            "protected list mirrors capability-evolution-governance policy + bootstrap/repair surfaces.".to_string(),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    replacement_target: Option<String>,
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

    // Resolve replacement_target for merge decisions.
    let arg_target = args
        .get("replacement_target")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut replacement_target: Option<String> = arg_target.clone();
    if decision == "merge" {
        if replacement_target.is_none() {
            let hint_index = HintIndex::from_project_root(&project_root);
            let hints = hint_index.for_source(kind, candidate_id);
            let registered_tools_set: BTreeSet<String> =
                registered_tools().into_iter().collect();
            let registered_flows_set: BTreeSet<String> =
                registered_flows().into_iter().collect();
            if let Some(hint) = pick_merge_target(
                &hints,
                candidate_id,
                &registered_tools_set,
                &registered_flows_set,
            ) {
                replacement_target = hint.target_id.clone();
            }
        }
        let Some(target) = replacement_target.as_deref() else {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    format!(
                        "decision=merge for `{}` requires `replacement_target` (no lisp hint resolves a target)",
                        candidate_id
                    ),
                )
                .with_suggestion(
                    "supply `replacement_target` arg or add an explicit lisp hint (deprecated/moved-to/replacement/supersedes)",
                ),
            ));
        };
        let target_protected = match kind {
            "flow" => is_protected_flow(target),
            _ => is_protected_tool(target),
        };
        if target_protected {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    "PROTECTED_CAPABILITY",
                    format!(
                        "replacement_target `{}` is protected; mark `{}` as review/monitor/keep instead",
                        target, candidate_id
                    ),
                )
                .with_suggestion("merging into a protected capability would re-route load to a guarded surface"),
            ));
        }
        if target == candidate_id {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("replacement_target == candidate_id == `{}`", candidate_id),
                ),
            ));
        }
    }

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
        replacement_target: replacement_target.clone(),
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
// Lisp semantic hint parser
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct SemanticHint {
    source_id: String,
    source_kind: String,
    token: String,
    target_id: Option<String>,
    target_kind: String,
    target_hint: Option<String>,
    file_anchor: String,
    raw_snippet: String,
}

#[derive(Debug, Default, Clone)]
struct HintIndex {
    by_source: HashMap<(String, String), Vec<SemanticHint>>,
    all: Vec<SemanticHint>,
    files_read: Vec<String>,
}

impl HintIndex {
    fn from_project_root(project_root: &Path) -> Self {
        let files = [
            ("intent-tools.lisp", "tool"),
            ("intent-flow.lisp", "flow"),
            ("intent-intent-layer.lisp", "policy"),
        ];
        let mut idx = HintIndex::default();
        for (fname, default_kind) in files {
            let path = project_root.join(".missiond").join("v2").join(fname);
            if let Ok(text) = std::fs::read_to_string(&path) {
                idx.files_read.push(fname.to_string());
                let hints = parse_lisp_hints(&text, fname, default_kind);
                for h in hints {
                    idx.all.push(h);
                }
            }
        }
        idx.resolve_targets();
        idx.rebuild_by_source();
        idx
    }

    fn rebuild_by_source(&mut self) {
        self.by_source.clear();
        for h in &self.all {
            self.by_source
                .entry((h.source_kind.clone(), h.source_id.clone()))
                .or_default()
                .push(h.clone());
        }
    }

    fn resolve_targets(&mut self) {
        let tools: Vec<String> = registered_tools();
        let flows: Vec<String> = registered_flows();
        self.resolve_targets_against(&tools, &flows);
    }

    fn resolve_targets_against(&mut self, tools: &[String], flows: &[String]) {
        for h in self.all.iter_mut() {
            if h.target_id.is_some() {
                continue;
            }
            let Some(hint) = h.target_hint.as_deref() else {
                continue;
            };
            let trimmed = hint.trim();
            if trimmed.is_empty() {
                continue;
            }
            let candidates: &[String] = match h.target_kind.as_str() {
                "flow" => flows,
                _ => tools,
            };
            let hits: Vec<&String> = candidates
                .iter()
                .filter(|name| name.contains(trimmed))
                .collect();
            if hits.len() == 1 {
                h.target_id = Some(hits[0].clone());
            }
        }
    }

    fn for_source(&self, kind: &str, id: &str) -> Vec<&SemanticHint> {
        self.by_source
            .get(&(kind.to_string(), id.to_string()))
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }
}

fn parse_lisp_hints(text: &str, file_label: &str, default_kind: &str) -> Vec<SemanticHint> {
    use regex::Regex;
    use std::sync::OnceLock;

    static TOOL_OPEN: OnceLock<Regex> = OnceLock::new();
    static FLOW_DEFINED: OnceLock<Regex> = OnceLock::new();
    static FUTURE_BLOCK: OnceLock<Regex> = OnceLock::new();
    static STATUS_DEPRECATED: OnceLock<Regex> = OnceLock::new();
    static MOVED_TO: OnceLock<Regex> = OnceLock::new();
    static SPLIT_DECISION: OnceLock<Regex> = OnceLock::new();
    static DISPATCH_PREFERRED: OnceLock<Regex> = OnceLock::new();
    static REPLACEMENT_KV: OnceLock<Regex> = OnceLock::new();
    static SUPERSEDES_KV: OnceLock<Regex> = OnceLock::new();
    static F_FLOW_REF: OnceLock<Regex> = OnceLock::new();

    let tool_open =
        TOOL_OPEN.get_or_init(|| Regex::new(r"^\s*\(tool\s+([A-Za-z][A-Za-z0-9_]+)").unwrap());
    let flow_def = FLOW_DEFINED
        .get_or_init(|| Regex::new(r"^\s*\(flow\s+(F-[A-Za-z][A-Za-z0-9_-]+)").unwrap());
    let future_block =
        FUTURE_BLOCK.get_or_init(|| Regex::new(r"^\s{4,}\(([a-z][a-z0-9-]+)\s*$").unwrap());
    let status_dep = STATUS_DEPRECATED
        .get_or_init(|| Regex::new(r#":status\s+"(deprecated[A-Za-z0-9_-]*)""#).unwrap());
    let moved_to = MOVED_TO.get_or_init(|| Regex::new(r#":moved-to\s+"([^"]+)""#).unwrap());
    let split_keep = SPLIT_DECISION
        .get_or_init(|| Regex::new(r#":split-decision\s+"keep consolidated[^"]*""#).unwrap());
    let dispatch_pref = DISPATCH_PREFERRED.get_or_init(|| {
        Regex::new(r#":workstation-dispatch-role\s+"(preferred[^"]*)""#).unwrap()
    });
    let replacement_kv = REPLACEMENT_KV
        .get_or_init(|| Regex::new(r#":replacement\s+"([A-Za-z0-9_-]+)""#).unwrap());
    let supersedes_kv = SUPERSEDES_KV
        .get_or_init(|| Regex::new(r#":supersedes\s+"([A-Za-z0-9_-]+)""#).unwrap());
    let f_flow_ref =
        F_FLOW_REF.get_or_init(|| Regex::new(r"(F-[A-Za-z][A-Za-z0-9_-]+)").unwrap());

    let mut out: Vec<SemanticHint> = Vec::new();
    let mut current: Option<(String, String)> = None;

    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        if let Some(caps) = tool_open.captures(line) {
            current = Some(("tool".to_string(), caps[1].to_string()));
            continue;
        }
        if let Some(caps) = flow_def.captures(line) {
            current = Some(("flow".to_string(), caps[1].to_string()));
            continue;
        }
        if default_kind == "flow" {
            if let Some(caps) = future_block.captures(line) {
                current = Some(("flow".to_string(), caps[1].to_string()));
                continue;
            }
        }
        let Some((ref kind, ref source_id)) = current else {
            continue;
        };
        let anchor = format!("{}:{}", file_label, line_no);
        if let Some(caps) = status_dep.captures(line) {
            let status_str = caps[1].to_string();
            let target_hint = status_str
                .strip_prefix("deprecated-migrated-to-")
                .or_else(|| status_str.strip_prefix("deprecated-by-"))
                .map(|s| s.to_string());
            out.push(SemanticHint {
                source_id: source_id.clone(),
                source_kind: kind.clone(),
                token: "deprecated".to_string(),
                target_id: None,
                target_kind: kind.clone(),
                target_hint,
                file_anchor: anchor.clone(),
                raw_snippet: trim_snippet(line),
            });
        }
        if let Some(caps) = moved_to.captures(line) {
            let blob = caps[1].to_string();
            let target_id = f_flow_ref.captures(&blob).map(|c| c[1].to_string());
            out.push(SemanticHint {
                source_id: source_id.clone(),
                source_kind: kind.clone(),
                token: "moved-to".to_string(),
                target_id,
                target_kind: "flow".to_string(),
                target_hint: Some(blob),
                file_anchor: anchor.clone(),
                raw_snippet: trim_snippet(line),
            });
        }
        if split_keep.is_match(line) {
            out.push(SemanticHint {
                source_id: source_id.clone(),
                source_kind: kind.clone(),
                token: "consolidated-keep".to_string(),
                target_id: None,
                target_kind: "tool".to_string(),
                target_hint: None,
                file_anchor: anchor.clone(),
                raw_snippet: trim_snippet(line),
            });
        }
        if let Some(caps) = dispatch_pref.captures(line) {
            out.push(SemanticHint {
                source_id: source_id.clone(),
                source_kind: kind.clone(),
                token: "preferred-substrate".to_string(),
                target_id: None,
                target_kind: "tool".to_string(),
                target_hint: Some(caps[1].to_string()),
                file_anchor: anchor.clone(),
                raw_snippet: trim_snippet(line),
            });
        }
        if let Some(caps) = replacement_kv.captures(line) {
            out.push(SemanticHint {
                source_id: source_id.clone(),
                source_kind: kind.clone(),
                token: "replacement".to_string(),
                target_id: Some(caps[1].to_string()),
                target_kind: kind.clone(),
                target_hint: None,
                file_anchor: anchor.clone(),
                raw_snippet: trim_snippet(line),
            });
        }
        if let Some(caps) = supersedes_kv.captures(line) {
            out.push(SemanticHint {
                source_id: source_id.clone(),
                source_kind: kind.clone(),
                token: "supersedes".to_string(),
                target_id: Some(caps[1].to_string()),
                target_kind: kind.clone(),
                target_hint: None,
                file_anchor: anchor.clone(),
                raw_snippet: trim_snippet(line),
            });
        }
    }
    out
}

fn trim_snippet(line: &str) -> String {
    let s = line.trim();
    if s.chars().count() <= 120 {
        return s.to_string();
    }
    let mut acc = String::new();
    for (i, c) in s.chars().enumerate() {
        if i >= 117 {
            break;
        }
        acc.push(c);
    }
    acc.push_str("...");
    acc
}

fn pick_merge_target<'a>(
    hints: &'a [&'a SemanticHint],
    source_id: &str,
    registered_tools: &BTreeSet<String>,
    registered_flows: &BTreeSet<String>,
) -> Option<&'a SemanticHint> {
    for h in hints {
        let target = match (h.token.as_str(), h.target_id.as_deref()) {
            ("deprecated", Some(t))
            | ("moved-to", Some(t))
            | ("replacement", Some(t))
            | ("supersedes", Some(t)) => Some(t.to_string()),
            _ => None,
        };
        let Some(target) = target else { continue };
        if target == source_id {
            continue;
        }
        let registered = match h.target_kind.as_str() {
            "flow" => registered_flows.contains(&target),
            _ => registered_tools.contains(&target),
        };
        if !registered {
            continue;
        }
        return Some(*h);
    }
    None
}

fn render_hint_evidence(hints: &[&SemanticHint]) -> Vec<String> {
    hints
        .iter()
        .map(|h| {
            let target = h
                .target_id
                .clone()
                .or_else(|| h.target_hint.clone())
                .unwrap_or_else(|| "<unresolved>".to_string());
            format!("{} {} -> {}", h.file_anchor, h.token, target)
        })
        .collect()
}

fn apply_hint_overlay(
    base_class: &str,
    base_evidence: Vec<String>,
    source_id: &str,
    hints: &[&SemanticHint],
    registered_tools: &BTreeSet<String>,
    registered_flows: &BTreeSet<String>,
) -> (String, Vec<String>, Option<String>, Vec<String>) {
    if hints.is_empty() {
        return (base_class.to_string(), base_evidence, None, Vec::new());
    }
    let hint_lines = render_hint_evidence(hints);
    let mut evidence = base_evidence;
    if base_class == "active" || base_class == "protected" {
        return (base_class.to_string(), evidence, None, hint_lines);
    }
    if hints.iter().any(|h| h.token == "consolidated-keep") {
        evidence.push("lisp says `consolidated-keep`: suppress merge candidacy".to_string());
        return (base_class.to_string(), evidence, None, hint_lines);
    }
    let Some(target_hint) =
        pick_merge_target(hints, source_id, registered_tools, registered_flows)
    else {
        evidence.push("semantic_hint_missing_target".to_string());
        return (base_class.to_string(), evidence, None, hint_lines);
    };
    let target_id = target_hint
        .target_id
        .as_deref()
        .unwrap_or("")
        .to_string();
    let target_kind = target_hint.target_kind.clone();
    let target_protected = match target_kind.as_str() {
        "flow" => is_protected_flow(&target_id),
        _ => is_protected_tool(&target_id),
    };
    evidence.push(format!(
        "merge-candidate: {} {} {} -> {}",
        target_hint.file_anchor, target_hint.token, source_id, target_id
    ));
    if target_protected {
        evidence.push(format!(
            "target `{}` is protected; mark only as review, never destructive",
            target_id
        ));
    }
    (
        "merge-candidate".to_string(),
        evidence,
        Some(target_id),
        hint_lines,
    )
}

// ───────────────────────────────────────────────────────────────────────
// event_log flow probe
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
struct EventLogFlowProbe {
    counts: HashMap<String, i64>,
    events_scanned: i64,
    events_with_template: i64,
    error: Option<String>,
}

async fn probe_event_log_flows(state: &AppState) -> EventLogFlowProbe {
    let mut probe = EventLogFlowProbe::default();
    let since = (Utc::now() - Duration::days(90))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let result = state
        .store
        .query_timeline_filtered(
            Some("board::task_created"),
            None,
            Some(since.as_str()),
            None,
            500,
            0,
        )
        .await;
    let rows = match result {
        Ok(rows) => rows,
        Err(e) => {
            probe.error = Some(format!("event_log probe failed: {}", e));
            return probe;
        }
    };
    probe.events_scanned = rows.len() as i64;
    for r in rows {
        let payload: Value = match serde_json::from_str(&r.payload) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let template = payload
            .get("flow_template")
            .and_then(|v| v.as_str())
            .or_else(|| payload.get("template").and_then(|v| v.as_str()))
            .or_else(|| {
                payload
                    .pointer("/task/flow_template")
                    .and_then(|v| v.as_str())
            });
        if let Some(t) = template {
            probe.events_with_template += 1;
            *probe.counts.entry(t.to_string()).or_insert(0) += 1;
        }
    }
    probe
}

// ───────────────────────────────────────────────────────────────────────
// workflow_execution_stats lane
// ───────────────────────────────────────────────────────────────────────

/// Per-workflow stats lifted from the `workflow` table. Counts come straight
/// from `workflow.executions / success_count / avg_cost_usd / last_used_at`.
/// `failure_count` is derived (`executions - success_count`).
#[derive(Debug, Clone)]
struct WorkflowExecutionStats {
    name: String,
    executions: i64,
    success_count: i64,
    failure_count: i64,
    avg_cost_usd: Option<f64>,
    last_used_at: Option<String>,
}

/// Aggregated read of the `workflow` table.
///
/// `by_name` maps workflow.name → stats (exact match against flow id).
/// `by_match_reference` maps a flow id mentioned in workflow.match_rules
/// (`tool_calls` / `flows` arrays, exact strings only — never substring) to
/// the list of stats whose match_rules referenced it.
///
/// Failure to query the table is captured in `error`; both maps remain empty
/// in that case so the main snapshot pipeline does not stall.
#[derive(Debug, Default, Clone)]
struct WorkflowStatsProbe {
    by_name: HashMap<String, WorkflowExecutionStats>,
    by_match_reference: HashMap<String, Vec<WorkflowExecutionStats>>,
    workflows_observed: i64,
    error: Option<String>,
}

/// Maximum rows pulled from `workflow_list_top_n`. The table is small (a few
/// hundred rows in mature deployments) and `top_n` orders by usage so this
/// covers the live ranking without paginating.
const WORKFLOW_PROBE_LIMIT: i64 = 500;

async fn probe_workflow_execution_stats(state: &AppState) -> WorkflowStatsProbe {
    let mut probe = WorkflowStatsProbe::default();
    let rows = match state.store.workflow_list_top_n(WORKFLOW_PROBE_LIMIT).await {
        Ok(rs) => rs,
        Err(e) => {
            probe.error = Some(format!("DirectiveLayerStore::workflow_list_top_n failed: {}", e));
            return probe;
        }
    };
    probe.workflows_observed = rows.len() as i64;
    for w in rows {
        let executions = w.executions as i64;
        let success_count = w.success_count as i64;
        let failure_count = (executions - success_count).max(0);
        let stats = WorkflowExecutionStats {
            name: w.name.clone(),
            executions,
            success_count,
            failure_count,
            avg_cost_usd: w.avg_cost_usd,
            last_used_at: w.last_used_at.map(iso),
        };
        // Exact-name lane: workflow.name → stats.
        probe.by_name.insert(w.name.clone(), stats.clone());
        // match_rules.tool_calls + match_rules.flows array references (exact
        // string match only — no substring, no case folding). Anything outside
        // those well-known keys is intentionally ignored to keep the mapping
        // deterministic.
        for ref_id in extract_match_references(&w.match_rules) {
            probe
                .by_match_reference
                .entry(ref_id)
                .or_default()
                .push(stats.clone());
        }
    }
    probe
}

/// Pull exact id references out of a workflow.match_rules JSONB blob.
///
/// Only honours the `tool_calls` and `flows` arrays at the root, with each
/// entry treated as a verbatim string. Any nested or fuzzy-text fields are
/// ignored. This mirrors the lane contract: exact-name mapping only.
fn extract_match_references(match_rules: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(obj) = match_rules.as_object() else {
        return out;
    };
    for key in &["tool_calls", "flows"] {
        let Some(arr) = obj.get(*key).and_then(|v| v.as_array()) else {
            continue;
        };
        for item in arr {
            if let Some(s) = item.as_str() {
                if !s.is_empty() {
                    out.push(s.to_string());
                }
            }
        }
    }
    out
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
                replacement_target: None,
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

    #[test]
    fn lisp_parser_extracts_explicit_replacement_token() {
        let snippet = r#"
  (tool mission_old_proc
    :desc "deprecated"
    :replacement "mission_new_proc")
"#;
        let hints = parse_lisp_hints(snippet, "intent-tools.lisp", "tool");
        let kept: Vec<_> = hints.iter().filter(|h| h.token == "replacement").collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].source_id, "mission_old_proc");
        assert_eq!(kept[0].target_id.as_deref(), Some("mission_new_proc"));
    }

    #[test]
    fn lisp_parser_extracts_moved_to_flow_target() {
        let snippet = r#"
  (flow F-old-flow
    :desc "old"
    :status "moved-to named flow"
    :moved-to "category workflow-runtime-flows :: F-new-flow")
"#;
        let hints = parse_lisp_hints(snippet, "intent-flow.lisp", "flow");
        let moved: Vec<_> = hints.iter().filter(|h| h.token == "moved-to").collect();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].source_id, "F-old-flow");
        assert_eq!(moved[0].target_id.as_deref(), Some("F-new-flow"));
        assert_eq!(moved[0].target_kind, "flow");
    }

    #[test]
    fn lisp_parser_ignores_fuzzy_text_without_target() {
        let snippet = r#"
  (tool mission_freeform
    :desc "Eventually we'll migrate this to a different module, but no replacement key here")
"#;
        let hints = parse_lisp_hints(snippet, "intent-tools.lisp", "tool");
        assert!(
            hints.is_empty(),
            "fuzzy desc text must not generate hints; got: {:?}",
            hints
        );
    }

    #[test]
    fn lisp_parser_emits_unresolved_when_target_hint_missing() {
        let snippet = r#"
  (tool mission_orphan
    :status "deprecated")
"#;
        let hints = parse_lisp_hints(snippet, "intent-tools.lisp", "tool");
        let dep: Vec<_> = hints.iter().filter(|h| h.token == "deprecated").collect();
        assert_eq!(dep.len(), 1);
        assert_eq!(dep[0].target_hint, None);
        assert_eq!(dep[0].target_id, None);
    }

    #[test]
    fn pick_merge_target_skips_self_and_unregistered() {
        let hints_owned = vec![
            SemanticHint {
                source_id: "mission_a".into(),
                source_kind: "tool".into(),
                token: "replacement".into(),
                target_id: Some("mission_a".into()),
                target_kind: "tool".into(),
                target_hint: None,
                file_anchor: "x:1".into(),
                raw_snippet: "".into(),
            },
            SemanticHint {
                source_id: "mission_a".into(),
                source_kind: "tool".into(),
                token: "replacement".into(),
                target_id: Some("mission_unregistered".into()),
                target_kind: "tool".into(),
                target_hint: None,
                file_anchor: "x:2".into(),
                raw_snippet: "".into(),
            },
            SemanticHint {
                source_id: "mission_a".into(),
                source_kind: "tool".into(),
                token: "replacement".into(),
                target_id: Some("mission_b".into()),
                target_kind: "tool".into(),
                target_hint: None,
                file_anchor: "x:3".into(),
                raw_snippet: "".into(),
            },
        ];
        let hints: Vec<&SemanticHint> = hints_owned.iter().collect();
        let mut tools = BTreeSet::new();
        tools.insert("mission_a".to_string());
        tools.insert("mission_b".to_string());
        let flows = BTreeSet::new();
        let picked = pick_merge_target(&hints, "mission_a", &tools, &flows).unwrap();
        assert_eq!(picked.target_id.as_deref(), Some("mission_b"));
    }

    #[test]
    fn apply_overlay_promotes_quiet_to_merge_candidate_with_resolved_target() {
        let hint = SemanticHint {
            source_id: "mission_old".into(),
            source_kind: "tool".into(),
            token: "deprecated".into(),
            target_id: Some("mission_new".into()),
            target_kind: "tool".into(),
            target_hint: None,
            file_anchor: "intent-tools.lisp:42".into(),
            raw_snippet: "".into(),
        };
        let hint_refs: Vec<&SemanticHint> = vec![&hint];
        let mut tools = BTreeSet::new();
        tools.insert("mission_new".to_string());
        let flows = BTreeSet::new();
        let (cls, evidence, target, hint_evidence) = apply_hint_overlay(
            "quiet",
            vec!["count_30d=0".into()],
            "mission_old",
            &hint_refs,
            &tools,
            &flows,
        );
        assert_eq!(cls, "merge-candidate");
        assert_eq!(target.as_deref(), Some("mission_new"));
        assert!(evidence.iter().any(|e| e.contains("merge-candidate:")));
        assert_eq!(hint_evidence.len(), 1);
    }

    #[test]
    fn apply_overlay_keeps_active_unchanged() {
        let hint = SemanticHint {
            source_id: "mission_x".into(),
            source_kind: "tool".into(),
            token: "deprecated".into(),
            target_id: Some("mission_y".into()),
            target_kind: "tool".into(),
            target_hint: None,
            file_anchor: "x:1".into(),
            raw_snippet: "".into(),
        };
        let hint_refs: Vec<&SemanticHint> = vec![&hint];
        let mut tools = BTreeSet::new();
        tools.insert("mission_y".to_string());
        let flows = BTreeSet::new();
        let (cls, _, target, _) = apply_hint_overlay(
            "active",
            vec![],
            "mission_x",
            &hint_refs,
            &tools,
            &flows,
        );
        assert_eq!(cls, "active");
        assert_eq!(target, None);
    }

    #[test]
    fn apply_overlay_protected_target_keeps_merge_with_review_evidence() {
        let hint = SemanticHint {
            source_id: "mission_legacy".into(),
            source_kind: "tool".into(),
            token: "replacement".into(),
            target_id: Some("mission_audit".into()),
            target_kind: "tool".into(),
            target_hint: None,
            file_anchor: "x:1".into(),
            raw_snippet: "".into(),
        };
        let hint_refs: Vec<&SemanticHint> = vec![&hint];
        let mut tools = BTreeSet::new();
        tools.insert("mission_audit".to_string());
        let flows = BTreeSet::new();
        let (cls, evidence, target, _) = apply_hint_overlay(
            "stale",
            vec![],
            "mission_legacy",
            &hint_refs,
            &tools,
            &flows,
        );
        assert_eq!(cls, "merge-candidate");
        assert_eq!(target.as_deref(), Some("mission_audit"));
        assert!(
            evidence.iter().any(|e| e.contains("protected; mark only as review")),
            "evidence should warn about protected target; got {:?}",
            evidence
        );
    }

    #[test]
    fn apply_overlay_emits_missing_target_when_hint_unresolved() {
        let hint = SemanticHint {
            source_id: "mission_old".into(),
            source_kind: "tool".into(),
            token: "deprecated".into(),
            target_id: None,
            target_kind: "tool".into(),
            target_hint: Some("nonexistent-suffix".into()),
            file_anchor: "x:1".into(),
            raw_snippet: "".into(),
        };
        let hint_refs: Vec<&SemanticHint> = vec![&hint];
        let tools = BTreeSet::new();
        let flows = BTreeSet::new();
        let (cls, evidence, target, hint_evidence) = apply_hint_overlay(
            "stale",
            vec!["count_all=0".into()],
            "mission_old",
            &hint_refs,
            &tools,
            &flows,
        );
        assert_eq!(cls, "stale");
        assert_eq!(target, None);
        assert!(evidence.iter().any(|e| e == "semantic_hint_missing_target"));
        assert_eq!(hint_evidence.len(), 1);
    }

    #[test]
    fn apply_overlay_consolidated_keep_suppresses_merge_candidacy() {
        let hint_keep = SemanticHint {
            source_id: "mission_kb_ops".into(),
            source_kind: "tool".into(),
            token: "consolidated-keep".into(),
            target_id: None,
            target_kind: "tool".into(),
            target_hint: None,
            file_anchor: "intent-tools.lisp:1203".into(),
            raw_snippet: "".into(),
        };
        let hint_dep = SemanticHint {
            source_id: "mission_kb_ops".into(),
            source_kind: "tool".into(),
            token: "deprecated".into(),
            target_id: Some("mission_other".into()),
            target_kind: "tool".into(),
            target_hint: None,
            file_anchor: "intent-tools.lisp:1300".into(),
            raw_snippet: "".into(),
        };
        let hint_refs: Vec<&SemanticHint> = vec![&hint_keep, &hint_dep];
        let mut tools = BTreeSet::new();
        tools.insert("mission_other".to_string());
        let flows = BTreeSet::new();
        let (cls, evidence, target, _) = apply_hint_overlay(
            "quiet",
            vec![],
            "mission_kb_ops",
            &hint_refs,
            &tools,
            &flows,
        );
        assert_eq!(cls, "quiet");
        assert_eq!(target, None);
        assert!(evidence.iter().any(|e| e.contains("consolidated-keep")));
    }

    #[test]
    fn build_source_coverage_dict_includes_all_six_lanes() {
        let mut hint_index = HintIndex::default();
        hint_index.files_read.push("intent-tools.lisp".into());
        hint_index.all.push(SemanticHint {
            source_id: "x".into(),
            source_kind: "tool".into(),
            token: "deprecated".into(),
            target_id: None,
            target_kind: "tool".into(),
            target_hint: None,
            file_anchor: "intent-tools.lisp:1".into(),
            raw_snippet: "".into(),
        });
        let probe = EventLogFlowProbe::default();
        let workflow_probe = WorkflowStatsProbe::default();
        let dict = build_source_coverage_dict(&[], &hint_index, &probe, &workflow_probe, Some(0));
        for key in &[
            "conversation_tool_calls",
            "board_tasks_flow_template",
            "event_log_flow_events",
            "lisp_semantic_hints",
            "review_sidecar",
            "workflow_execution_stats",
        ] {
            assert!(
                dict.contains_key(*key),
                "source_coverage missing lane `{}`; have keys {:?}",
                key,
                dict.keys().collect::<Vec<_>>()
            );
        }
        assert_eq!(dict.get("lisp_semantic_hints").unwrap().count, 1);
    }

    #[test]
    fn workflow_stats_lane_unavailable_when_probe_errors() {
        let hint_index = HintIndex::default();
        let event_probe = EventLogFlowProbe::default();
        let workflow_probe = WorkflowStatsProbe {
            by_name: HashMap::new(),
            by_match_reference: HashMap::new(),
            workflows_observed: 0,
            error: Some("connection refused".to_string()),
        };
        let dict = build_source_coverage_dict(
            &[],
            &hint_index,
            &event_probe,
            &workflow_probe,
            Some(0),
        );
        let lane = dict
            .get("workflow_execution_stats")
            .expect("workflow_execution_stats lane present even on probe failure");
        assert_eq!(lane.status, "unavailable");
        assert_eq!(lane.count, 0);
        assert!(
            lane.note.contains("connection refused"),
            "note should surface upstream error; got `{}`",
            lane.note
        );
    }

    #[test]
    fn workflow_stats_lane_active_when_rows_present() {
        let hint_index = HintIndex::default();
        let event_probe = EventLogFlowProbe::default();
        let stats = WorkflowExecutionStats {
            name: "F-some-flow".into(),
            executions: 5,
            success_count: 4,
            failure_count: 1,
            avg_cost_usd: Some(0.1234),
            last_used_at: Some("2026-04-25T00:00:00Z".into()),
        };
        let mut by_name = HashMap::new();
        by_name.insert("F-some-flow".to_string(), stats.clone());
        let workflow_probe = WorkflowStatsProbe {
            by_name,
            by_match_reference: HashMap::new(),
            workflows_observed: 1,
            error: None,
        };
        let dict = build_source_coverage_dict(
            &[],
            &hint_index,
            &event_probe,
            &workflow_probe,
            Some(0),
        );
        let lane = dict.get("workflow_execution_stats").unwrap();
        assert_eq!(lane.status, "active");
        assert_eq!(lane.count, 1);
        assert!(
            lane.note.contains("scanned 1 workflow rows"),
            "note: {}",
            lane.note
        );
    }

    #[test]
    fn workflow_stats_lane_empty_when_no_rows() {
        let hint_index = HintIndex::default();
        let event_probe = EventLogFlowProbe::default();
        let workflow_probe = WorkflowStatsProbe {
            by_name: HashMap::new(),
            by_match_reference: HashMap::new(),
            workflows_observed: 0,
            error: None,
        };
        let dict = build_source_coverage_dict(
            &[],
            &hint_index,
            &event_probe,
            &workflow_probe,
            None,
        );
        let lane = dict.get("workflow_execution_stats").unwrap();
        assert_eq!(lane.status, "empty");
        assert_eq!(lane.count, 0);
    }

    #[test]
    fn extract_match_references_pulls_exact_strings_only() {
        let v = json!({
            "tool_calls": ["mission_router_chat", "mission_kb_query"],
            "flows": ["F-some-flow"],
            "free_text": "this should be ignored",
            "nested": {"tool_calls": ["mission_ignored"]}
        });
        let refs = extract_match_references(&v);
        assert!(refs.contains(&"mission_router_chat".to_string()));
        assert!(refs.contains(&"mission_kb_query".to_string()));
        assert!(refs.contains(&"F-some-flow".to_string()));
        assert!(
            !refs.contains(&"mission_ignored".to_string()),
            "nested keys must not leak: {:?}",
            refs
        );
        assert!(
            !refs.iter().any(|s| s.contains("ignored")),
            "free text must not leak: {:?}",
            refs
        );
    }

    #[test]
    fn extract_match_references_ignores_non_object_or_missing_keys() {
        let v = json!([]);
        assert!(extract_match_references(&v).is_empty());
        let v = json!({"unrelated": 1});
        assert!(extract_match_references(&v).is_empty());
        let v = json!({"tool_calls": "not-an-array"});
        assert!(extract_match_references(&v).is_empty());
        let v = json!({"tool_calls": [42, null, ""]});
        // numeric / null / empty entries must be skipped, not stringified
        assert!(extract_match_references(&v).is_empty());
    }

    #[test]
    fn workflow_probe_lookup_rejects_substring_and_case_variants() {
        // The lane contract says exact-name only; validate the lookup side
        // (the probe builder is store-backed and gets a separate functional
        // test once a fake store lands).
        let stats = WorkflowExecutionStats {
            name: "F-canonical".into(),
            executions: 3,
            success_count: 3,
            failure_count: 0,
            avg_cost_usd: None,
            last_used_at: None,
        };
        let mut by_name = HashMap::new();
        by_name.insert("F-canonical".to_string(), stats);
        let probe = WorkflowStatsProbe {
            by_name,
            by_match_reference: HashMap::new(),
            workflows_observed: 1,
            error: None,
        };
        // Substring lookup must miss.
        assert!(probe.by_name.get("F-canonical-extended").is_none());
        // Case-insensitive lookup must miss (HashMap<String,_> is exact).
        assert!(probe.by_name.get("f-canonical").is_none());
        // Exact lookup hits.
        assert!(probe.by_name.get("F-canonical").is_some());
    }
}
