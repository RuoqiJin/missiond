use super::ResolvedExec;
use missiond_core::types::Plan;
use missiond_mcp::tools::{error_codes, ToolContent, ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

mod schema_parser;

pub(super) fn parse_backend_registry(input: &str) -> Result<Vec<BackendEntry>, String> {
    schema_parser::parse_backend_registry(input)
}

pub(super) fn parse_router_policy(input: &str) -> Result<PolicyDoc, String> {
    schema_parser::parse_router_policy(input)
}

/// Default policy file. Mirrors the wave24-01 seed location and the
/// wave24-03 CLI's documented default. Resolved relative to the
/// process CWD when the caller passes only `router_policy_mode=dry_run`
/// without `router_policy_path`.
pub(super) const DEFAULT_POLICY_PATH: &str = ".missiond/router/router-policy-v1.lisp";

/// Schema label embedded in every emitted recommendation block. Mirrors
/// the wave24-03 CLI so downstream consumers can verify the wire shape.
pub(super) const SCHEMA: &str = "missiond.router-recommendation.v0";

/// Fallback backend used when no rule matches. The wave24-03 CLI
/// surfaces the exact same value + reason — keeping these literals in
/// sync is part of the cross-wave contract.
pub(super) const FALLBACK_BACKEND: &str = "claudecode";
pub(super) const FALLBACK_REASON: &str = "insufficient_trace_history";

/// Backend enum (mirrors wave24-01 schema). Anything outside this set
/// is surfaced verbatim in the recommendation block but flagged as
/// `status="rejected"` (the wave24-01 checker rejects unknown backends
/// at validation time; the daemon re-checks defensively).
const KNOWN_BACKENDS: &[&str] = &[
    "claudecode",
    "missiond-llm-router",
    "deterministic-checker",
    "patch-worker",
    "verifier-worker",
];

/// Recognised top-level `router_policy_mode` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RouterPolicyMode {
    /// Default. The recommendation block is NOT emitted; the response
    /// is byte-identical to the wave-15..23 baseline.
    Off,
    /// Compute the recommendation and emit `applied=false`. Never
    /// changes target / dispatch_strategy / workstation_dispatch.
    DryRun,
}

/// Parse the optional `router_policy_mode` arg. Returns `Off` when the
/// arg is absent or the literal string `"off"`. Returns a structured
/// `INVALID_PARAM` error for any other value (including `apply`,
/// `auto`, and unknown strings) so a typo cannot silently route the
/// recommendation through an unimplemented surface.
pub(super) fn parse_router_policy_mode(args: &Value) -> Result<RouterPolicyMode, ToolResult> {
    let raw = match args.get("router_policy_mode") {
        None | Some(Value::Null) => return Ok(RouterPolicyMode::Off),
        Some(v) => v,
    };
    let s = match raw.as_str() {
        Some(s) => s.trim(),
        None => {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    "router_policy_mode must be a string",
                )
                .with_suggestion("expected one of: \"off\", \"dry_run\""),
            ));
        }
    };
    match s {
        "" | "off" => Ok(RouterPolicyMode::Off),
        "dry_run" => Ok(RouterPolicyMode::DryRun),
        other => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!(
                    "router_policy_mode `{}` is not supported in this surface (wave24-04 only ships `off` and `dry_run`)",
                    other
                ),
            )
            .with_suggestion(
                "expected one of: \"off\" (default; no recommendation block) or \"dry_run\" (informational block, applied=false)",
            ),
        )),
    }
}

/// Splice the recommendation block onto a successful response. No-op
/// when `mode=Off` so callers that never opted in observe the
/// wave-15..23 byte-shape. Errors are also passed through unchanged
/// — we never decorate a structured error with the recommendation
/// (the operator needs the error path uncluttered).
pub(super) fn attach_router_recommendation_block(
    mut result: ToolResult,
    mode: RouterPolicyMode,
    args: &Value,
    resolved: &ResolvedExec,
    plan: &Plan,
) -> ToolResult {
    if matches!(mode, RouterPolicyMode::Off) {
        return result;
    }
    if result.is_error.unwrap_or(false) {
        return result;
    }
    let block = compute_recommendation(args, resolved, plan);
    let Some(ToolContent::Text { text }) = result.content.first_mut() else {
        return result;
    };
    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
        return result;
    };
    if let Some(map) = value.as_object_mut() {
        // Never overwrite a pre-existing block — preserves any forward-
        // compatible attachment a downstream layer may add.
        map.entry("router_recommendation".to_string())
            .or_insert(block);
    }
    *text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.clone());
    result
}

/// Pure projection of the execute context into the predicate input.
/// Mirrors the wave24-03 CLI's task-projection: the live execute
/// `args` + resolved dispatch + plan status are treated like a
/// task-contract for predicate matching.
#[derive(Debug, Clone, Default)]
struct PredicateContext {
    kind: String,
    dispatch_strategy: String,
    owner: String,
    status: String,
    write_scope: Vec<String>,
}

