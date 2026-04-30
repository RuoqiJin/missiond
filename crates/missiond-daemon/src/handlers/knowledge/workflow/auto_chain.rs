//! Auto-chain and auto-trigger layers for mission_workflow distillation.
//!
//! This module owns the wave-20 deterministic auto-trigger gate and
//! re-exports the wave-19 explicit recorder from auto_chain/recorder.rs.

mod recorder;

#[allow(unused_imports)]
pub(super) use recorder::{
    attach_auto_chain_to_payload, auto_chain_requested, build_auto_chain_block,
    compute_evidence_sha256, derive_auto_chain_id, maybe_apply_auto_chain, parse_auto_chain_name,
    pick_workflow_anchor, AUTO_CHAIN_EVIDENCE_KIND, AUTO_CHAIN_EVIDENCE_SOURCE,
    AUTO_CHAIN_ID_SOURCE_DERIVED, AUTO_CHAIN_STATUS_RECORDED, AUTO_CHAIN_STATUS_RECORD_FAILED,
    AUTO_CHAIN_STATUS_RESOLVE_FAILED,
};

use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::Path;

use crate::state::AppState;

use super::auto_sonnet::{
    auto_sonnet_requested, maybe_apply_auto_sonnet, maybe_apply_auto_sonnet_no_trigger,
    maybe_apply_auto_sonnet_policy, parse_auto_sonnet_policy, AutoSonnetPolicy,
    AutoSonnetTriggerContext,
};
use super::distill::{evidence_sidecar_path, read_evidence_sidecar, EvidenceOutcome};
use super::resolve_project_root_from_args;

// ───────────────────────────────────────────────────────────────────────
// wave-20 / task 06 :: cross-plan distill auto-trigger v1
//
// Layered ON TOP of the wave-19 auto-chain recorder. Default trigger mode
// is `"never"` so existing callers (including the wave-19 `auto_chain=true`
// opt-in path) see byte-identical responses. When the caller passes
// `auto_chain_trigger="auto_safe"` the daemon evaluates a deterministic
// safety-rule set; only if ALL rules pass does it fall through to the
// wave-19 recorder. Rule failures surface a non-recording `skipped` block
// so audit consumers can replay the exact rule outcomes.
//
// Non-negotiables (mirror the wave-20 / task 06 brief):
//   - DEFAULT `auto_chain_trigger="never"` (legacy behaviour preserved).
//   - ONLY deterministic rules; NEVER calls Sonnet implicitly. Sonnet is
//     reachable solely via the existing `distill_mode="sonnet"` arg, which
//     is upstream of this trigger.
//   - Rule failure → `trigger_status="skipped_rules_failed"` + the full
//     rule-result list. NEVER partially appends a chain entry.
//   - Rule pass → behaves as if `auto_chain=true`: same evidence row, same
//     `auto_chain` block, same top-level `auto_chain_status` /
//     `auto_chain_id` shortcuts. We additionally splice an
//     `auto_trigger` block carrying `trigger_status`, `chain_id`,
//     `safety_rule_results`, and `sidecar` for symmetry with the
//     skipped path.
//   - Inner distill error → `trigger_status="skipped_inner_error"`; the
//     inner ToolResult is returned unmutated so error envelopes stay
//     loud (matches the wave-19 / plan.rs `apply_distill_chain` policy).
// ───────────────────────────────────────────────────────────────────────

/// Caller-facing trigger modes. `Never` (default) preserves wave-19
/// behaviour byte-for-byte. `AutoSafe` runs the deterministic safety
/// rules and triggers the wave-19 recorder iff all rules pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AutoChainTrigger {
    Never,
    AutoSafe,
}

impl AutoChainTrigger {
    pub(super) fn as_wire_str(self) -> &'static str {
        match self {
            AutoChainTrigger::Never => "never",
            AutoChainTrigger::AutoSafe => "auto_safe",
        }
    }
}

