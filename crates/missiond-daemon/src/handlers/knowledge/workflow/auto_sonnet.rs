//! Auto-Sonnet policy gates for mission_workflow distill chains.
//!
//! This module owns the wave-21 dual opt-in gate and the wave-22 policy gate.

use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};

use crate::state::AppState;

use super::auto_chain::{inner_result_is_error, AutoChainTrigger};
use super::distill::action_distill_sonnet;

// ───────────────────────────────────────────────────────────────────────
// wave-21 / task 07 :: sonnet distill chain auto-apply v1
//
// Layered ON TOP of the wave-20 `auto_chain_trigger="auto_safe"` path.
// Only when ALL six wave-20 deterministic safety rules pass AND the
// caller explicitly opts in via `auto_sonnet=true` AND attests
// `auto_sonnet_approved=true` does the daemon promote the inner distill
// from `dry_run` to `sonnet` automatically.
//
// Conservative invariants (mirror wave-21 / task 04..06 patterns):
//   I1: DEFAULT `auto_sonnet=false`. Existing callers see byte-identical
//       wave-20 responses (no `auto_sonnet` block).
//   I2: NEVER auto-invoke Sonnet without BOTH `auto_sonnet=true` AND
//       explicit `auto_sonnet_approved=true`. The latter is a separate
//       caller-attestation flag — a single typo cannot escalate the
//       daemon into invoking Sonnet.
//   I3: NEVER auto-invoke Sonnet unless ALL six wave-20 safety rules
//       passed. The auto-sonnet gate REUSES the same rule outcomes the
//       trigger evaluated; it does not relax them.
//   I4: NEVER auto-invoke Sonnet when the caller's `distill_mode` was
//       already `sonnet` — the call already chose its mode; we refuse
//       to re-call.
//   I5: On Sonnet failure (model error / invalid output) PRESERVE the
//       existing inner payload. The auto-sonnet block surfaces
//       `model_call_status="failed"` / `"invalid_output"` and
//       `applied=false`; the dry_run distill artifact stays durable.
//   I6: `review_required=true` PINNED on every successful auto-sonnet
//       outcome in v1 — the auto-applied sonnet output is always
//       receipt-only; no DB transition flips, the operator still
//       reviews the distilled workflow before promoting.
//   I7: The wave-20 `auto_trigger.trigger_status="triggered"` and
//       wave-19 `auto_chain.status="recorded"` blocks remain
//       UNCHANGED. The auto-sonnet block is purely additive.
// ───────────────────────────────────────────────────────────────────────

/// Status surfaced under `auto_sonnet.status`. Audit consumers pin these
/// strings — never rename in-place.
pub(super) const AUTO_SONNET_STATUS_NOT_REQUESTED: &str = "not_requested";
pub(super) const AUTO_SONNET_STATUS_DISABLED: &str = "disabled";
pub(super) const AUTO_SONNET_STATUS_SKIPPED_NO_TRIGGER: &str = "skipped_no_trigger";
pub(super) const AUTO_SONNET_STATUS_SKIPPED_RULES_FAILED: &str = "skipped_rules_failed";
pub(super) const AUTO_SONNET_STATUS_SKIPPED_NOT_APPROVED: &str = "skipped_caller_approval_missing";
pub(super) const AUTO_SONNET_STATUS_SKIPPED_ALREADY_SONNET: &str = "skipped_already_sonnet";
pub(super) const AUTO_SONNET_STATUS_SKIPPED_INNER_ERROR: &str = "skipped_inner_error";
pub(super) const AUTO_SONNET_STATUS_APPLIED_SONNET: &str = "applied_sonnet";

/// Status surfaced under `auto_sonnet.model_call_status`. Pinned so the
/// audit shape stays stable across runs.
pub(super) const AUTO_SONNET_MODEL_NOT_INVOKED: &str = "not_invoked";
pub(super) const AUTO_SONNET_MODEL_INVOKED: &str = "invoked";
pub(super) const AUTO_SONNET_MODEL_FAILED: &str = "failed";
pub(super) const AUTO_SONNET_MODEL_INVALID_OUTPUT: &str = "invalid_output";