fn project_context(args: &Value, resolved: &ResolvedExec, plan: &Plan) -> PredicateContext {
    let kind = arg_string(args, "kind").unwrap_or_default();
    // dispatch_strategy: prefer the resolved value (which already
    // reflects explicit_arg > plan_hint > parallelism > default
    // precedence). The wave24-03 CLI projects from the task contract
    // value; here the resolved value IS the live equivalent.
    let dispatch_strategy = resolved.dispatch_strategy.to_string();
    let owner = arg_string(args, "owner").unwrap_or_default();
    let status = arg_string(args, "status").unwrap_or_else(|| plan.status.as_str().to_string());
    let write_scope = arg_string_array(args, "owned_files");
    PredicateContext {
        kind,
        dispatch_strategy,
        owner,
        status,
        write_scope,
    }
}

fn arg_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn arg_string_array(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) if !s.trim().is_empty() => {
            // Tolerate single-string-as-array (mirrors many of the
            // wave-15+ workstation-dispatch knobs).
            vec![s.trim().to_string()]
        }
        _ => Vec::new(),
    }
}

/// Top-level entry: read + parse the policy, evaluate, and project
/// the response block. Always returns a serializable JSON object;
/// never panics; surfaces I/O / parse / invariant failures via the
/// `status` field (`computed` / `rejected` / `error`).
///
/// wave27-03: the OPTIONAL `router_dispatch_descriptor` arg, when
/// the JSON literal `true`, is honored ONLY here (i.e. inside the
/// dry_run code path; `attach_router_recommendation_block`'s Off
/// early-return predates this entry). When `BackendRegistryInfo` is
/// `Absent`, the recommendation block surfaces `descriptor_status =
/// "registry_missing"` and the descriptor body is OMITTED. Otherwise
/// a `router_dispatch_descriptor` sub-object is spliced onto the
/// recommendation block, projecting the wave27-01 schema fields off
/// the existing readiness/recommendation values plus three LOCKED
/// literal-bool invariants (`Value::Bool(true)` / `Value::Bool(false)`
/// — never strings, never computed) so the descriptor can never be
/// mis-read as a runtime promote signal.
fn compute_recommendation(args: &Value, resolved: &ResolvedExec, plan: &Plan) -> Value {
    let policy_path_input =
        arg_string(args, "router_policy_path").unwrap_or_else(|| DEFAULT_POLICY_PATH.to_string());
    // wave25-03: trace-index is OPTIONAL and read ONLY here (i.e. inside
    // dry_run). The Off branch in `attach_router_recommendation_block`
    // never calls into `compute_recommendation`, so this preserves the
    // "no file I/O when mode=off" invariant by construction.
    let trace_index_path_input = arg_string(args, "router_policy_trace_index_path");
    let trace_info = load_trace_index(trace_index_path_input.as_deref());
    // wave26-03: backend-readiness registry is OPTIONAL on the same Off-
    // gated path — same invariant applies (no file I/O when mode=off).
    let backend_registry_path_input = arg_string(args, "router_backend_registry_path");
    let registry_info = load_backend_registry(backend_registry_path_input.as_deref());
    let mut block = compute_recommendation_block(
        args,
        resolved,
        plan,
        &policy_path_input,
        &trace_info,
        &registry_info,
    );
    // wave27-03: optional descriptor surface. Always evaluated AFTER
    // the recommendation + readiness fields land so the descriptor
    // can simply project off them. `arg_bool` is strict — only the
    // JSON literal `true` opts in; absent / `false` / strings are
    // ignored. The descriptor branch performs ZERO additional file
    // I/O (registry / policy / trace-index were already loaded above
    // for the recommendation itself).
    if dispatch_descriptor_requested(args) {
        attach_router_dispatch_descriptor(&mut block, plan, &registry_info, &policy_path_input);
    }
    block
}

/// wave27-03: did the caller opt in to the dispatch descriptor surface?
/// Strict: only the JSON literal `true` returns `true`. Absent / `false`
/// / strings / numbers all return `false` so a typo can never
/// silently emit the descriptor.
fn dispatch_descriptor_requested(args: &Value) -> bool {
    matches!(
        args.get("router_dispatch_descriptor"),
        Some(Value::Bool(true))
    )
}

