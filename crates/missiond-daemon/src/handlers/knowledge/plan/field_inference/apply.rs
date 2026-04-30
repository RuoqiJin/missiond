use super::*;

/// Apply high-confidence inferred fields to a clone of `args` so the
/// downstream pipeline sees the augmented input. Caller-supplied values
/// are NEVER overwritten (they only ever land as conflicts upstream, and
/// conflicts are not promoted into `inferred`).
pub(in crate::handlers::knowledge::plan) fn apply_safe_augmentation(
    args: &Value,
    inference: &PlanFieldInference,
) -> Value {
    let mut augmented = args.clone();
    let map = match augmented.as_object_mut() {
        Some(m) => m,
        None => return augmented,
    };
    for f in &inference.inferred {
        if !f.confidence.meets_apply_threshold() {
            continue;
        }
        // Defensive guard: refuse to overwrite a caller-provided slot.
        // The inferer should already have routed any caller value into
        // `conflicts`, but we double-check at the mutation site so a
        // future regression cannot silently override caller intent.
        let already_set = map
            .get(f.field)
            .map(|v| match v {
                Value::Null => false,
                Value::String(s) => !s.trim().is_empty(),
                Value::Array(a) => !a.is_empty(),
                Value::Bool(_) => true,
                _ => true,
            })
            .unwrap_or(false);
        if already_set {
            continue;
        }
        map.insert(f.field.to_string(), f.value.clone());
    }
    augmented
}

// ── wave-21 / task 05 — PLAN inference apply-gate v1 ───────────────────
//
// Layered on top of wave-18 / task 06 (deterministic `infer_plan_fields`
// modes) and wave-20 / task 07 (LLM-augmented `sonnet_suggest`). The
// existing wave-18 `apply_safe` mode auto-applies high-confidence fields
// silently — which the wave-21 review surfaced as too lenient. The new
// `apply_inferred_fields=true` flag introduces an EXPLICIT operator
// approval before any inferred / proposed value mutates the call.
//
// Default behaviour (`apply_inferred_fields` absent / false) is suggest-
// only:
//   * `preview` / `sonnet_suggest` short-circuit unchanged;
//   * `apply_safe` still auto-fills high-confidence slots (legacy
//     behaviour preserved for back-compat — callers that relied on
//     wave-18 byte-shape do NOT have to opt into the new gate).
//
// Opt-in behaviour (`apply_inferred_fields=true`) is conservative:
//   * deterministic high-confidence inferred fields with NO conflict
//     are applied;
//   * deterministic suggestions (medium / low) are SKIPPED with reason
//     `"below_apply_threshold"`;
//   * caller-vs-inferred conflicts are NEVER applied — they surface on
//     `conflict_fields[]` with the conflict source intact;
//   * LLM proposals (wave-20 / sonnet_suggest) are SKIPPED unless the
//     caller explicitly approved them via `llm_caller_approved`
//     (per-field bool map or array of field names);
//   * approved LLM proposals additionally require:
//       - `confidence ∈ {high, medium}` (low-confidence LLM proposals
//         are conservative-skip);
//       - `conflict_status="none"` (no caller / deterministic clash);
//       - `safety_check` passes the per-field whitelist (mirrors
//         workstation_dispatch::WorkstationProposalValidator allowlists).
//
// The response carries a structured `apply_gate` block with:
//   * `requested`               — bool echoing the flag.
//   * `applied_fields[]`        — `{field, value, source, origin}`.
//   * `skipped_fields[]`        — `{field, reason, origin}`.
//   * `conflict_fields[]`       — `{field, caller_value, inferred_value,
//                                    confidence, source}`.
//   * `resulting_plan_preview`  — augmented args view (caller-supplied
//                                  ∪ applied_fields), suitable for the
//                                  caller to dry-run a follow-up call.
//   * `persist_inference_requested` — bool echoing `persist_inference`.
//   * `persist_inference_applied`   — always `false` in v1 (the gate
//                                      RESPECTS the persistence boundary
//                                      but does NOT mutate plan.sexp_text;
//                                      a future wave will wire the
//                                      persisted plan write).
//
// Lisp authority forward reference (Wave 21 backfill):
//   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//                         s4 plan-authoring (apply gate v1)
//   - intent-tools.lisp :: implemented-surface mission_plan ::
//                         :execute-contract :apply-inferred-fields-gate

