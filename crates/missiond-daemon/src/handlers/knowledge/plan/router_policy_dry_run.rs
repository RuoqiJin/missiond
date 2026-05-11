use super::ResolvedExec;
use missiond_core::types::Plan;
use missiond_mcp::tools::{error_codes, ToolContent, ToolError, ToolResult};
use serde_json::Value;

mod descriptor;
mod predicate;
mod readiness;
mod schema_parser;

use descriptor::attach_router_dispatch_descriptor;
use predicate::{arg_string, evaluate_clause, project_context};
use readiness::{
    computed_block, error_block, events_for_backend, events_for_task, load_backend_registry,
    load_trace_index, rejected_block, resolve_policy_path, BackendEntry, BackendRegistryInfo,
    TraceIndexInfo, RICH_TRACE_THRESHOLD,
};

fn parse_backend_registry(input: &str) -> Result<Vec<BackendEntry>, String> {
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
            );
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