/// Internal: build the wave24-04 / wave25-03 / wave26-03 recommendation
/// block. Extracted so wave27-03 can post-process the block (splicing
/// the dispatch descriptor) without duplicating the policy / trace /
/// registry pre-loads.
fn compute_recommendation_block(
    args: &Value,
    resolved: &ResolvedExec,
    plan: &Plan,
    policy_path_input: &str,
    trace_info: &TraceIndexInfo,
    registry_info: &BackendRegistryInfo,
) -> Value {
    let resolved_path = resolve_policy_path(policy_path_input);
    let raw = match std::fs::read_to_string(&resolved_path) {
        Ok(s) => s,
        Err(e) => {
            return error_block(
                policy_path_input,
                &format!("read failed: {}", e),
                trace_info,
                registry_info,
                None,
                "low",
            );
        }
    };
    let policy = match parse_router_policy(&raw) {
        Ok(p) => p,
        Err(msg) => {
            return error_block(
                policy_path_input,
                &msg,
                trace_info,
                registry_info,
                None,
                "low",
            )
        }
    };
    if !policy.dry_run_only {
        return rejected_block(
            policy_path_input,
            "policy missing :dry-run-only true (cross-wave invariant: router output is advisory only)",
            trace_info,
            registry_info,
            None,
            "low",
        );
    }
    if policy.runtime_replacement {
        return rejected_block(
            policy_path_input,
            "policy declares :runtime-replacement true (cross-wave invariant: router output is advisory only)",
            trace_info,
            registry_info,
            None,
            "low",
        );
    }
    let ctx = project_context(args, resolved, plan);
    let mut matched: Vec<MatchedRule> = Vec::new();
    for rule in &policy.rules {
        if evaluate_clause(&rule.when, &ctx).is_ok() {
            matched.push(MatchedRule {
                id: rule.id.clone(),
                priority: rule.priority,
                reasoning: rule.reasoning.clone(),
            });
        }
    }
    // Rules already sorted by priority ascending — the parser does
    // this once on construction.
    if matched.is_empty() {
        // No-match: confidence is `low` regardless of trace-index
        // (mirrors the Node CLI's `if matched.length === 0 -> low`).
        return computed_block(
            policy_path_input,
            FALLBACK_BACKEND,
            "low",
            vec![format!("fallback (no rule matched): {}", FALLBACK_REASON)],
            trace_info,
            registry_info,
        );
    }
    let winner = &matched[0];
    let winner_rule = policy
        .rules
        .iter()
        .find(|r| r.id == winner.id)
        .expect("winner id must be present in policy.rules");
    let backend = winner_rule.backend.clone();
    let mut reasons: Vec<String> = matched
        .iter()
        .map(|m| {
            format!(
                "matched rule {} (priority {}): {}",
                m.id, m.priority, m.reasoning
            )
        })
        .collect();
    // Surface unknown backend defensively even though the wave24-01
    // checker already rejects this — the operator should see the
    // mismatch loud.
    if !KNOWN_BACKENDS.iter().any(|b| *b == backend) {
        reasons.push(format!(
            "warning: matched backend `{}` is not in the known v1 enum",
            backend
        ));
    }
    // wave25-03: confidence selection mirrors scripts/recommend-task-backend.mjs
    //   * trace-index supplied AND parsed ("used") AND
    //     max(by_task[plan.board_task_id].events, by_backend[backend].events) >= 5
    //     -> high
    //   * trace-index supplied AND parsed AND max in 1..=4 -> medium
    //   * trace-index supplied AND parsed AND max == 0 -> low
    //   * trace-index NOT supplied OR degraded (missing/unreadable/malformed) ->
    //     `medium` (the legacy wave24-04 default for matched outcomes).
    let confidence = match trace_info {
        TraceIndexInfo::Used {
            task_events,
            backend_events,
            ..
        } => {
            let task_events = events_for_task(task_events, &plan.board_task_id);
            let backend_events = events_for_backend(backend_events, &backend);
            let max = task_events.max(backend_events);
            if max >= RICH_TRACE_THRESHOLD {
                "high"
            } else if max >= 1 {
                "medium"
            } else {
                "low"
            }
        }
        // Degraded / absent: keep wave24-04 default of `medium` for matched.
        _ => "medium",
    };
    computed_block(
        policy_path_input,
        &backend,
        confidence,
        reasons,
        trace_info,
        registry_info,
    )
}

/// wave25-03: trace-index threshold mirrors scripts/recommend-task-backend.mjs
/// `RICH_TRACE_THRESHOLD = 5`. Keep this constant and the Node CLI in lock-
/// step. The threshold counts the MAX of per-task vs per-backend events.
const RICH_TRACE_THRESHOLD: u64 = 5;

/// wave25-03 trace-index status flavours surfaced on the recommendation
/// block. `Absent` is the default (no path supplied) and is observable on
/// the wire as the absence of `trace_index_*` fields.
#[derive(Debug, Clone)]
pub(super) enum TraceIndexInfo {
    /// No trace-index path was supplied. Block does NOT carry any
    /// `trace_index_*` fields (preserves wave24-04 byte-shape for
    /// callers that opted out).
    Absent,
    /// Path was supplied; file read + parsed; `by_task` / `by_backend`
    /// available for confidence scoring.
    Used {
        path: String,
        task_events: serde_json::Map<String, Value>,
        backend_events: serde_json::Map<String, Value>,
    },
    /// Path was supplied but the file does not exist on disk.
    Missing { path: String, warning: String },
    /// Path was supplied; std::fs::read_to_string returned an I/O error
    /// other than NotFound.
    Unreadable { path: String, warning: String },
    /// Path was supplied; serde_json failed to parse OR the top-level
    /// shape is not a JSON object.
    Malformed { path: String, warning: String },
}

fn load_trace_index(input: Option<&str>) -> TraceIndexInfo {
    let Some(path_str) = input else {
        return TraceIndexInfo::Absent;
    };
    let path = path_str.to_string();
    let resolved = resolve_policy_path(&path); // same resolution rule
    let raw = match std::fs::read_to_string(&resolved) {
        Ok(s) => s,
        Err(e) => {
            let warning = format!("trace-index read failed: {}", e);
            return if e.kind() == std::io::ErrorKind::NotFound {
                TraceIndexInfo::Missing { path, warning }
            } else {
                TraceIndexInfo::Unreadable { path, warning }
            };
        }
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return TraceIndexInfo::Malformed {
                path,
                warning: format!("trace-index JSON parse failed: {}", e),
            };
        }
    };
    let map = match value.as_object() {
        Some(m) => m,
        None => {
            return TraceIndexInfo::Malformed {
                path,
                warning: "trace-index top-level value is not a JSON object".to_string(),
            };
        }
    };
    let task_events = match map.get("by_task") {
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return TraceIndexInfo::Malformed {
                path,
                warning: "trace-index `by_task` is not a JSON object".to_string(),
            };
        }
        None => serde_json::Map::new(),
    };
    let backend_events = match map.get("by_backend") {
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return TraceIndexInfo::Malformed {
                path,
                warning: "trace-index `by_backend` is not a JSON object".to_string(),
            };
        }
        None => serde_json::Map::new(),
    };
    TraceIndexInfo::Used {
        path,
        task_events,
        backend_events,
    }
}

