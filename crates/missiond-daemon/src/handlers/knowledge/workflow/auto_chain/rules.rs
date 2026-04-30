//! Deterministic auto-trigger parser and safety-rule evaluator.
//!
//! This is the pure core under auto_chain.rs: no side effects, only typed
//! trigger parsing, rule evaluation, and JSON projection of rule results.

use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use std::path::Path;

use super::super::distill::EvidenceOutcome;
use super::{AutoChainTrigger, AUTO_CHAIN_EVIDENCE_KIND};

/// Deterministic safety-rule identifiers. Each maps to a single
/// boolean check evaluated by `evaluate_auto_trigger_safety_rules`.
pub(in crate::handlers::knowledge::workflow) const SAFETY_RULE_INNER_DISTILL_OK: &str =
    "inner_distill_succeeded";
pub(in crate::handlers::knowledge::workflow) const SAFETY_RULE_DISTILL_MODE_RECORDED: &str =
    "distill_mode_recorded";
pub(in crate::handlers::knowledge::workflow) const SAFETY_RULE_PROJECT_ROOT_RESOLVED: &str =
    "project_root_resolved";
pub(in crate::handlers::knowledge::workflow) const SAFETY_RULE_EVIDENCE_PRESENT: &str =
    "evidence_sidecar_present";
pub(in crate::handlers::knowledge::workflow) const SAFETY_RULE_EVIDENCE_MIN_ENTRIES: &str =
    "evidence_min_entries";
pub(in crate::handlers::knowledge::workflow) const SAFETY_RULE_NOT_ALREADY_CHAINED: &str =
    "chain_id_not_already_recorded";

/// Default minimum sidecar-entry count required by
/// `SAFETY_RULE_EVIDENCE_MIN_ENTRIES`. Mirrors the existing
/// `min_evidence` default on the sonnet distill gate so the trigger's
/// notion of "real evidence" matches the upstream.
pub(in crate::handlers::knowledge::workflow) const AUTO_TRIGGER_DEFAULT_MIN_EVIDENCE: usize = 1;

/// Parse the caller-supplied trigger mode. Missing / blank / null →
/// `Never` (default-off). Unknown values are rejected loudly so a typo
/// can't silently disable the trigger.
pub(in crate::handlers::knowledge::workflow) fn parse_auto_chain_trigger(
    raw: Option<&str>,
) -> Result<AutoChainTrigger, String> {
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
pub(in crate::handlers::knowledge::workflow) struct SafetyRuleResult {
    pub(in crate::handlers::knowledge::workflow) rule_id: &'static str,
    pub(in crate::handlers::knowledge::workflow) passed: bool,
    pub(in crate::handlers::knowledge::workflow) detail: Option<String>,
}

impl SafetyRuleResult {
    pub(in crate::handlers::knowledge::workflow) fn pass(rule_id: &'static str) -> Self {
        Self {
            rule_id,
            passed: true,
            detail: None,
        }
    }

    pub(in crate::handlers::knowledge::workflow) fn fail(
        rule_id: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            rule_id,
            passed: false,
            detail: Some(detail.into()),
        }
    }

    pub(in crate::handlers::knowledge::workflow) fn to_value(&self) -> Value {
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
pub(in crate::handlers::knowledge::workflow) fn render_safety_rule_results(
    rules: &[SafetyRuleResult],
) -> Value {
    Value::Array(rules.iter().map(SafetyRuleResult::to_value).collect())
}

/// Pure check: does the inner ToolResult carry an error envelope?
/// Wave-19's `maybe_apply_auto_chain` skips chain side-effects on
/// errors, so the trigger must surface the same skip — but with a
/// distinct `trigger_status` so audit consumers can tell a rule
/// failure apart from an upstream distill failure.
pub(in crate::handlers::knowledge::workflow) fn inner_result_is_error(inner: &ToolResult) -> bool {
    inner.is_error.unwrap_or(false)
}

/// Pure check: does the inner payload carry a `distill_mode` field?
/// The wave-19 dry_run / sonnet branches both stamp this; an inner
/// payload missing it indicates the trigger is being asked to chain
/// a non-distill response (e.g. someone re-shaped the inner result
/// without preserving the wire contract). Refuse loud rather than
/// guess.
pub(in crate::handlers::knowledge::workflow) fn inner_payload_has_distill_mode(
    payload: &Value,
) -> bool {
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
pub(in crate::handlers::knowledge::workflow) fn chain_id_already_in_sidecar(
    sidecar_value: &Value,
    candidate_chain_id: &str,
) -> bool {
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
pub(in crate::handlers::knowledge::workflow) struct SafetyRuleContext<'a> {
    pub(in crate::handlers::knowledge::workflow) inner: &'a ToolResult,
    pub(in crate::handlers::knowledge::workflow) inner_payload: &'a Value,
    pub(in crate::handlers::knowledge::workflow) project_root: Option<&'a Path>,
    pub(in crate::handlers::knowledge::workflow) project_resolve_error: Option<&'a str>,
    pub(in crate::handlers::knowledge::workflow) evidence_outcome: &'a EvidenceOutcome,
    pub(in crate::handlers::knowledge::workflow) candidate_chain_id: Option<&'a str>,
    pub(in crate::handlers::knowledge::workflow) min_evidence: usize,
}

/// Pure evaluator. Returns the rule list in a fixed order so audit
/// consumers can pin the indices. `all_passed` is `true` iff every
/// rule's `passed=true`.
pub(in crate::handlers::knowledge::workflow) fn evaluate_auto_trigger_safety_rules(
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