/// Provenance tag for an apply-gate decision row. Keeps deterministic
/// inference distinguishable from LLM-augmented proposals on the wire so
/// observers can pivot on `origin` without re-reading the bundle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::handlers::knowledge::plan) enum ApplyOrigin {
    /// Field came from `PlanFieldInference::inferred[]`.
    DeterministicInferred,
    /// Field came from `PlanFieldInference::suggested[]`.
    DeterministicSuggested,
    /// Field came from `PlanFieldInference::conflicts[]`.
    DeterministicConflict,
    /// Field came from a `LlmProposal` (wave-20 / sonnet_suggest).
    LlmProposal,
}

impl ApplyOrigin {
    pub(in crate::handlers::knowledge::plan) fn as_wire(self) -> &'static str {
        match self {
            ApplyOrigin::DeterministicInferred => "deterministic_inferred",
            ApplyOrigin::DeterministicSuggested => "deterministic_suggested",
            ApplyOrigin::DeterministicConflict => "deterministic_conflict",
            ApplyOrigin::LlmProposal => "llm_proposal",
        }
    }
}

/// Field actually applied by the gate. Carries enough provenance for an
/// audit reader to reconstruct WHY the field was promoted (deterministic
/// high-confidence vs caller-approved LLM proposal).
#[derive(Debug, Clone)]
pub(in crate::handlers::knowledge::plan) struct AppliedField {
    pub(in crate::handlers::knowledge::plan) field: &'static str,
    pub(in crate::handlers::knowledge::plan) value: Value,
    pub(in crate::handlers::knowledge::plan) source: &'static str,
    pub(in crate::handlers::knowledge::plan) origin: ApplyOrigin,
}

impl AppliedField {
    fn to_json(&self) -> Value {
        json!({
            "field": self.field,
            "value": self.value.clone(),
            "source": self.source,
            "origin": self.origin.as_wire(),
        })
    }
}

/// Field deliberately NOT applied. The `reason` is a short canonical
/// string; observers can pivot on it without re-deriving the policy from
/// the rest of the response.
#[derive(Debug, Clone)]
pub(in crate::handlers::knowledge::plan) struct SkippedField {
    pub(in crate::handlers::knowledge::plan) field: &'static str,
    pub(in crate::handlers::knowledge::plan) reason: &'static str,
    pub(in crate::handlers::knowledge::plan) origin: ApplyOrigin,
    /// Optional human-readable detail (e.g. `"caller already set target"`).
    pub(in crate::handlers::knowledge::plan) detail: Option<String>,
}

impl SkippedField {
    fn to_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("field".to_string(), json!(self.field));
        m.insert("reason".to_string(), json!(self.reason));
        m.insert("origin".to_string(), json!(self.origin.as_wire()));
        if let Some(d) = &self.detail {
            m.insert("detail".to_string(), json!(d));
        }
        Value::Object(m)
    }
}

/// Aggregate apply-gate decision attached to the response under
/// `apply_gate`. Mirrors `PlanFieldInference::to_response_json` in always
/// emitting every list (empty when nothing fired) so observers pivot on a
/// stable shape regardless of which inference mode ran.
#[derive(Debug, Default)]
pub(in crate::handlers::knowledge::plan) struct ApplyGateOutcome {
    pub(in crate::handlers::knowledge::plan) requested: bool,
    pub(in crate::handlers::knowledge::plan) persist_inference_requested: bool,
    pub(in crate::handlers::knowledge::plan) applied: Vec<AppliedField>,
    pub(in crate::handlers::knowledge::plan) skipped: Vec<SkippedField>,
    pub(in crate::handlers::knowledge::plan) conflict: Vec<InferenceConflict>,
    /// Caller-supplied args augmented with `applied[]` — preview only.
    /// Always emitted so a follow-up caller can dry-run with the same
    /// shape without re-deriving it.
    pub(in crate::handlers::knowledge::plan) resulting_plan_preview: Value,
}