fn events_for_task(by_task: &serde_json::Map<String, Value>, task_id: &str) -> u64 {
    bucket_events(by_task, task_id)
}

fn events_for_backend(by_backend: &serde_json::Map<String, Value>, backend: &str) -> u64 {
    bucket_events(by_backend, backend)
}

fn bucket_events(map: &serde_json::Map<String, Value>, key: &str) -> u64 {
    map.get(key)
        .and_then(|v| v.get("events"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}

fn resolve_policy_path(input: &str) -> PathBuf {
    let p = Path::new(input);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        // The daemon runs out of the repo root in production; in
        // tests CWD points at the crate dir which still resolves
        // correctly because the policy path includes `.missiond/`.
        // Falling back to verbatim `input` here keeps the helper
        // free of repo-root-detection logic.
        PathBuf::from(input)
    }
}

fn error_block(
    policy_source: &str,
    message: &str,
    trace: &TraceIndexInfo,
    registry: &BackendRegistryInfo,
    recommended_backend: Option<&str>,
    confidence: &str,
) -> Value {
    let mut block = json!({
        "applied": false,
        "confidence": "low",
        "policy_source": policy_source,
        "reasons": [format!("error: {}", message)],
        "recommended_backend": FALLBACK_BACKEND,
        "schema": SCHEMA,
        "status": "error",
    });
    attach_trace_index_fields(&mut block, trace);
    attach_backend_readiness_fields(
        &mut block,
        registry,
        recommended_backend.unwrap_or(FALLBACK_BACKEND),
        "error",
        confidence,
    );
    block
}

fn rejected_block(
    policy_source: &str,
    message: &str,
    trace: &TraceIndexInfo,
    registry: &BackendRegistryInfo,
    recommended_backend: Option<&str>,
    confidence: &str,
) -> Value {
    let mut block = json!({
        "applied": false,
        "confidence": "low",
        "policy_source": policy_source,
        "reasons": [format!("rejected: {}", message)],
        "recommended_backend": FALLBACK_BACKEND,
        "schema": SCHEMA,
        "status": "rejected",
    });
    attach_trace_index_fields(&mut block, trace);
    attach_backend_readiness_fields(
        &mut block,
        registry,
        recommended_backend.unwrap_or(FALLBACK_BACKEND),
        "rejected",
        confidence,
    );
    block
}

fn computed_block(
    policy_source: &str,
    backend: &str,
    confidence: &str,
    reasons: Vec<String>,
    trace: &TraceIndexInfo,
    registry: &BackendRegistryInfo,
) -> Value {
    let mut block = json!({
        "applied": false,
        "confidence": confidence,
        "policy_source": policy_source,
        "reasons": reasons,
        "recommended_backend": backend,
        "schema": SCHEMA,
        "status": "computed",
    });
    attach_trace_index_fields(&mut block, trace);
    attach_backend_readiness_fields(&mut block, registry, backend, "computed", confidence);
    block
}

// -------------------------------------------------------------------
// wave26-03: optional backend-readiness registry consumption.
//
// The registry seed at .missiond/router/router-backend-registry-v1.lisp
// (top form: `(router-backend-registry <id> :schema ... :version ...
// (backend :id ... :readiness_status ... :runtime_allowed ...
//          :apply_blockers [...] ...))`) is read OPTIONALLY when
// `router_backend_registry_path` is supplied AND mode=dry_run. The
// daemon extracts ONLY the four fields it needs per backend entry —
// every other key (`:substrate`, `:non-goals`, `:notes`, `:owner`,
// `:adapter_path`) is ignored gracefully so the wave26-01 schema can
// grow without forcing a daemon update. Failure modes are non-fatal:
// dispatch always continues; only the apply-eligibility surface
// degrades.
// -------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(super) struct BackendEntry {
    pub(super) id: String,
    pub(super) readiness_status: String,
    pub(super) runtime_allowed: bool,
    pub(super) apply_blockers: Vec<String>,
}

/// Backend-registry status flavours surfaced on the recommendation
/// block. `Absent` is the default (no path supplied) and is observable
/// on the wire as the absence of every `backend_*` field.
#[derive(Debug, Clone)]
pub(super) enum BackendRegistryInfo {
    /// No registry path was supplied. Block does NOT carry any
    /// `backend_*` field (preserves wave24-04 / wave25-03 byte-shape
    /// for callers that opted out).
    Absent,
    /// Path was supplied; file read + parsed; backend entries indexed
    /// by id for O(1) join against the recommended backend.
    Used {
        path: String,
        backends: Vec<BackendEntry>,
    },
    /// Path was supplied but the file does not exist on disk.
    Missing { path: String, warning: String },
    /// Path was supplied; std::fs::read_to_string returned an I/O error
    /// other than NotFound.
    Unreadable { path: String, warning: String },
    /// Path was supplied; the Lisp parser failed OR the top-level shape
    /// did not match `(router-backend-registry ...)` OR a backend entry
    /// was missing a required field / had an enum violation.
    Malformed { path: String, warning: String },
}