/// Status surfaced under `auto_trigger.trigger_status`. Audit consumers
/// pin these strings — never rename in-place.
pub(super) const AUTO_TRIGGER_STATUS_DISABLED: &str = "skipped_disabled";
pub(super) const AUTO_TRIGGER_STATUS_INNER_ERROR: &str = "skipped_inner_error";
pub(super) const AUTO_TRIGGER_STATUS_RULES_FAILED: &str = "skipped_rules_failed";
pub(super) const AUTO_TRIGGER_STATUS_TRIGGERED: &str = "triggered";
pub(super) const AUTO_TRIGGER_STATUS_TRIGGERED_RECORD_FAILED: &str = "triggered_record_failed";
pub(super) const AUTO_TRIGGER_STATUS_TRIGGERED_RESOLVE_FAILED: &str = "triggered_resolve_failed";

/// Deterministic safety-rule identifiers. Each maps to a single
/// boolean check evaluated by `evaluate_auto_trigger_safety_rules`.
pub(super) const SAFETY_RULE_INNER_DISTILL_OK: &str = "inner_distill_succeeded";
pub(super) const SAFETY_RULE_DISTILL_MODE_RECORDED: &str = "distill_mode_recorded";
pub(super) const SAFETY_RULE_PROJECT_ROOT_RESOLVED: &str = "project_root_resolved";
pub(super) const SAFETY_RULE_EVIDENCE_PRESENT: &str = "evidence_sidecar_present";
pub(super) const SAFETY_RULE_EVIDENCE_MIN_ENTRIES: &str = "evidence_min_entries";
pub(super) const SAFETY_RULE_NOT_ALREADY_CHAINED: &str = "chain_id_not_already_recorded";

/// Default minimum sidecar-entry count required by
/// `SAFETY_RULE_EVIDENCE_MIN_ENTRIES`. Mirrors the existing
/// `min_evidence` default on the sonnet distill gate so the trigger's
/// notion of "real evidence" matches the upstream.
pub(super) const AUTO_TRIGGER_DEFAULT_MIN_EVIDENCE: usize = 1;

/// Parse the caller-supplied trigger mode. Missing / blank / null →
/// `Never` (default-off). Unknown values are rejected loudly so a typo
/// can't silently disable the trigger.
pub(super) fn parse_auto_chain_trigger(raw: Option<&str>) -> Result<AutoChainTrigger, String> {
    match raw.map(str::trim) {
        None | Some("") | Some("never") => Ok(AutoChainTrigger::Never),
        Some("auto_safe") => Ok(AutoChainTrigger::AutoSafe),
        Some(other) => Err(format!(
            "auto_chain_trigger must be one of [\"never\", \"auto_safe\"]; got `{}`",
            other
        )),
    }
}

/// Pure outcome of a single safety-rule evaluation. `detail` is omitted
/// from the response when `passed=true` to keep the payload small.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SafetyRuleResult {
    pub(super) rule_id: &'static str,
    pub(super) passed: bool,
    pub(super) detail: Option<String>,
}

impl SafetyRuleResult {
    pub(super) fn pass(rule_id: &'static str) -> Self {
        Self {
            rule_id,
            passed: true,
            detail: None,
        }
    }

    pub(super) fn fail(rule_id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            rule_id,
            passed: false,
            detail: Some(detail.into()),
        }
    }

    pub(super) fn to_value(&self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("rule_id".to_string(), json!(self.rule_id));
        obj.insert("passed".to_string(), json!(self.passed));
        if let Some(d) = &self.detail {
            obj.insert("detail".to_string(), json!(d));
        }
        Value::Object(obj)
    }
}

/// Render the full rule list as a JSON array suitable for the
/// response payload.
pub(super) fn render_safety_rule_results(rules: &[SafetyRuleResult]) -> Value {
    Value::Array(rules.iter().map(SafetyRuleResult::to_value).collect())
}