impl ApplyGateOutcome {
    pub(in crate::handlers::knowledge::plan) fn to_response_json(&self) -> Value {
        let applied: Vec<Value> = self.applied.iter().map(|f| f.to_json()).collect();
        let skipped: Vec<Value> = self.skipped.iter().map(|f| f.to_json()).collect();
        let conflict: Vec<Value> = self.conflict.iter().map(|c| c.to_json()).collect();
        json!({
            "requested": self.requested,
            "persist_inference_requested": self.persist_inference_requested,
            // v1 invariant: persisted plan text is NEVER mutated by this
            // gate. A future wave will wire the persisted plan write
            // gated by an existing `persist=true` action arg or the
            // explicit `persist_inference=true` flag.
            "persist_inference_applied": false,
            "applied_fields": applied,
            "skipped_fields": skipped,
            "conflict_fields": conflict,
            "resulting_plan_preview": self.resulting_plan_preview.clone(),
        })
    }
}

/// Parse the per-field `llm_caller_approved` map. Accepts:
///   * absent / null            → empty set (no LLM approvals).
///   * object `{field: bool}`   → set of fields with `true`.
///   * array of strings         → set of field names verbatim.
/// Strings outside the LLM allowlist are dropped silently (the gate
/// surfaces an "unknown_field" skip reason elsewhere if needed).
pub(in crate::handlers::knowledge::plan) fn parse_llm_caller_approved(
    args: &Value,
) -> std::collections::HashSet<&'static str> {
    let mut out: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    let raw = match args.get("llm_caller_approved") {
        Some(v) => v,
        None => return out,
    };
    match raw {
        Value::Object(map) => {
            for (k, v) in map.iter() {
                if !v.as_bool().unwrap_or(false) {
                    continue;
                }
                if let Some(canonical) = LLM_ALLOWED_FIELDS
                    .iter()
                    .find(|allowed| allowed.eq_ignore_ascii_case(k))
                    .copied()
                {
                    out.insert(canonical);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter() {
                let Some(s) = item.as_str() else {
                    continue;
                };
                if let Some(canonical) = LLM_ALLOWED_FIELDS
                    .iter()
                    .find(|allowed| allowed.eq_ignore_ascii_case(s.trim()))
                    .copied()
                {
                    out.insert(canonical);
                }
            }
        }
        // Any other shape is ignored — `llm_caller_approved` is always
        // an explicit map/array; a stray bool/string/number cannot be
        // construed as an approval list, so we treat it as empty rather
        // than erroring. A typo therefore keeps proposals SKIPPED, which
        // is the conservative default.
        _ => {}
    }
    out
}

/// True when caller passed `apply_inferred_fields=true` (any other shape
/// — including the literal string `"true"` — is rejected by the wave-21
/// validator before we get here, so this only checks the bool form).
pub(in crate::handlers::knowledge::plan) fn caller_requested_apply(args: &Value) -> bool {
    args.get("apply_inferred_fields")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// True when caller passed `persist_inference=true`. Surfaced on the
/// gate response so observers can audit which persistence boundary the
/// gate honoured. The actual plan-text write is FUTURE work — see the
/// `persist_inference_applied=false` invariant in
/// `ApplyGateOutcome::to_response_json`.
pub(in crate::handlers::knowledge::plan) fn caller_requested_persist_inference(
    args: &Value,
) -> bool {
    args.get("persist_inference")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Strict pre-flight validator for `apply_inferred_fields` /
/// `persist_inference`. We only accept the bool form — a literal
/// string `"true"` / `"false"` is rejected so a typo cannot silently
/// masquerade as opt-in. Mirrors the conservative posture of the
/// `infer_plan_fields` validator.
pub(in crate::handlers::knowledge::plan) fn validate_apply_gate_args(
    args: &Value,
) -> std::result::Result<(), String> {
    if let Some(v) = args.get("apply_inferred_fields") {
        if !v.is_boolean() && !v.is_null() {
            return Err(format!(
                "apply_inferred_fields must be a boolean (true|false); got {}",
                json_kind(v)
            ));
        }
    }
    if let Some(v) = args.get("persist_inference") {
        if !v.is_boolean() && !v.is_null() {
            return Err(format!(
                "persist_inference must be a boolean (true|false); got {}",
                json_kind(v)
            ));
        }
    }
    if let Some(v) = args.get("llm_caller_approved") {
        if !v.is_object() && !v.is_array() && !v.is_null() {
            return Err(format!(
                "llm_caller_approved must be object {{field: bool}} or array of field strings; got {}",
                json_kind(v)
            ));
        }
    }
    // wave-22 / task 04 — persisted apply v2 strict shape. `caller_approved`
    // is the second human opt-in (in addition to `apply_inferred_fields`)
    // that arms the persist path. `proposal_hash` is a 32-hex SHA-256
    // prefix the caller echoes back so an out-of-band tamper of
    // `apply_gate.applied_fields[]` is loud (mismatch ⇒ structured error
    // BEFORE any DB mutation per the goal contract). Both args are bool /
    // string only — any other shape (number / array / object) fails fast
    // here so a typo never silently arms or skips the persist path.
    if let Some(v) = args.get("caller_approved") {
        if !v.is_boolean() && !v.is_null() {
            return Err(format!(
                "caller_approved must be a boolean (true|false); got {}",
                json_kind(v)
            ));
        }
    }
    if let Some(v) = args.get("proposal_hash") {
        if !v.is_string() && !v.is_null() {
            return Err(format!(
                "proposal_hash must be a string (32-hex SHA-256 prefix); got {}",
                json_kind(v)
            ));
        }
    }
    Ok(())
}

/// Per-field safety check for an LLM proposal. Mirrors the conservative
/// whitelists wave-21 / task 04 pinned for `workstation_inference_mode`
/// (the LLM proposal pipeline already validated the value shape; this is
/// a second guard at the apply boundary). Returns `Ok(())` when the
/// proposal is safe to apply; `Err(detail)` otherwise.
pub(in crate::handlers::knowledge::plan) fn llm_proposal_safety_check(
    field: &str,
    value: &Value,
) -> std::result::Result<(), String> {
    match field {
        "target" => {
            let s = value.as_str().unwrap_or("");
            if matches!(
                s,
                "mission_execution" | "mission_task_delegate" | "mission_flow_run"
            ) {
                Ok(())
            } else {
                Err(format!("target value `{}` not in apply-gate whitelist", s))
            }
        }
        "dispatch_strategy" => {
            let s = value.as_str().unwrap_or("");
            // Conservative: prompt-fallback / unknown deliberately
            // EXCLUDED from auto-apply. Mirrors wave-21 / task 04.
            if matches!(
                s,
                "resident-lisp" | "fresh-code-alignment" | "agent-team" | "mixed"
            ) {
                Ok(())
            } else {
                Err(format!(
                    "dispatch_strategy value `{}` not in apply-gate whitelist",
                    s
                ))
            }
        }
        "acceptance_mode" => {
            let s = value.as_str().unwrap_or("");
            if canonicalize_acceptance_mode(s).is_some() {
                Ok(())
            } else {
                Err(format!(
                    "acceptance_mode value `{}` not in apply-gate whitelist",
                    s
                ))
            }
        }
        "owned_files" => {
            let arr = value.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
            if arr.is_empty() {
                Err("owned_files value is empty".to_string())
            } else if arr
                .iter()
                .all(|x| x.as_str().map(|s| !s.trim().is_empty()).unwrap_or(false))
            {
                Ok(())
            } else {
                Err("owned_files entries must be non-empty strings".to_string())
            }
        }
        "target_project" => {
            let s = value.as_str().unwrap_or("").trim();
            if s.is_empty() {
                Err("target_project value is empty".to_string())
            } else {
                Ok(())
            }
        }
        "workstation_dispatch" => {
            if value.as_bool().is_some() {
                Ok(())
            } else {
                Err("workstation_dispatch value must be boolean".to_string())
            }
        }
        other => Err(format!("field `{}` not supported by apply gate", other)),
    }
}

/// Compute the apply-gate decision over the inference result + caller
/// args. PURE function — no IO, no AppState reads — so the unit tests
/// can pin every edge case without touching the LLM.
///
/// The gate is suggest-only by default; only when
/// `apply_inferred_fields=true` does the function promote any field
/// into `applied[]`. Conflict / suggestion / non-approved-LLM rows
/// always land in `skipped[]` with a canonical reason so observers can
/// pivot on a stable shape.
pub(in crate::handlers::knowledge::plan) fn compute_apply_gate(
    args: &Value,
    inference: &PlanFieldInference,
) -> ApplyGateOutcome {
    let requested = caller_requested_apply(args);
    let persist_requested = caller_requested_persist_inference(args);
    let approved_llm_fields = parse_llm_caller_approved(args);
    let mut outcome = ApplyGateOutcome {
        requested,
        persist_inference_requested: persist_requested,
        ..Default::default()
    };

    // Conflicts always surface on `conflict_fields[]` regardless of the
    // gate flag — they are the strongest "do NOT silently mutate" signal
    // and observers must see them whether or not apply was requested.
    for c in &inference.conflicts {
        outcome.conflict.push(c.clone());
        // Also record a skip row so a single grep over `skipped_fields[]`
        // tells observers that the conflict-field WOULD have been skipped
        // had the gate tried to apply it.
        outcome.skipped.push(SkippedField {
            field: c.field,
            reason: "caller_value_conflict",
            origin: ApplyOrigin::DeterministicConflict,
            detail: Some(format!(
                "caller_value differs from inferred_value (source={})",
                c.source
            )),
        });
    }

    // Track which field slots are already accounted for (caller-supplied
    // OR already applied) to keep `resulting_plan_preview` deterministic
    // even when the same field appears in both the deterministic block
    // and an LLM proposal.
    let mut preview = args.clone();
    let mut filled: std::collections::HashSet<&'static str> = std::collections::HashSet::new();

    // Deterministic high-confidence inferred fields. Skipped without
    // approval; applied when `apply_inferred_fields=true` AND caller did
    // not already populate the slot.
    for f in &inference.inferred {
        if !f.confidence.meets_apply_threshold() {
            // Defensive — wave-18 invariant places only High in
            // `inferred[]`; record the row as a suggestion-tier skip if
            // a future regression sneaks one in.
            outcome.skipped.push(SkippedField {
                field: f.field,
                reason: "below_apply_threshold",
                origin: ApplyOrigin::DeterministicInferred,
                detail: Some(format!("confidence={}", f.confidence.as_wire())),
            });
            continue;
        }
        let caller_already_set = caller_value_for_field(args, f.field).is_some();
        if caller_already_set {
            outcome.skipped.push(SkippedField {
                field: f.field,
                reason: "caller_value_already_set",
                origin: ApplyOrigin::DeterministicInferred,
                detail: None,
            });
            continue;
        }
        if !requested {
            outcome.skipped.push(SkippedField {
                field: f.field,
                reason: "apply_gate_not_requested",
                origin: ApplyOrigin::DeterministicInferred,
                detail: None,
            });
            continue;
        }
        // Promote.
        outcome.applied.push(AppliedField {
            field: f.field,
            value: f.value.clone(),
            source: f.source,
            origin: ApplyOrigin::DeterministicInferred,
        });
        filled.insert(f.field);
        if let Some(map) = preview.as_object_mut() {
            map.insert(f.field.to_string(), f.value.clone());
        }
    }

    // Deterministic suggestions (medium / low). Always skipped — the
    // gate is conservative; sub-threshold fields require the caller to
    // promote them via an explicit arg, NOT the apply flag.
    for f in &inference.suggested {
        outcome.skipped.push(SkippedField {
            field: f.field,
            reason: "below_apply_threshold",
            origin: ApplyOrigin::DeterministicSuggested,
            detail: Some(format!("confidence={}", f.confidence.as_wire())),
        });
    }

    // LLM proposals — apply only when caller approval set + safety check
    // passes + no conflict + confidence != low + caller has not already
    // populated the slot + deterministic inferred[] has not already
    // claimed the slot.
    if let Some(bundle) = inference.llm.as_ref() {
        for p in &bundle.proposals {
            let approved = approved_llm_fields.contains(p.field);
            if !approved {
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "llm_not_caller_approved",
                    origin: ApplyOrigin::LlmProposal,
                    detail: None,
                });
                continue;
            }
            if !requested {
                // Caller approved the LLM proposal but did not flip the
                // master apply gate — skip with a distinct reason so
                // observers see the layered miss.
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "apply_gate_not_requested",
                    origin: ApplyOrigin::LlmProposal,
                    detail: None,
                });
                continue;
            }
            if matches!(p.confidence, InferenceConfidence::Low) {
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "llm_confidence_too_low",
                    origin: ApplyOrigin::LlmProposal,
                    detail: Some(format!("confidence={}", p.confidence.as_wire())),
                });
                continue;
            }
            if !matches!(p.conflict_status, LlmConflictStatus::None) {
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "llm_conflict_present",
                    origin: ApplyOrigin::LlmProposal,
                    detail: Some(format!("conflict_status={}", p.conflict_status.as_wire())),
                });
                continue;
            }
            if let Err(detail) = llm_proposal_safety_check(p.field, &p.value) {
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "llm_safety_check_failed",
                    origin: ApplyOrigin::LlmProposal,
                    detail: Some(detail),
                });
                continue;
            }
            if caller_value_for_field(args, p.field).is_some() {
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "caller_value_already_set",
                    origin: ApplyOrigin::LlmProposal,
                    detail: None,
                });
                continue;
            }
            if filled.contains(p.field) {
                // Deterministic inferred[] already promoted this slot;
                // surface the redundant LLM proposal as a structured
                // skip rather than silently duplicating it.
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "deterministic_inferred_already_applied",
                    origin: ApplyOrigin::LlmProposal,
                    detail: None,
                });
                continue;
            }
            outcome.applied.push(AppliedField {
                field: p.field,
                value: p.value.clone(),
                source: "llm_proposal",
                origin: ApplyOrigin::LlmProposal,
            });
            filled.insert(p.field);
            if let Some(map) = preview.as_object_mut() {
                map.insert(p.field.to_string(), p.value.clone());
            }
        }
    }

    outcome.resulting_plan_preview = preview;
    outcome
}

/// Splice the `apply_gate` block onto a successful response. Mirrors
/// `attach_inference_block`: structured errors are left untouched, and a
/// pre-existing block is preserved (NEVER overwritten) so future DAG /
/// resume paths can attach their own gate row.
pub(in crate::handlers::knowledge::plan) fn attach_apply_gate_block(
    mut result: ToolResult,
    block: Option<Value>,
) -> ToolResult {
    let Some(block) = block else {
        return result;
    };
    if result.is_error.unwrap_or(false) {
        return result;
    }
    let text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => return result,
    };
    let mut payload: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return result,
    };
    if let Some(map) = payload.as_object_mut() {
        map.entry("apply_gate".to_string()).or_insert(block);
    }
    result.content = vec![ToolContent::Text {
        text: serde_json::to_string_pretty(&payload).unwrap_or(text),
    }];
    result
}

mod persisted;

#[allow(unused_imports)]
pub(in crate::handlers::knowledge::plan) use persisted::*;