fn load_backend_registry(input: Option<&str>) -> BackendRegistryInfo {
    let Some(path_str) = input else {
        return BackendRegistryInfo::Absent;
    };
    let path = path_str.to_string();
    let resolved = resolve_policy_path(&path);
    let raw = match std::fs::read_to_string(&resolved) {
        Ok(s) => s,
        Err(e) => {
            let warning = format!("backend-registry read failed: {}", e);
            return if e.kind() == std::io::ErrorKind::NotFound {
                BackendRegistryInfo::Missing { path, warning }
            } else {
                BackendRegistryInfo::Unreadable { path, warning }
            };
        }
    };
    match parse_backend_registry(&raw) {
        Ok(backends) => BackendRegistryInfo::Used { path, backends },
        Err(msg) => BackendRegistryInfo::Malformed {
            path,
            warning: format!("backend-registry parse failed: {}", msg),
        },
    }
}

/// wave26-03: splice the optional `backend_*` fields onto a recommendation
/// block. `Absent` emits NO fields at all (preserves wave24-04 / wave25-03
/// byte-shape for callers that opted out). All other variants emit
/// `backend_registry_path` + `backend_registry_status`; degraded variants
/// additionally emit `backend_warning`. When `Used` AND the recommended
/// backend is present in the registry, the block also surfaces
/// `backend_readiness_status` + `backend_runtime_allowed` +
/// `router_apply_eligible` + `router_apply_blockers`. When `Used` AND
/// the backend is missing, `backend_registry_status="unknown_backend"`,
/// `backend_readiness_status="unknown"`, `router_apply_eligible=false`.
fn attach_backend_readiness_fields(
    block: &mut Value,
    registry: &BackendRegistryInfo,
    recommended_backend: &str,
    status: &str,
    confidence: &str,
) {
    let Some(map) = block.as_object_mut() else {
        return;
    };
    match registry {
        BackendRegistryInfo::Absent => {
            // Intentionally emit NOTHING — preserves the byte-shape
            // for callers that did not opt in to wave26-03.
        }
        BackendRegistryInfo::Missing { path, warning } => {
            map.insert(
                "backend_registry_path".to_string(),
                Value::String(path.clone()),
            );
            map.insert(
                "backend_registry_status".to_string(),
                Value::String("missing".to_string()),
            );
            map.insert(
                "backend_warning".to_string(),
                Value::String(warning.clone()),
            );
            map.insert("router_apply_eligible".to_string(), Value::Bool(false));
            map.insert(
                "router_apply_blockers".to_string(),
                Value::Array(vec![Value::String(
                    "backend registry file is missing".to_string(),
                )]),
            );
        }
        BackendRegistryInfo::Unreadable { path, warning } => {
            map.insert(
                "backend_registry_path".to_string(),
                Value::String(path.clone()),
            );
            map.insert(
                "backend_registry_status".to_string(),
                Value::String("unreadable".to_string()),
            );
            map.insert(
                "backend_warning".to_string(),
                Value::String(warning.clone()),
            );
            map.insert("router_apply_eligible".to_string(), Value::Bool(false));
            map.insert(
                "router_apply_blockers".to_string(),
                Value::Array(vec![Value::String(
                    "backend registry file is unreadable".to_string(),
                )]),
            );
        }
        BackendRegistryInfo::Malformed { path, warning } => {
            map.insert(
                "backend_registry_path".to_string(),
                Value::String(path.clone()),
            );
            map.insert(
                "backend_registry_status".to_string(),
                Value::String("malformed".to_string()),
            );
            map.insert(
                "backend_warning".to_string(),
                Value::String(warning.clone()),
            );
            map.insert("router_apply_eligible".to_string(), Value::Bool(false));
            map.insert(
                "router_apply_blockers".to_string(),
                Value::Array(vec![Value::String(
                    "backend registry file is malformed".to_string(),
                )]),
            );
        }
        BackendRegistryInfo::Used { path, backends } => {
            map.insert(
                "backend_registry_path".to_string(),
                Value::String(path.clone()),
            );
            let matched: Option<&BackendEntry> =
                backends.iter().find(|b| b.id == recommended_backend);
            match matched {
                None => {
                    // Recommended backend absent from registry — surface
                    // unknown_backend status and force eligible=false.
                    map.insert(
                        "backend_registry_status".to_string(),
                        Value::String("unknown_backend".to_string()),
                    );
                    map.insert(
                        "backend_readiness_status".to_string(),
                        Value::String("unknown".to_string()),
                    );
                    map.insert("router_apply_eligible".to_string(), Value::Bool(false));
                    map.insert(
                        "router_apply_blockers".to_string(),
                        Value::Array(vec![Value::String(format!(
                            "recommended_backend `{}` not in registry",
                            recommended_backend
                        ))]),
                    );
                }
                Some(entry) => {
                    map.insert(
                        "backend_registry_status".to_string(),
                        Value::String("used".to_string()),
                    );
                    map.insert(
                        "backend_readiness_status".to_string(),
                        Value::String(entry.readiness_status.clone()),
                    );
                    map.insert(
                        "backend_runtime_allowed".to_string(),
                        Value::Bool(entry.runtime_allowed),
                    );
                    // 6-condition apply-eligibility gate — every miss
                    // contributes a synthetic blocker so operators can
                    // see WHY the gate is closed.
                    let mut blockers: Vec<String> = Vec::new();
                    if status != "computed" {
                        blockers.push(format!("policy status is `{}`; computed required", status));
                    }
                    if confidence != "high" {
                        blockers.push(format!("confidence is `{}`; high required", confidence));
                    }
                    if !entry.runtime_allowed {
                        blockers.push(
                            "backend runtime_allowed is false; runtime-ready adapter required"
                                .to_string(),
                        );
                    }
                    if entry.readiness_status != "runtime-ready" {
                        blockers.push(format!(
                            "backend readiness_status is `{}`; runtime-ready required",
                            entry.readiness_status
                        ));
                    }
                    // Echo the registry's own apply_blockers verbatim
                    // when present (operator should see the registry's
                    // reasons even when the synthetic gate already
                    // closed for another reason).
                    for b in &entry.apply_blockers {
                        blockers.push(b.clone());
                    }
                    let eligible = blockers.is_empty();
                    map.insert("router_apply_eligible".to_string(), Value::Bool(eligible));
                    map.insert(
                        "router_apply_blockers".to_string(),
                        Value::Array(blockers.into_iter().map(Value::String).collect()),
                    );
                }
            }
        }
    }
}