/// Pure check: does the inner ToolResult carry an error envelope?
/// Wave-19's `maybe_apply_auto_chain` skips chain side-effects on
/// errors, so the trigger must surface the same skip — but with a
/// distinct `trigger_status` so audit consumers can tell a rule
/// failure apart from an upstream distill failure.
pub(super) fn inner_result_is_error(inner: &ToolResult) -> bool {
    inner.is_error.unwrap_or(false)
}

/// Pure check: does the inner payload carry a `distill_mode` field?
/// The wave-19 dry_run / sonnet branches both stamp this; an inner
/// payload missing it indicates the trigger is being asked to chain
/// a non-distill response (e.g. someone re-shaped the inner result
/// without preserving the wire contract). Refuse loud rather than
/// guess.
pub(super) fn inner_payload_has_distill_mode(payload: &Value) -> bool {
    payload
        .get("distill_mode")
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// Pure check: does the existing evidence sidecar already contain a
/// `distill_chain_record` entry whose `chain_id` matches the candidate?
/// The check is deterministic over the on-disk sidecar so two
/// concurrent triggers with the same canonical inputs both refuse to
/// double-record (the second one's rule simply fails — no DB
/// transaction needed).
///
/// Only the chain_id is compared; sources (wave-18 plan_dag vs wave-19
/// workflow) intentionally share the kind tag so audit queries see
/// both, and for dedup purposes we treat any prior `chain_id` collision
/// as "already chained".
pub(super) fn chain_id_already_in_sidecar(sidecar_value: &Value, candidate_chain_id: &str) -> bool {
    let entries = match sidecar_value.get("entries").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return false,
    };
    for entry in entries {
        let kind = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if kind != AUTO_CHAIN_EVIDENCE_KIND {
            continue;
        }
        let chain_id = entry
            .get("extra")
            .and_then(|e| e.get("chain_id"))
            .and_then(|v| v.as_str())
            .or_else(|| entry.get("chain_id").and_then(|v| v.as_str()))
            .unwrap_or("");
        if chain_id == candidate_chain_id {
            return true;
        }
    }
    false
}

/// Bundle of inputs + final pass/fail outcome for the deterministic
/// safety-rule evaluator. Public-shape members are private so the
/// evaluator is the single source of truth for rule wiring.
pub(super) struct SafetyRuleContext<'a> {
    pub(super) inner: &'a ToolResult,
    pub(super) inner_payload: &'a Value,
    pub(super) project_root: Option<&'a Path>,
    pub(super) project_resolve_error: Option<&'a str>,
    pub(super) evidence_outcome: &'a EvidenceOutcome,
    pub(super) candidate_chain_id: Option<&'a str>,
    pub(super) min_evidence: usize,
}