/// Strict pre-flight shape validator for the wave-21 / task 07 knobs.
/// Both `auto_sonnet` and `auto_sonnet_approved` MUST be booleans when
/// supplied — string `"true"` is rejected with INVALID_PARAM (mirrors
/// wave-21 / task 05's `validate_apply_gate_args`).
pub(super) fn validate_auto_sonnet_args(args: &Value) -> std::result::Result<(), String> {
    if let Some(v) = args.get("auto_sonnet") {
        if !v.is_boolean() {
            return Err(format!(
                "auto_sonnet must be a boolean (true|false); got {}",
                shape_label(v)
            ));
        }
    }
    if let Some(v) = args.get("auto_sonnet_approved") {
        if !v.is_boolean() {
            return Err(format!(
                "auto_sonnet_approved must be a boolean (true|false); got {}",
                shape_label(v)
            ));
        }
    }
    Ok(())
}

/// Tiny helper to render the JSON shape label for INVALID_PARAM
/// diagnostics. Keeps the validator messages stable across upgrades.
pub(super) fn shape_label(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Pure check: caller opted into auto-sonnet via `auto_sonnet=true`.
/// Defaults to false on missing / non-bool / null shapes (validator
/// already rejected non-bool above; this guard preserves byte-compat
/// for callers that omit the key).
pub(super) fn auto_sonnet_requested(args: &Value) -> bool {
    args.get("auto_sonnet")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Pure check: caller attested `auto_sonnet_approved=true`. Same
/// strict-bool semantics as `auto_sonnet_requested`. Both flags must
/// flip true independently — a single typo cannot escalate the daemon.
pub(super) fn auto_sonnet_caller_approved(args: &Value) -> bool {
    args.get("auto_sonnet_approved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Pure check: caller's original `distill_mode` was already `sonnet`.
/// The auto-apply gate refuses to double-invoke Sonnet on the same
/// call (it would land a duplicate sidecar entry + waste tokens).
pub(super) fn caller_already_chose_sonnet(args: &Value) -> bool {
    matches!(
        args.get("distill_mode").and_then(|v| v.as_str()),
        Some("sonnet")
    )
}

/// Context passed into `maybe_apply_auto_sonnet` from the wave-20
/// trigger orchestrator. The trigger has already paid for project
/// resolution + sidecar load + rule evaluation — we forward the
/// outcomes verbatim so the auto-sonnet gate doesn't re-do the work.
pub(super) struct AutoSonnetTriggerContext<'a> {
    pub(super) trigger_mode: AutoChainTrigger,
    pub(super) rules_passed: bool,
    pub(super) rules_value: Value,
    pub(super) sidecar: Option<&'a str>,
}

/// Build the `auto_sonnet` block surfaced under the response payload.
/// Always carries `requested`, `status`, `applied`, `review_required`,
/// `model_call_status`, and `safety_rule_results`. Optional fields
/// (`model_call_error`, `sidecar`, `chain_id`, `caller_approval`,
/// `caller_distill_mode`) are surfaced when known.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_auto_sonnet_block(
    requested: bool,
    status: &str,
    applied: bool,
    review_required: bool,
    model_call_status: &str,
    safety_rule_results: Value,
    caller_approval: bool,
    caller_distill_mode: Option<&str>,
    model_call_error: Option<&str>,
    sidecar: Option<&str>,
    chain_id: Option<&str>,
) -> Value {
    let mut block = json!({
        "requested": requested,
        "status": status,
        "applied": applied,
        "review_required": review_required,
        "model_call_status": model_call_status,
        "safety_rule_results": safety_rule_results,
        "caller_approval": caller_approval,
    });
    if let Some(m) = caller_distill_mode {
        block["caller_distill_mode"] = json!(m);
    }
    if let Some(e) = model_call_error {
        block["model_call_error"] = json!(e);
    }
    if let Some(p) = sidecar {
        block["sidecar"] = json!(p);
    }
    if let Some(id) = chain_id {
        block["chain_id"] = json!(id);
    }
    block
}

/// Splice the `auto_sonnet` block + top-level shortcut onto the
/// response payload. Mirrors the wave-19 / wave-20 attach helpers.
pub(super) fn attach_auto_sonnet_to_payload(payload: &mut Value, block: Value) {
    if let Some(obj) = payload.as_object_mut() {
        if let Some(status) = block.get("status").and_then(|v| v.as_str()) {
            obj.insert("auto_sonnet_status".to_string(), json!(status));
        }
        obj.insert("auto_sonnet".to_string(), block);
    }
}

/// Top-level orchestrator for the wave-21 / task 07 auto-apply gate.
///
/// Order of operations:
///   1. If `auto_sonnet` was not requested → return inner unchanged
///      (default-off byte-compat path).
///   2. If inner payload is a structured error → splice
///      `skipped_inner_error` block; do NOT call Sonnet.
///   3. If wave-20 trigger mode is `Never` → splice `skipped_no_trigger`
///      block; the gate refuses to operate without the deterministic
///      trigger context.
///   4. If wave-20 rules failed → splice `skipped_rules_failed` block;
///      the gate REUSES the trigger's rule outcomes — it does not
///      relax them.
///   5. If `auto_sonnet_approved` was not set → splice
///      `skipped_caller_approval_missing` block.
///   6. If caller's `distill_mode` was already `sonnet` → splice
///      `skipped_already_sonnet` block.
///   7. All gates pass → invoke `mission_workflow(action=distill,
///      distill_mode="sonnet", …)` internally. On success, replace the
///      inner payload with the sonnet payload + splice the
///      `auto_sonnet` block. On model failure / invalid output,
///      PRESERVE the inner payload + splice the failure block.
pub(super) async fn maybe_apply_auto_sonnet<'a>(
    state: &AppState,
    args: &Value,
    plan: &missiond_core::types::Plan,
    name: &str,
    inner: ToolResult,
    ctx: AutoSonnetTriggerContext<'a>,
) -> ToolResult {
    if !auto_sonnet_requested(args) {
        // I1: default-off byte-compat — return inner verbatim.
        return inner;
    }

    // From here on the caller opted in; surface a block on every branch.
    let inner_is_error = inner_result_is_error(&inner);
    let mut payload = super::super::plan::tool_result_payload(&inner);
    if !payload.is_object() {
        // Non-object payload (defensive — wave-19/20 always emit
        // objects). Surrender without splicing.
        return inner;
    }

    let caller_approval = auto_sonnet_caller_approved(args);
    let caller_already_sonnet = caller_already_chose_sonnet(args);
    let caller_mode = args
        .get("distill_mode")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let chain_id = payload
        .get("auto_chain_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // I5 short-circuit: inner error envelope. We never call Sonnet on
    // top of an error and we never overwrite the error envelope.
    if inner_is_error {
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_SKIPPED_INNER_ERROR,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Trigger context check. We only operate inside the wave-20
    // auto_safe path so the deterministic safety rules already ran.
    if ctx.trigger_mode == AutoChainTrigger::Never {
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_SKIPPED_NO_TRIGGER,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I3: rules MUST have all passed. The auto-sonnet gate REUSES the
    // trigger's rule outcomes; it does not re-evaluate or relax them.
    if !ctx.rules_passed {
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_SKIPPED_RULES_FAILED,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I2: caller approval MUST be explicit. A single typo on
    // `auto_sonnet_approved` cannot escalate the daemon into invoking
    // Sonnet — it stays at `disabled` even when `auto_sonnet=true`.
    if !caller_approval {
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_SKIPPED_NOT_APPROVED,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I4: caller's `distill_mode` must NOT already be sonnet. We refuse
    // to double-invoke (would land a duplicate sidecar entry + burn
    // tokens). The caller's existing sonnet payload is already in the
    // response; we just surface a `skipped_already_sonnet` status.
    if caller_already_sonnet {
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_SKIPPED_ALREADY_SONNET,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // All gates passed — invoke the sonnet distiller internally. We
    // synthesise an args view with `distill_mode="sonnet"` flipped on,
    // and `auto_sonnet=false` to prevent infinite recursion. The
    // wave-19 `auto_chain` + wave-20 `auto_trigger` blocks already
    // landed on the parent payload — we strip them from the synthesised
    // args (`auto_chain_trigger=never`, `auto_chain=false`) so the
    // sonnet sub-call only runs the distiller and does NOT re-record a
    // chain entry (the parent already did).
    let mut sonnet_args = args.clone();
    if let Some(obj) = sonnet_args.as_object_mut() {
        obj.insert("distill_mode".to_string(), json!("sonnet"));
        obj.insert("auto_sonnet".to_string(), json!(false));
        obj.insert("auto_sonnet_approved".to_string(), json!(false));
        obj.insert("auto_chain".to_string(), json!(false));
        obj.insert("auto_chain_trigger".to_string(), json!("never"));
    }

    // Re-run the inner sonnet path directly. We bypass the top-level
    // `action_distill` dispatcher because we already validated args and
    // already ran the auto-chain layer; calling `action_distill_sonnet`
    // gives us the bare sonnet payload without re-entering the chain
    // orchestrator.
    let persist = args
        .get("persist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let sonnet_outcome = action_distill_sonnet(state, &sonnet_args, plan, name, persist).await;

    let sonnet_result = match sonnet_outcome {
        Ok(tr) => tr,
        Err(e) => {
            // Handler-level error → preserve inner payload, surface
            // `model_call_status="failed"` with the error text. The
            // dry_run / record-only distill artifact stays durable
            // (I5).
            let block = build_auto_sonnet_block(
                true,
                AUTO_SONNET_STATUS_APPLIED_SONNET,
                false,
                true,
                AUTO_SONNET_MODEL_FAILED,
                ctx.rules_value.clone(),
                caller_approval,
                caller_mode,
                Some(&format!("sonnet handler error: {}", e)),
                ctx.sidecar,
                chain_id.as_deref(),
            );
            attach_auto_sonnet_to_payload(&mut payload, block);
            return ToolResult::json_pretty(&payload);
        }
    };

    // I5: structured error envelope from the sonnet path → preserve
    // inner payload; surface `model_call_status="invalid_output"` (the
    // gateway returned, but the distiller refused — model returned bad
    // JSON / missing fields / unbalanced sexp).
    if sonnet_result.is_error.unwrap_or(false) {
        let err_payload = super::super::plan::tool_result_payload(&sonnet_result);
        let err_text = err_payload
            .get("error")
            .and_then(|e| e.get("message").or_else(|| e.get("code")))
            .and_then(|v| v.as_str())
            .unwrap_or("sonnet returned a structured error envelope")
            .to_string();
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_APPLIED_SONNET,
            false,
            true,
            AUTO_SONNET_MODEL_INVALID_OUTPUT,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            Some(&err_text),
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Sonnet succeeded — promote the sonnet payload as the new outer
    // payload (it carries `distill_mode="sonnet"`, `compiler_model`,
    // `workflow_sexp`, `match_rules`, etc.). We carry forward the
    // wave-19 / wave-20 blocks from the original inner payload so the
    // chain recorder + trigger receipts remain visible.
    let mut sonnet_payload = super::super::plan::tool_result_payload(&sonnet_result);
    if !sonnet_payload.is_object() {
        // Defensive — sonnet path always emits an object; preserve
        // inner if it didn't.
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_APPLIED_SONNET,
            false,
            true,
            AUTO_SONNET_MODEL_INVALID_OUTPUT,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            Some("sonnet payload was not a JSON object"),
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Carry forward wave-19 + wave-20 receipt fields from the original
    // dry_run payload so observers do not lose the chain recording.
    if let Some(obj) = sonnet_payload.as_object_mut() {
        for key in [
            "auto_chain",
            "auto_chain_status",
            "auto_chain_id",
            "auto_trigger",
            "auto_trigger_status",
            "auto_trigger_chain_id",
        ] {
            if let Some(v) = payload.get(key).cloned() {
                obj.entry(key.to_string()).or_insert(v);
            }
        }
    }

    // I6: review_required PINNED true on the auto-sonnet block; the
    // sonnet payload itself already carries `review_required: true`
    // for callers grepping that field directly.
    let block = build_auto_sonnet_block(
        true,
        AUTO_SONNET_STATUS_APPLIED_SONNET,
        true,
        true,
        AUTO_SONNET_MODEL_INVOKED,
        ctx.rules_value.clone(),
        caller_approval,
        caller_mode,
        None,
        ctx.sidecar,
        chain_id.as_deref(),
    );
    attach_auto_sonnet_to_payload(&mut sonnet_payload, block);
    ToolResult::json_pretty(&sonnet_payload)
}

/// Wave-21 / task 07 helper: the wave-19 explicit `auto_chain=true`
/// path (no wave-20 trigger) does NOT pre-evaluate the safety rules —
/// it just records. When the caller flips `auto_sonnet=true` on that
/// path the gate must still refuse (I3) because the deterministic rule
/// set was never run. We surface a `skipped_no_trigger` status so the
/// caller sees the missed pre-condition explicitly.
///
/// This wrapper is invoked from the wave-19 explicit path inside
/// `maybe_apply_distill_chain_layers` (see the early-return branch
/// for `trigger=Never AND auto_chain=true`).
pub(super) async fn maybe_apply_auto_sonnet_no_trigger(
    state: &AppState,
    args: &Value,
    plan: &missiond_core::types::Plan,
    name: &str,
    inner: ToolResult,
) -> ToolResult {
    if !auto_sonnet_requested(args) {
        return inner;
    }
    let ctx = AutoSonnetTriggerContext {
        trigger_mode: AutoChainTrigger::Never,
        rules_passed: false,
        rules_value: Value::Array(Vec::new()),
        sidecar: None,
    };
    maybe_apply_auto_sonnet(state, args, plan, name, inner, ctx).await
}

// ───────────────────────────────────────────────────────────────────────
// wave-22 / task 06 :: distill chain POLICY auto-Sonnet v2
//
// Replaces the wave-21 / task 07 dual opt-in (`auto_sonnet=true` AND
// `auto_sonnet_approved=true`) with a single explicit policy gate
// `auto_sonnet_policy ∈ {off, safe_after_rules, dry_run}`. The legacy
// dual opt-in path stays available for byte-shape back-compat (off →
// wave-21/07 unchanged); supplying any non-off policy promotes the
// daemon to the policy path which surfaces a `auto_sonnet_policy`
// block instead of (or in addition to, when both opt-ins land on the
// same call) the legacy `auto_sonnet` block.
//
// All seven wave-21/07 invariants STAY pinned on the policy path:
//   I1: DEFAULT `auto_sonnet_policy=off`. No `auto_sonnet_policy`
//       block emitted; existing callers see byte-identical
//       wave-21/07 + wave-20 responses.
//   I2: closed-enum strict-shape parser; string typo / unknown values
//       fail-fast as INVALID_PARAM. A single typo cannot escalate the
//       daemon — a malformed policy is rejected at action entry, a
//       missing policy stays `off`. The policy itself is the single
//       opt-in; supplying `safe_after_rules` IS the explicit operator
//       attestation (it is documented as the "auto-promote inner
//       distill from dry_run to sonnet" choice).
//   I3: `safe_after_rules` REUSES the wave-20 trigger's rule outcomes
//       verbatim — never relaxes them. The trigger MUST be
//       `auto_safe` AND ALL six deterministic rules MUST have passed
//       before the policy fires. Any rule failure surfaces
//       `skipped_rules_failed` with the full safety_rule_results.
//   I4: caller's `distill_mode=sonnet` still rejected (no double
//       Sonnet call). Surfaces `skipped_already_sonnet`.
//   I5: on Sonnet failure (model error / invalid output) PRESERVE
//       the existing inner payload. Policy block surfaces
//       `model_call_status="failed"|"invalid_output"` and
//       `applied=false`; the dry_run distill artifact stays durable.
//   I6: `review_required=true` PINNED on every successful
//       `safe_after_rules_applied` outcome — the auto-applied sonnet
//       output is always receipt-only; no DB transition flips, the
//       operator still reviews the distilled workflow.
//   I7: wave-19 `auto_chain` + wave-20 `auto_trigger` blocks remain
//       UNCHANGED. The `auto_sonnet_policy` block is purely additive.
//
// Block shape (always carried when policy != off):
//   {requested, policy, policy_status, applied, review_required,
//    model_call_status, safety_rule_results, sidecar?, chain_id?,
//    model_call_error?, caller_distill_mode?}
//   plus top-level shortcut `auto_sonnet_policy_status`.
// ───────────────────────────────────────────────────────────────────────

/// Closed-enum policy value surfaced in `auto_sonnet_policy.policy`.
/// Wire strings are pinned by `auto_sonnet_policy_value_strings_pin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AutoSonnetPolicy {
    Off,
    SafeAfterRules,
    DryRun,
}

pub(super) const AUTO_SONNET_POLICY_OFF_STR: &str = "off";
pub(super) const AUTO_SONNET_POLICY_SAFE_AFTER_RULES_STR: &str = "safe_after_rules";
pub(super) const AUTO_SONNET_POLICY_DRY_RUN_STR: &str = "dry_run";

impl AutoSonnetPolicy {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            AutoSonnetPolicy::Off => AUTO_SONNET_POLICY_OFF_STR,
            AutoSonnetPolicy::SafeAfterRules => AUTO_SONNET_POLICY_SAFE_AFTER_RULES_STR,
            AutoSonnetPolicy::DryRun => AUTO_SONNET_POLICY_DRY_RUN_STR,
        }
    }

    pub(super) fn is_active(self) -> bool {
        !matches!(self, AutoSonnetPolicy::Off)
    }
}

/// `auto_sonnet_policy.policy_status` taxonomy. Pinned by
/// `auto_sonnet_policy_status_constants_pin_the_wire_form`.
pub(super) const AUTO_SONNET_POLICY_STATUS_NOT_REQUESTED: &str = "not_requested";
pub(super) const AUTO_SONNET_POLICY_STATUS_OFF: &str = "off";
pub(super) const AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED: &str = "safe_after_rules_applied";
pub(super) const AUTO_SONNET_POLICY_STATUS_SAFE_DRY_RUN: &str = "safe_after_rules_dry_run";
pub(super) const AUTO_SONNET_POLICY_STATUS_SKIPPED_NO_TRIGGER: &str = "skipped_no_trigger";
pub(super) const AUTO_SONNET_POLICY_STATUS_SKIPPED_RULES_FAILED: &str = "skipped_rules_failed";
pub(super) const AUTO_SONNET_POLICY_STATUS_SKIPPED_ALREADY_SONNET: &str = "skipped_already_sonnet";
pub(super) const AUTO_SONNET_POLICY_STATUS_SKIPPED_INNER_ERROR: &str = "skipped_inner_error";

/// Strict closed-enum parser for the wave-22 / task 06 policy knob.
/// Returns Ok(Off) for missing / null / empty string (back-compat with
/// wave-21/07 callers). Rejects unknown values + non-string shapes
/// loudly so a typo (`"safe-after-rules"`, `"safeAfterRules"`, `1`)
/// surfaces as INVALID_PARAM at action entry — a single typo can NEVER
/// silently escalate the daemon (I2 carryover from wave-21/07).
pub(super) fn parse_auto_sonnet_policy(
    args: &Value,
) -> std::result::Result<AutoSonnetPolicy, String> {
    let raw = match args.get("auto_sonnet_policy") {
        None => return Ok(AutoSonnetPolicy::Off),
        Some(Value::Null) => return Ok(AutoSonnetPolicy::Off),
        Some(v) => v,
    };
    let s = match raw.as_str() {
        Some(s) => s,
        None => {
            return Err(format!(
                "auto_sonnet_policy must be a string (one of [\"off\",\"safe_after_rules\",\"dry_run\"]); got {}",
                shape_label(raw)
            ));
        }
    };
    match s {
        "" | AUTO_SONNET_POLICY_OFF_STR => Ok(AutoSonnetPolicy::Off),
        AUTO_SONNET_POLICY_SAFE_AFTER_RULES_STR => Ok(AutoSonnetPolicy::SafeAfterRules),
        AUTO_SONNET_POLICY_DRY_RUN_STR => Ok(AutoSonnetPolicy::DryRun),
        other => Err(format!(
            "auto_sonnet_policy must be one of [\"off\",\"safe_after_rules\",\"dry_run\"]; got `{}`",
            other
        )),
    }
}

/// Build the `auto_sonnet_policy` block. Mirrors `build_auto_sonnet_block`
/// but anchored on the policy enum so the wire surface stays decoupled
/// from the wave-21/07 dual opt-in vocabulary.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_auto_sonnet_policy_block(
    requested: bool,
    policy: AutoSonnetPolicy,
    policy_status: &str,
    applied: bool,
    review_required: bool,
    model_call_status: &str,
    safety_rule_results: Value,
    caller_distill_mode: Option<&str>,
    model_call_error: Option<&str>,
    sidecar: Option<&str>,
    chain_id: Option<&str>,
) -> Value {
    let mut block = json!({
        "requested": requested,
        "policy": policy.as_wire(),
        "policy_status": policy_status,
        "applied": applied,
        "review_required": review_required,
        "model_call_status": model_call_status,
        "safety_rule_results": safety_rule_results,
    });
    if let Some(m) = caller_distill_mode {
        block["caller_distill_mode"] = json!(m);
    }
    if let Some(e) = model_call_error {
        block["model_call_error"] = json!(e);
    }
    if let Some(p) = sidecar {
        block["sidecar"] = json!(p);
    }
    if let Some(id) = chain_id {
        block["chain_id"] = json!(id);
    }
    block
}

/// Splice the `auto_sonnet_policy` block + top-level shortcut onto the
/// response payload. Mirrors the wave-19 / wave-20 / wave-21 attach
/// helpers — preserves any pre-existing payload fields.
pub(super) fn attach_auto_sonnet_policy_to_payload(payload: &mut Value, block: Value) {
    if let Some(obj) = payload.as_object_mut() {
        if let Some(status) = block.get("policy_status").and_then(|v| v.as_str()) {
            obj.insert("auto_sonnet_policy_status".to_string(), json!(status));
        }
        obj.insert("auto_sonnet_policy".to_string(), block);
    }
}

/// Top-level orchestrator for the wave-22 / task 06 policy gate.
///
/// Order of operations (executed AFTER the wave-21/07 layer, so the
/// inner payload may already carry `auto_sonnet*` keys from the legacy
/// dual opt-in path — those are preserved verbatim):
///   0. Policy=Off → return inner unchanged (default-off byte-compat
///      with wave-21/07).
///   1. Inner payload is a structured error → splice
///      `skipped_inner_error` block; do NOT call Sonnet.
///   2. Wave-20 trigger is `Never` → splice `skipped_no_trigger`
///      block; the policy refuses to operate without the deterministic
///      trigger context (I3 — rules never ran).
///   3. Wave-20 rules failed → splice `skipped_rules_failed` block
///      (REUSES the trigger's rule outcomes verbatim — I3).
///   4. Caller's `distill_mode` was already `sonnet` → splice
///      `skipped_already_sonnet` block (I4 — no double-call).
///   5. Policy=DryRun → splice `safe_after_rules_dry_run` block; do
///      NOT call Sonnet. Used for testing the gate path without
///      burning model tokens.
///   6. Policy=SafeAfterRules + all gates pass → invoke
///      `mission_workflow(action=distill, distill_mode="sonnet", …)`
///      internally. On success, replace the inner payload with the
///      sonnet payload + splice the policy block. On model failure /
///      invalid output, PRESERVE the inner payload + splice the
///      failure block (I5).
pub(super) async fn maybe_apply_auto_sonnet_policy<'a>(
    state: &AppState,
    args: &Value,
    plan: &missiond_core::types::Plan,
    name: &str,
    inner: ToolResult,
    policy: AutoSonnetPolicy,
    ctx: AutoSonnetTriggerContext<'a>,
) -> ToolResult {
    if !policy.is_active() {
        return inner;
    }

    let inner_is_error = inner_result_is_error(&inner);
    let mut payload = super::super::plan::tool_result_payload(&inner);
    if !payload.is_object() {
        // Defensive — wave-19/20/21 always emit objects.
        return inner;
    }

    let caller_already_sonnet = caller_already_chose_sonnet(args);
    let caller_mode = args
        .get("distill_mode")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let chain_id = payload
        .get("auto_chain_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // I5 short-circuit: inner error envelope. Never call Sonnet on top
    // of an error and never overwrite the error envelope.
    if inner_is_error {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SKIPPED_INNER_ERROR,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I3 pre-condition: trigger context required. Without the wave-20
    // auto_safe trigger the deterministic rules never ran, so the
    // policy refuses to operate.
    if ctx.trigger_mode == AutoChainTrigger::Never {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SKIPPED_NO_TRIGGER,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I3: ALL six wave-20 safety rules MUST have passed. Reuses the
    // trigger's outcomes verbatim — never re-evaluates or relaxes.
    if !ctx.rules_passed {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SKIPPED_RULES_FAILED,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I4: caller's distill_mode must NOT already be sonnet.
    if caller_already_sonnet {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SKIPPED_ALREADY_SONNET,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Policy=DryRun: surface the gate verdict but do NOT invoke
    // Sonnet. Used for testing the policy path end-to-end without
    // burning model tokens. `applied=false`, `model_call_status=
    // not_invoked`, `review_required=true` PINNED.
    if matches!(policy, AutoSonnetPolicy::DryRun) {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SAFE_DRY_RUN,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Policy=SafeAfterRules + all gates passed → invoke the sonnet
    // distiller internally. Synthesise an args view with
    // `distill_mode="sonnet"` flipped on, `auto_sonnet_policy="off"`
    // (anti-recursion) AND clear the wave-21/07 dual opt-in flags +
    // wave-19/20 chain knobs so the sonnet sub-call only runs the
    // distiller and does NOT re-record a chain entry (the parent
    // already did via the wave-19 + wave-20 path).
    let mut sonnet_args = args.clone();
    if let Some(obj) = sonnet_args.as_object_mut() {
        obj.insert("distill_mode".to_string(), json!("sonnet"));
        obj.insert("auto_sonnet_policy".to_string(), json!("off"));
        obj.insert("auto_sonnet".to_string(), json!(false));
        obj.insert("auto_sonnet_approved".to_string(), json!(false));
        obj.insert("auto_chain".to_string(), json!(false));
        obj.insert("auto_chain_trigger".to_string(), json!("never"));
    }

    let persist = args
        .get("persist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let sonnet_outcome = action_distill_sonnet(state, &sonnet_args, plan, name, persist).await;

    let sonnet_result = match sonnet_outcome {
        Ok(tr) => tr,
        Err(e) => {
            // I5: handler-level error → preserve inner payload, surface
            // `model_call_status="failed"`. The dry_run / record-only
            // distill artifact stays durable.
            let block = build_auto_sonnet_policy_block(
                true,
                policy,
                AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
                false,
                true,
                AUTO_SONNET_MODEL_FAILED,
                ctx.rules_value.clone(),
                caller_mode,
                Some(&format!("sonnet handler error: {}", e)),
                ctx.sidecar,
                chain_id.as_deref(),
            );
            attach_auto_sonnet_policy_to_payload(&mut payload, block);
            return ToolResult::json_pretty(&payload);
        }
    };

    // I5: structured error envelope from the sonnet path → preserve
    // inner payload; surface `model_call_status="invalid_output"`.
    if sonnet_result.is_error.unwrap_or(false) {
        let err_payload = super::super::plan::tool_result_payload(&sonnet_result);
        let err_text = err_payload
            .get("error")
            .and_then(|e| e.get("message").or_else(|| e.get("code")))
            .and_then(|v| v.as_str())
            .unwrap_or("sonnet returned a structured error envelope")
            .to_string();
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
            false,
            true,
            AUTO_SONNET_MODEL_INVALID_OUTPUT,
            ctx.rules_value.clone(),
            caller_mode,
            Some(&err_text),
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Sonnet succeeded — promote the sonnet payload as the new outer
    // payload. Carry forward the wave-19/20/21 receipt blocks from the
    // original inner payload so chain recorder + trigger receipts +
    // legacy auto_sonnet block (if any) remain visible.
    let mut sonnet_payload = super::super::plan::tool_result_payload(&sonnet_result);
    if !sonnet_payload.is_object() {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
            false,
            true,
            AUTO_SONNET_MODEL_INVALID_OUTPUT,
            ctx.rules_value.clone(),
            caller_mode,
            Some("sonnet payload was not a JSON object"),
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I7: carry forward wave-19 + wave-20 + wave-21/07 receipt fields
    // from the original dry_run payload so observers do not lose the
    // chain recording / trigger receipts / legacy auto_sonnet block.
    if let Some(obj) = sonnet_payload.as_object_mut() {
        for key in [
            "auto_chain",
            "auto_chain_status",
            "auto_chain_id",
            "auto_trigger",
            "auto_trigger_status",
            "auto_trigger_chain_id",
            "auto_sonnet",
            "auto_sonnet_status",
        ] {
            if let Some(v) = payload.get(key).cloned() {
                obj.entry(key.to_string()).or_insert(v);
            }
        }
    }

    // I6: review_required PINNED true on every successful auto-sonnet
    // policy outcome.
    let block = build_auto_sonnet_policy_block(
        true,
        policy,
        AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
        true,
        true,
        AUTO_SONNET_MODEL_INVOKED,
        ctx.rules_value.clone(),
        caller_mode,
        None,
        ctx.sidecar,
        chain_id.as_deref(),
    );
    attach_auto_sonnet_policy_to_payload(&mut sonnet_payload, block);
    ToolResult::json_pretty(&sonnet_payload)
}