/// wave25-03: splice the optional `trace_index_path` / `trace_index_status`
/// / `trace_index_warning` fields onto a recommendation block. `Absent`
/// emits NO fields at all (preserves wave24-04 byte-shape for callers
/// that opted out). All other variants emit `trace_index_path` +
/// `trace_index_status`; degraded variants additionally emit
/// `trace_index_warning`.
fn attach_trace_index_fields(block: &mut Value, trace: &TraceIndexInfo) {
    let Some(map) = block.as_object_mut() else {
        return;
    };
    match trace {
        TraceIndexInfo::Absent => {
            // Intentionally emit NOTHING — keeps wave24-04 byte-shape
            // for callers that did not opt in to wave25-03.
        }
        TraceIndexInfo::Used { path, .. } => {
            map.insert("trace_index_path".to_string(), Value::String(path.clone()));
            map.insert(
                "trace_index_status".to_string(),
                Value::String("used".to_string()),
            );
        }
        TraceIndexInfo::Missing { path, warning } => {
            map.insert("trace_index_path".to_string(), Value::String(path.clone()));
            map.insert(
                "trace_index_status".to_string(),
                Value::String("missing".to_string()),
            );
            map.insert(
                "trace_index_warning".to_string(),
                Value::String(warning.clone()),
            );
        }
        TraceIndexInfo::Unreadable { path, warning } => {
            map.insert("trace_index_path".to_string(), Value::String(path.clone()));
            map.insert(
                "trace_index_status".to_string(),
                Value::String("unreadable".to_string()),
            );
            map.insert(
                "trace_index_warning".to_string(),
                Value::String(warning.clone()),
            );
        }
        TraceIndexInfo::Malformed { path, warning } => {
            map.insert("trace_index_path".to_string(), Value::String(path.clone()));
            map.insert(
                "trace_index_status".to_string(),
                Value::String("malformed".to_string()),
            );
            map.insert(
                "trace_index_warning".to_string(),
                Value::String(warning.clone()),
            );
        }
    }
}