/// Pure evaluator. Returns the rule list in a fixed order so audit
/// consumers can pin the indices. `all_passed` is `true` iff every
/// rule's `passed=true`.
pub(super) fn evaluate_auto_trigger_safety_rules(
    ctx: &SafetyRuleContext<'_>,
) -> (Vec<SafetyRuleResult>, bool) {
    let mut rules: Vec<SafetyRuleResult> = Vec::new();

    // R1: inner distill must have produced a non-error envelope. Without
    // this rule the trigger could append chain rows for failed distills.
    if inner_result_is_error(ctx.inner) {
        rules.push(SafetyRuleResult::fail(
            SAFETY_RULE_INNER_DISTILL_OK,
            "inner distill returned a structured error envelope",
        ));
    } else {
        rules.push(SafetyRuleResult::pass(SAFETY_RULE_INNER_DISTILL_OK));
    }

    // R2: inner payload must carry a `distill_mode` so we know the inner
    // call really ran the distiller (dry_run or sonnet). A missing field
    // points at upstream contract drift — refuse loud instead of guessing.
    if inner_payload_has_distill_mode(ctx.inner_payload) {
        rules.push(SafetyRuleResult::pass(SAFETY_RULE_DISTILL_MODE_RECORDED));
    } else {
        rules.push(SafetyRuleResult::fail(
            SAFETY_RULE_DISTILL_MODE_RECORDED,
            "inner distill payload missing `distill_mode`",
        ));
    }

    // R3: project root must resolve. The wave-19 recorder anchors the
    // chain id on the canonical project root, so an unresolved root
    // makes the deterministic id meaningless.
    if ctx.project_root.is_some() {
        rules.push(SafetyRuleResult::pass(SAFETY_RULE_PROJECT_ROOT_RESOLVED));
    } else {
        let detail = ctx
            .project_resolve_error
            .map(|s| s.to_string())
            .unwrap_or_else(|| "project root resolution failed".to_string());
        rules.push(SafetyRuleResult::fail(
            SAFETY_RULE_PROJECT_ROOT_RESOLVED,
            detail,
        ));
    }

    // R4: evidence sidecar must exist and parse cleanly. Wave-19's
    // recorder will append even when the sidecar is missing (using the
    // `<no-evidence>` placeholder), but the v1 trigger refuses to do
    // that automatically — auto-mode demands the caller has already
    // recorded evidence.
    let (sidecar_present, sidecar_entry_count) = match ctx.evidence_outcome {
        EvidenceOutcome::Present { entry_count, .. } => (true, *entry_count),
        _ => (false, 0usize),
    };
    if sidecar_present {
        rules.push(SafetyRuleResult::pass(SAFETY_RULE_EVIDENCE_PRESENT));
    } else {
        let detail = match ctx.evidence_outcome {
            EvidenceOutcome::Missing => "evidence sidecar not found".to_string(),
            EvidenceOutcome::ParseFailed { error } => {
                format!("evidence sidecar parse failed: {}", error)
            }
            EvidenceOutcome::Present { .. } => unreachable!(),
        };
        rules.push(SafetyRuleResult::fail(SAFETY_RULE_EVIDENCE_PRESENT, detail));
    }

    // R5: sidecar must carry at least `min_evidence` entries. Mirrors
    // the upstream sonnet distill gate's `min_evidence` default; the
    // trigger never overrides it.
    if sidecar_present && sidecar_entry_count >= ctx.min_evidence {
        rules.push(SafetyRuleResult::pass(SAFETY_RULE_EVIDENCE_MIN_ENTRIES));
    } else if sidecar_present {
        rules.push(SafetyRuleResult::fail(
            SAFETY_RULE_EVIDENCE_MIN_ENTRIES,
            format!(
                "sidecar has {} entries; require >= {}",
                sidecar_entry_count, ctx.min_evidence
            ),
        ));
    } else {
        // R5 cannot evaluate independently when the sidecar is missing;
        // surface the dependency explicitly rather than silently
        // skipping.
        rules.push(SafetyRuleResult::fail(
            SAFETY_RULE_EVIDENCE_MIN_ENTRIES,
            "sidecar missing — entry count cannot be verified",
        ));
    }

    // R6: candidate chain id must NOT already exist in the sidecar.
    // Without this dedup the trigger could append the same chain_id
    // twice on rapid successive calls. We only evaluate when the
    // sidecar parsed AND a candidate id was derived.
    match (ctx.candidate_chain_id, ctx.evidence_outcome) {
        (Some(id), EvidenceOutcome::Present { value, .. }) => {
            if chain_id_already_in_sidecar(value, id) {
                rules.push(SafetyRuleResult::fail(
                    SAFETY_RULE_NOT_ALREADY_CHAINED,
                    format!("chain_id `{}` already recorded in sidecar", id),
                ));
            } else {
                rules.push(SafetyRuleResult::pass(SAFETY_RULE_NOT_ALREADY_CHAINED));
            }
        }
        (None, _) => {
            // Without a candidate chain id we cannot evaluate the
            // dedup rule — fail loud so the caller knows the trigger
            // is incomplete.
            rules.push(SafetyRuleResult::fail(
                SAFETY_RULE_NOT_ALREADY_CHAINED,
                "candidate chain_id not derived (upstream gate failed)",
            ));
        }
        (Some(_), _) => {
            // Sidecar absent / unreadable was already flagged by R4;
            // surface the dependency rather than silently passing.
            rules.push(SafetyRuleResult::fail(
                SAFETY_RULE_NOT_ALREADY_CHAINED,
                "sidecar unavailable — dedup cannot be verified",
            ));
        }
    }

    let all_passed = rules.iter().all(|r| r.passed);
    (rules, all_passed)
}

/// Build the `auto_trigger` block surfaced under the response payload.
/// The block always carries `requested`, `mode`, `trigger_status`, and
/// `safety_rule_results` so audit consumers see a stable shape.
/// `chain_id` and `sidecar` are surfaced when known.
pub(super) fn build_auto_trigger_block(
    requested: bool,
    mode: AutoChainTrigger,
    trigger_status: &str,
    safety_rule_results: Value,
    chain_id: Option<&str>,
    sidecar: Option<&str>,
) -> Value {
    let mut block = json!({
        "requested": requested,
        "mode": mode.as_wire_str(),
        "trigger_status": trigger_status,
        "safety_rule_results": safety_rule_results,
    });
    if let Some(id) = chain_id {
        block["chain_id"] = json!(id);
    }
    if let Some(p) = sidecar {
        block["sidecar"] = json!(p);
    }
    block
}

/// Splice the `auto_trigger` block + top-level shortcut onto the
/// response payload. Mirrors `attach_auto_chain_to_payload`'s
/// always-stable shape contract.
pub(super) fn attach_auto_trigger_to_payload(payload: &mut Value, block: Value) {
    if let Some(obj) = payload.as_object_mut() {
        if let Some(status) = block.get("trigger_status").and_then(|v| v.as_str()) {
            obj.insert("auto_trigger_status".to_string(), json!(status));
        }
        if let Some(id) = block.get("chain_id").and_then(|v| v.as_str()) {
            obj.insert("auto_trigger_chain_id".to_string(), json!(id));
        }
        obj.insert("auto_trigger".to_string(), block);
    }
}