// -------------------------------------------------------------------
// wave27-03: optional router dispatch descriptor surface.
//
// Splice a `router_dispatch_descriptor` sub-object onto an existing
// recommendation block, mirroring the wave27-01 schema
// (`missiond.router-dispatch-descriptor.v1`). The descriptor is a
// PURE PROJECTION of the wave24-04 / wave25-03 / wave26-03 fields
// already on the block — no new file I/O happens here, no backend is
// ever invoked, and dispatch is never altered. Three invariants are
// hard-coded as `Value::Bool` literals so the descriptor cannot be
// mis-promoted to a runtime apply signal:
//
//   - dry_run_only        = Value::Bool(true)   (locked)
//   - runtime_replacement = Value::Bool(false)  (locked)
//   - no_execution        = Value::Bool(true)   (locked)
//
// Registry semantics:
//   * `BackendRegistryInfo::Absent` (i.e. no `router_backend_registry_path`
//     supplied) → top-level `descriptor_status="registry_missing"` is
//     surfaced on the recommendation block; the descriptor BODY is
//     OMITTED so a downstream consumer cannot mistake the absence of
//     readiness for "ready". This matches the wave27-01 schema's
//     refusal to fake readiness.
//   * Any other registry state → emit a structured descriptor body.
//     For degraded states (Missing / Unreadable / Malformed /
//     unknown_backend) the recommendation block already carries the
//     synthetic `router_apply_eligible=false` / `router_apply_blockers`
//     fields plus (for unknown_backend) `backend_readiness_status="unknown"`;
//     the descriptor projects those off the block as-is. For Missing /
//     Unreadable / Malformed the block has no `backend_readiness_status`
//     at all — the descriptor falls back to the synthetic
//     `unknown` enum value (legal per wave27-01 readiness-statuses)
//     and `backend_runtime_allowed=false`.
fn attach_router_dispatch_descriptor(
    block: &mut Value,
    plan: &Plan,
    registry: &BackendRegistryInfo,
    policy_path_input: &str,
) {
    let Some(map) = block.as_object_mut() else {
        return;
    };
    // Branch 1: registry path was not supplied. Surface a structured
    // status field on the recommendation block; the descriptor body
    // is intentionally omitted because the wave27-01 schema requires
    // backend_readiness_status / backend_runtime_allowed values that
    // we cannot honestly produce without consulting a registry.
    if matches!(registry, BackendRegistryInfo::Absent) {
        map.insert(
            "descriptor_status".to_string(),
            Value::String("registry_missing".to_string()),
        );
        return;
    }
    // Branch 2: registry path supplied (any state — Used / Missing /
    // Unreadable / Malformed / unknown_backend). Build the descriptor
    // body by reading the wave26-03 fields back off the block. They
    // are guaranteed to be present except in the Missing / Unreadable
    // / Malformed paths where readiness/runtime_allowed are NOT set
    // upstream — for those we fall back to the synthetic `unknown`
    // enum + `false` runtime-allowed.
    let recommended_backend = map
        .get("recommended_backend")
        .and_then(|v| v.as_str())
        .unwrap_or(FALLBACK_BACKEND)
        .to_string();
    let router_confidence = map
        .get("confidence")
        .and_then(|v| v.as_str())
        .unwrap_or("low")
        .to_string();
    let backend_readiness_status = map
        .get("backend_readiness_status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let backend_runtime_allowed = map
        .get("backend_runtime_allowed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let router_apply_eligible = map
        .get("router_apply_eligible")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let router_apply_blockers: Vec<Value> = map
        .get("router_apply_blockers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let source_backend_registry_path = registry_path(registry).to_string();

    let mut descriptor = serde_json::Map::new();
    descriptor.insert(
        "schema".to_string(),
        Value::String("missiond.router-dispatch-descriptor.v1".to_string()),
    );
    descriptor.insert(
        "task_id".to_string(),
        Value::String(plan.board_task_id.clone()),
    );
    descriptor.insert(
        "recommended_backend".to_string(),
        Value::String(recommended_backend),
    );
    descriptor.insert(
        "router_confidence".to_string(),
        Value::String(router_confidence),
    );
    descriptor.insert(
        "backend_readiness_status".to_string(),
        Value::String(backend_readiness_status),
    );
    descriptor.insert(
        "backend_runtime_allowed".to_string(),
        Value::Bool(backend_runtime_allowed),
    );
    descriptor.insert(
        "router_apply_eligible".to_string(),
        Value::Bool(router_apply_eligible),
    );
    descriptor.insert(
        "router_apply_blockers".to_string(),
        Value::Array(router_apply_blockers),
    );
    // wave27-01 LOCKED INVARIANTS — these are hard-coded literal bools
    // and MUST NEVER be derived from any other field. If any future
    // change tries to compute these, the descriptor is no longer a
    // safe handoff record (per wave27-01 cross-wave-invariant text).
    descriptor.insert("dry_run_only".to_string(), Value::Bool(true));
    descriptor.insert("runtime_replacement".to_string(), Value::Bool(false));
    descriptor.insert("no_execution".to_string(), Value::Bool(true));
    descriptor.insert(
        "source_recommendation_schema".to_string(),
        Value::String(SCHEMA.to_string()),
    );
    descriptor.insert(
        "source_policy_path".to_string(),
        Value::String(policy_path_input.to_string()),
    );
    descriptor.insert(
        "source_backend_registry_path".to_string(),
        Value::String(source_backend_registry_path),
    );

    map.insert(
        "router_dispatch_descriptor".to_string(),
        Value::Object(descriptor),
    );
}

/// Extract the registry path string from any non-`Absent`
/// `BackendRegistryInfo` variant. `Absent` should never reach this
/// helper (the caller branches before).
fn registry_path(registry: &BackendRegistryInfo) -> &str {
    match registry {
        BackendRegistryInfo::Absent => "",
        BackendRegistryInfo::Used { path, .. }
        | BackendRegistryInfo::Missing { path, .. }
        | BackendRegistryInfo::Unreadable { path, .. }
        | BackendRegistryInfo::Malformed { path, .. } => path.as_str(),
    }
}

// ---- predicate AST + evaluator -----------------------------------

#[derive(Debug, Clone)]
pub(super) enum Clause {
    Kind(String),
    DispatchStrategy(String),
    Owner(String),
    Status(String),
    PathGlob(String),
    Any(Vec<Clause>),
    All(Vec<Clause>),
}

fn evaluate_clause(clause: &Clause, ctx: &PredicateContext) -> Result<(), &'static str> {
    match clause {
        Clause::Kind(v) => {
            if &ctx.kind == v {
                Ok(())
            } else {
                Err("kind mismatch")
            }
        }
        Clause::DispatchStrategy(v) => {
            if &ctx.dispatch_strategy == v {
                Ok(())
            } else {
                Err("dispatch_strategy mismatch")
            }
        }
        Clause::Owner(v) => {
            if &ctx.owner == v {
                Ok(())
            } else {
                Err("owner mismatch")
            }
        }
        Clause::Status(v) => {
            if &ctx.status == v {
                Ok(())
            } else {
                Err("status mismatch")
            }
        }
        Clause::PathGlob(pat) => {
            let re = glob_to_regex(pat);
            let any = ctx.write_scope.iter().any(|p| re.is_match(p));
            if any {
                Ok(())
            } else {
                Err("path-glob no match")
            }
        }
        Clause::All(children) => {
            if children.is_empty() {
                return Err("empty all");
            }
            for c in children {
                evaluate_clause(c, ctx)?;
            }
            Ok(())
        }
        Clause::Any(children) => {
            if children.is_empty() {
                return Err("empty any");
            }
            let mut last: Result<(), &'static str> = Err("any: no child matched");
            for c in children {
                if evaluate_clause(c, ctx).is_ok() {
                    return Ok(());
                }
                last = Err("any: no child matched");
            }
            last
        }
    }
}

/// Minimal glob-to-regex shim. Mirrors the wave24-03 CLI / scripts/lib
/// shape: `**` matches any sequence including `/`; `*` matches any
/// non-`/` sequence; `?` matches a single non-`/` char. Other regex
/// metacharacters are escaped. Inputs and candidates are normalised
/// to forward slashes and stripped of leading `./` / `/`.
pub(super) struct GlobRegex {
    pattern: String,
}

impl GlobRegex {
    fn is_match(&self, candidate: &str) -> bool {
        let candidate = normalize_path(candidate);
        // Tiny custom matcher (no regex crate dependency added). We
        // implement the predicate via a recursive descent over the
        // pattern. The pattern shape is small and exhaustive; this is
        // enough for path-glob predicates.
        glob_match(&self.pattern, &candidate)
    }
}

fn glob_to_regex(pattern: &str) -> GlobRegex {
    GlobRegex {
        pattern: normalize_path(pattern),
    }
}

fn normalize_path(p: &str) -> String {
    let mut s = p.replace('\\', "/");
    if let Some(stripped) = s.strip_prefix("./") {
        s = stripped.to_string();
    }
    while let Some(stripped) = s.strip_prefix('/') {
        s = stripped.to_string();
    }
    s
}

/// Recursive matcher: returns true iff `pattern` matches all of
/// `candidate` from start to end. Supports `**`, `*`, `?` per the
/// shared glob shape. Iterative-style with explicit indices to keep
/// the complexity bounded (no backtracking blowup on pathological
/// inputs because the patterns we accept are small and well-formed).
fn glob_match(pattern: &str, candidate: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let c: Vec<char> = candidate.chars().collect();
    glob_match_inner(&p, 0, &c, 0)
}

fn glob_match_inner(p: &[char], pi: usize, c: &[char], ci: usize) -> bool {
    let mut pi = pi;
    let mut ci = ci;
    loop {
        if pi >= p.len() {
            return ci >= c.len();
        }
        let pc = p[pi];
        if pc == '*' {
            if pi + 1 < p.len() && p[pi + 1] == '*' {
                // `**` matches any sequence including `/`. Try every
                // possible split. Skip a following `/` so `**/foo`
                // matches both `foo` and `a/foo`.
                let mut next_pi = pi + 2;
                if next_pi < p.len() && p[next_pi] == '/' {
                    next_pi += 1;
                }
                // Try matching zero characters first, then progressively
                // more from the candidate.
                if glob_match_inner(p, next_pi, c, ci) {
                    return true;
                }
                let mut k = ci;
                while k < c.len() {
                    k += 1;
                    if glob_match_inner(p, next_pi, c, k) {
                        return true;
                    }
                }
                return false;
            } else {
                // `*` matches any non-`/` sequence.
                if glob_match_inner(p, pi + 1, c, ci) {
                    return true;
                }
                let mut k = ci;
                while k < c.len() && c[k] != '/' {
                    k += 1;
                    if glob_match_inner(p, pi + 1, c, k) {
                        return true;
                    }
                }
                return false;
            }
        }
        if pc == '?' {
            if ci >= c.len() || c[ci] == '/' {
                return false;
            }
            pi += 1;
            ci += 1;
            continue;
        }
        // Literal char.
        if ci >= c.len() || c[ci] != pc {
            return false;
        }
        pi += 1;
        ci += 1;
    }
}

// ---- minimal Lisp parser for the wave24-01 router-policy schema ---

#[derive(Debug, Clone)]
pub(super) struct PolicyDoc {
    pub(super) dry_run_only: bool,
    pub(super) runtime_replacement: bool,
    pub(super) rules: Vec<RuleDoc>,
}

#[derive(Debug, Clone)]
pub(super) struct RuleDoc {
    pub(super) id: String,
    pub(super) priority: u32,
    pub(super) when: Clause,
    pub(super) backend: String,
    pub(super) reasoning: String,
}

#[derive(Debug, Clone)]
struct MatchedRule {
    id: String,
    priority: u32,
    reasoning: String,
}