/// Top-level orchestrator. Decides whether the wave-19 explicit
/// `auto_chain=true` path runs, the wave-20 auto-trigger evaluates,
/// or the inner ToolResult is returned unmutated (default).
///
/// Order of operations:
///   1. Parse the trigger mode. A malformed mode short-circuits to a
///      structured error envelope so callers see the typo loud.
///   2. If trigger=Never AND `auto_chain=false` → return inner unchanged
///      (legacy fast path; zero overhead).
///   3. If trigger=Never AND `auto_chain=true` → delegate to wave-19
///      `maybe_apply_auto_chain` (existing behaviour preserved).
///   4. If trigger=AutoSafe → evaluate safety rules; on pass, route
///      through the wave-19 recorder AND splice the trigger block; on
///      fail, splice a `skipped_rules_failed` block WITHOUT calling
///      the recorder.
pub(super) async fn maybe_apply_distill_chain_layers(
    state: &AppState,
    args: &Value,
    plan: &missiond_core::types::Plan,
    name: &str,
    inner: ToolResult,
) -> ToolResult {
    let trigger_mode =
        match parse_auto_chain_trigger(args.get("auto_chain_trigger").and_then(|v| v.as_str())) {
            Ok(m) => m,
            Err(msg) => {
                return ToolResult::structured_error(
                    ToolError::new(error_codes::INVALID_PARAM, msg).with_suggestion(
                        "auto_chain_trigger valid values: \"never\" (default) | \"auto_safe\"",
                    ),
                );
            }
        };

    let explicit_auto_chain = auto_chain_requested(args);
    let explicit_auto_sonnet = auto_sonnet_requested(args);
    // wave-22 / task 06 — closed-enum policy parser. Validation already
    // ran inside `action_distill`, so any non-Ok here is defensive only;
    // we collapse to Off on the unlikely fail-path so the chain still
    // runs.
    let auto_sonnet_policy = parse_auto_sonnet_policy(args).unwrap_or(AutoSonnetPolicy::Off);
    let policy_active = auto_sonnet_policy.is_active();

    // Fast path: nothing to do — return inner unchanged.
    //
    // wave-21 / task 07: when `auto_sonnet=true` is opted in WITHOUT the
    // wave-20 trigger AND without the wave-19 explicit chain, the gate
    // refuses the auto-apply (I3 — rules never ran) but still surfaces
    // a `skipped_no_trigger` block so the caller sees the missed
    // pre-condition.
    //
    // wave-22 / task 06: same pre-condition for the policy path —
    // `auto_sonnet_policy=safe_after_rules|dry_run` without the wave-20
    // trigger surfaces `skipped_no_trigger` on the policy block (I3).
    if trigger_mode == AutoChainTrigger::Never && !explicit_auto_chain {
        let mut result = inner;
        if explicit_auto_sonnet {
            result = maybe_apply_auto_sonnet_no_trigger(state, args, plan, name, result).await;
        }
        if policy_active {
            result = maybe_apply_auto_sonnet_policy(
                state,
                args,
                plan,
                name,
                result,
                auto_sonnet_policy,
                AutoSonnetTriggerContext {
                    trigger_mode,
                    rules_passed: false,
                    rules_value: Value::Array(Vec::new()),
                    sidecar: None,
                },
            )
            .await;
        }
        return result;
    }

    // Explicit wave-19 opt-in (no wave-20 trigger): preserve byte-compat
    // by delegating directly to the existing recorder. Wave-21 / task 07:
    // if `auto_sonnet=true` accompanies the wave-19 path, the gate
    // refuses (no trigger ran the safety rules) but layers a
    // `skipped_no_trigger` block on top. Wave-22 / task 06 mirrors the
    // refusal on the policy path.
    if trigger_mode == AutoChainTrigger::Never && explicit_auto_chain {
        let mut recorded = maybe_apply_auto_chain(state, args, plan, name, inner).await;
        if explicit_auto_sonnet {
            recorded = maybe_apply_auto_sonnet_no_trigger(state, args, plan, name, recorded).await;
        }
        if policy_active {
            recorded = maybe_apply_auto_sonnet_policy(
                state,
                args,
                plan,
                name,
                recorded,
                auto_sonnet_policy,
                AutoSonnetTriggerContext {
                    trigger_mode,
                    rules_passed: false,
                    rules_value: Value::Array(Vec::new()),
                    sidecar: None,
                },
            )
            .await;
        }
        return recorded;
    }

    // Wave-20 auto-trigger path. Inner errors short-circuit with a
    // dedicated status (so audit can tell rule failure apart from
    // upstream distill failure).
    if inner_result_is_error(&inner) {
        // Inner is an error envelope; we do not mutate it. We surface a
        // synthesised `auto_trigger` summary on a SECOND ToolResult would
        // mask the upstream error, so we return inner verbatim — the
        // caller already sees the failure loud.
        return inner;
    }

    // Re-project inner payload so we can splice. Anything non-object
    // returns inner unchanged (no silent payload synthesis — same rule
    // as wave-19).
    let mut payload = super::super::plan::tool_result_payload(&inner);
    if !payload.is_object() {
        return inner;
    }

    // Resolve project root + load evidence sidecar ONCE for both rule
    // evaluation and downstream chain-id derivation.
    let project_root_outcome = resolve_project_root_from_args(state, args).await;
    let (project_root_opt, project_resolve_error) = match &project_root_outcome {
        Ok(p) => (Some(p.clone()), None),
        Err(e) => (None, Some(e.clone())),
    };

    let evidence_path_opt = project_root_opt
        .as_ref()
        .map(|root| evidence_sidecar_path(root, plan.id));
    let evidence_outcome = match &evidence_path_opt {
        Some(path) => read_evidence_sidecar(path),
        None => EvidenceOutcome::Missing,
    };

    // Derive the candidate chain id (used by the dedup rule + by the
    // recorder downstream). When the project root failed to resolve we
    // skip derivation — R3 will fail and the trigger short-circuits
    // before we ever need the id.
    let workflow_id_str = payload
        .get("workflow_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let workflow_anchor = pick_workflow_anchor(workflow_id_str.as_deref(), name);
    let candidate_chain_id = project_root_opt.as_ref().map(|root| {
        let evidence_sha = evidence_path_opt
            .as_ref()
            .and_then(|p| compute_evidence_sha256(p));
        let evidence_sha_for_id = evidence_sha.unwrap_or_else(|| "<no-evidence>".to_string());
        derive_auto_chain_id(root, plan.id, &workflow_anchor, &evidence_sha_for_id)
    });

    let min_evidence = args
        .get("auto_trigger_min_evidence")
        .and_then(|v| v.as_i64())
        .map(|n| n.max(0) as usize)
        .unwrap_or(AUTO_TRIGGER_DEFAULT_MIN_EVIDENCE);

    let ctx = SafetyRuleContext {
        inner: &inner,
        inner_payload: &payload,
        project_root: project_root_opt.as_deref(),
        project_resolve_error: project_resolve_error.as_deref(),
        evidence_outcome: &evidence_outcome,
        candidate_chain_id: candidate_chain_id.as_deref(),
        min_evidence,
    };

    let (rules, all_passed) = evaluate_auto_trigger_safety_rules(&ctx);
    let rules_value = render_safety_rule_results(&rules);
    let sidecar_str = evidence_path_opt.as_ref().map(|p| p.display().to_string());

    if !all_passed {
        // Rule failure path: build a `skipped_rules_failed` block, splice
        // onto the payload (no chain_id), and return without recording.
        let block = build_auto_trigger_block(
            true,
            trigger_mode,
            AUTO_TRIGGER_STATUS_RULES_FAILED,
            rules_value.clone(),
            None,
            sidecar_str.as_deref(),
        );
        attach_auto_trigger_to_payload(&mut payload, block);

        // wave-21 / task 07: even on rules failure, if `auto_sonnet=true`
        // was opted in we surface a `skipped_rules_failed` auto-sonnet
        // block (mirrors the trigger's status) so the caller sees the
        // pre-condition that blocked Sonnet. wave-22 / task 06 layers
        // the policy block on top with the same `skipped_rules_failed`
        // status — I3 carryover proof.
        let mut result = ToolResult::json_pretty(&payload);
        if explicit_auto_sonnet {
            let ctx = AutoSonnetTriggerContext {
                trigger_mode,
                rules_passed: false,
                rules_value: rules_value.clone(),
                sidecar: sidecar_str.as_deref(),
            };
            result = maybe_apply_auto_sonnet(state, args, plan, name, result, ctx).await;
        }
        if policy_active {
            let ctx = AutoSonnetTriggerContext {
                trigger_mode,
                rules_passed: false,
                rules_value,
                sidecar: sidecar_str.as_deref(),
            };
            result = maybe_apply_auto_sonnet_policy(
                state,
                args,
                plan,
                name,
                result,
                auto_sonnet_policy,
                ctx,
            )
            .await;
        }
        return result;
    }

    // Rules passed: route the inner result through the wave-19 recorder
    // EXACTLY as if the caller had passed `auto_chain=true`. We
    // synthesise an args view with `auto_chain=true` flipped on so the
    // existing recorder sees an opt-in caller; downstream code then
    // attaches the wave-19 `auto_chain` block + shortcuts.
    let mut auto_args = args.clone();
    if let Some(obj) = auto_args.as_object_mut() {
        obj.insert("auto_chain".to_string(), json!(true));
    }
    let recorded = maybe_apply_auto_chain(state, &auto_args, plan, name, inner).await;

    // Re-project the recorded result so we can append the trigger block
    // alongside the wave-19 fields. If the recorder collapsed back to a
    // non-object (impossible in practice — `tool_result_payload` keeps
    // objects), we surrender and return its result verbatim.
    let mut recorded_payload = super::super::plan::tool_result_payload(&recorded);
    if !recorded_payload.is_object() {
        return recorded;
    }

    let recorded_chain_id = recorded_payload
        .get("auto_chain_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| candidate_chain_id.clone());
    let recorded_status = recorded_payload
        .get("auto_chain_status")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| AUTO_CHAIN_STATUS_RECORDED.to_string());

    let trigger_status = match recorded_status.as_str() {
        AUTO_CHAIN_STATUS_RECORDED => AUTO_TRIGGER_STATUS_TRIGGERED,
        AUTO_CHAIN_STATUS_RECORD_FAILED => AUTO_TRIGGER_STATUS_TRIGGERED_RECORD_FAILED,
        AUTO_CHAIN_STATUS_RESOLVE_FAILED => AUTO_TRIGGER_STATUS_TRIGGERED_RESOLVE_FAILED,
        // Defensive default: an unknown wave-19 status means the
        // recorder shape changed without us — fail loud rather than
        // guess. Surface the literal string verbatim.
        other => {
            tracing::warn!(
                wave19_status = other,
                "auto_trigger: unexpected wave-19 auto_chain status; surfacing verbatim"
            );
            return ToolResult::json_pretty(&recorded_payload);
        }
    };

    let block = build_auto_trigger_block(
        true,
        trigger_mode,
        trigger_status,
        rules_value.clone(),
        recorded_chain_id.as_deref(),
        sidecar_str.as_deref(),
    );
    attach_auto_trigger_to_payload(&mut recorded_payload, block);

    // wave-21 / task 07 — auto-sonnet apply-gate v1. Only reachable from
    // the wave-20 `auto_chain_trigger="auto_safe"` path AFTER all 6
    // safety rules already passed (we are inside the rules-passed
    // branch). The gate then layers an EXPLICIT caller-approval check
    // (`auto_sonnet=true` AND `auto_sonnet_approved=true`) and refuses
    // to auto-invoke Sonnet when the caller's `distill_mode` was
    // already `sonnet` (no double call). When all gates pass, Sonnet
    // is invoked and the inner `dry_run` payload is replaced with the
    // sonnet payload; on Sonnet failure (model error / invalid output)
    // the existing payload is preserved verbatim.
    //
    // wave-22 / task 06 — POLICY auto-sonnet apply-gate v2. Layered
    // AFTER the wave-21/07 layer so a caller can opt into either
    // surface (or both, in which case the policy block records the v2
    // verdict alongside the legacy v1 block — I7 additive).
    let recorded_inner = ToolResult::json_pretty(&recorded_payload);
    let mut after_legacy = maybe_apply_auto_sonnet(
        state,
        args,
        plan,
        name,
        recorded_inner,
        AutoSonnetTriggerContext {
            trigger_mode,
            rules_passed: true,
            rules_value: rules_value.clone(),
            sidecar: sidecar_str.as_deref(),
        },
    )
    .await;
    if policy_active {
        after_legacy = maybe_apply_auto_sonnet_policy(
            state,
            args,
            plan,
            name,
            after_legacy,
            auto_sonnet_policy,
            AutoSonnetTriggerContext {
                trigger_mode,
                rules_passed: true,
                rules_value,
                sidecar: sidecar_str.as_deref(),
            },
        )
        .await;
    }
    after_legacy
}
