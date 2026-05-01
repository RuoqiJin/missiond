use super::*;

// ───────────────────────────────────────────────────────────────────────
// wave-18 / task 06 — autonomous PLAN field inference v0
//
// Conservative deterministic helper that infers a small set of PLAN DAG
// fields when the caller / PLAN.lisp / evidence-sidecar carry enough
// signal. Inference is gated on the new `infer_plan_fields` knob:
//
//   `off`        (default) — no inference; legacy byte-shape preserved.
//   `preview`    — runs the inference and returns ONLY the inference block;
//                  the underlying execute pipeline is NOT invoked. Caller
//                  uses this to verify what apply_safe would do without
//                  mutating any args.
//   `apply_safe` — runs the inference, augments caller args with every
//                  field whose confidence >= apply-threshold AND whose
//                  caller-side slot is empty, then proceeds with execute
//                  exactly as if the caller had passed the augmented args.
//                  Caller-supplied values ALWAYS win (conflicts surface
//                  on `conflicts[]` and are NEVER mutated).
//
// Six fields are supported in v0:
//   target / dispatch_strategy / target_project / owned_files /
//   acceptance_mode / workstation_dispatch.
//
// Sources scanned (deterministic, no LLM):
//   1. `plan.sexp_text` — already parsed via `parse_plan_hints`. PLAN-side
//      hints are the highest-confidence source.
//   2. `plan.compiled_from` — directive provenance string (e.g.
//      "directive/<id>:<v>" or "board_task/<id>"). Read for keyword
//      signals only (e.g. "task_delegate" / "agent-team").
//   3. evidence sidecar at
//      `<project>/.missiond/v3/runtime/plans/<plan_id>.evidence.json`
//      (with legacy `.missiond/v2/plans` fallback) — historical
//      `plan_runner_dispatch` / `workstation_dispatch` entries carry the
//      target / dispatch_strategy / owned_files we used last time.
//
// Lisp authority forward-reference (wave-18 / task 10 backfill):
//   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//                          s4 plan-authoring (autonomous inference)
//   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//                                  role plan-runner (deterministic infer)
// ───────────────────────────────────────────────────────────────────────

// mode.rs owns infer_plan_fields / workstation_inference_mode parsing and DAG preflight gates.
mod mode;
pub(crate) use mode::{parse_infer_plan_fields_mode, InferPlanFieldsMode};
pub(super) use mode::{
    parse_workstation_inference_mode, refuse_workstation_inference_in_dag_mode,
    WorkstationInferenceMode, WORKSTATION_INFER_MODE_SONNET_SUGGEST,
};
#[cfg(test)]
pub(super) use mode::{INFER_MODE_SONNET_SUGGEST, WORKSTATION_INFER_MODE_OFF};

/// Confidence tier for an inferred field. Only `High` is auto-applied
/// under `apply_safe`; lower tiers always degrade to suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InferenceConfidence {
    High,
    Medium,
    Low,
}

impl InferenceConfidence {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            InferenceConfidence::High => "high",
            InferenceConfidence::Medium => "medium",
            InferenceConfidence::Low => "low",
        }
    }

    /// Apply threshold for `apply_safe`. Conservative on purpose: only
    /// `High` confidence fields auto-fill missing caller args. Medium /
    /// Low always degrade to suggestions.
    pub(super) fn meets_apply_threshold(self) -> bool {
        matches!(self, InferenceConfidence::High)
    }
}

/// Single inferred field with its provenance + confidence.
#[derive(Debug, Clone)]
pub(super) struct InferredField {
    pub(super) field: &'static str,
    pub(super) value: Value,
    pub(super) confidence: InferenceConfidence,
    pub(super) source: &'static str,
    pub(super) detail: Option<String>,
}

impl InferredField {
    fn to_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("field".to_string(), json!(self.field));
        m.insert("value".to_string(), self.value.clone());
        m.insert("confidence".to_string(), json!(self.confidence.as_wire()));
        m.insert("source".to_string(), json!(self.source));
        if let Some(d) = &self.detail {
            m.insert("detail".to_string(), json!(d));
        }
        Value::Object(m)
    }
}

/// One conflict entry: caller passed an explicit value but the inferer
/// derived a different value from a recognised source. The inferer NEVER
/// mutates over a caller value — the conflict is surfaced for review only.
#[derive(Debug, Clone)]
pub(super) struct InferenceConflict {
    pub(super) field: &'static str,
    pub(super) caller_value: Value,
    pub(super) inferred_value: Value,
    pub(super) confidence: InferenceConfidence,
    pub(super) source: &'static str,
}

impl InferenceConflict {
    fn to_json(&self) -> Value {
        json!({
            "field": self.field,
            "caller_value": self.caller_value,
            "inferred_value": self.inferred_value,
            "confidence": self.confidence.as_wire(),
            "source": self.source,
        })
    }
}

mod llm;
pub(super) use llm::*;

/// Aggregate inference result attached to the response under
/// `plan_field_inference`. Always carries every field so a caller can
/// pivot on a single shape (`mode`, `inferred_fields[]`,
/// `suggested_fields[]`, `conflicts[]`, `inference_status`,
/// `evidence_sources[]`).
#[derive(Debug, Default)]
pub(super) struct PlanFieldInference {
    pub(super) inferred: Vec<InferredField>,
    pub(super) suggested: Vec<InferredField>,
    pub(super) conflicts: Vec<InferenceConflict>,
    /// Names of evidence sources actually consulted (e.g.
    /// `"plan_sexp"`, `"compiled_from"`, `"evidence_sidecar"`). Surfaced
    /// so observers can tell which knobs the inferer scanned without
    /// reconstructing it from the per-field `source` strings.
    pub(super) evidence_sources: Vec<&'static str>,
    /// wave-20 / task 07 — Sonnet-augmented proposals. Always `None` for
    /// `off` / `preview` / `apply_safe`. Populated only under
    /// `sonnet_suggest`. NEVER auto-applied; surfaced for caller review.
    pub(super) llm: Option<LlmProposalBundle>,
}

impl PlanFieldInference {
    /// Wire status string. Surfaced as `inference_status` on the response.
    pub(super) fn status(&self, mode: InferPlanFieldsMode) -> &'static str {
        match mode {
            InferPlanFieldsMode::Off => "off",
            InferPlanFieldsMode::Preview => {
                if self.inferred.is_empty()
                    && self.suggested.is_empty()
                    && self.conflicts.is_empty()
                {
                    "preview_no_signal"
                } else {
                    "preview"
                }
            }
            InferPlanFieldsMode::ApplySafe => {
                let any_applied = self
                    .inferred
                    .iter()
                    .any(|f| f.confidence.meets_apply_threshold());
                if !any_applied && self.suggested.is_empty() && self.conflicts.is_empty() {
                    "apply_safe_no_signal"
                } else if any_applied {
                    "apply_safe_applied"
                } else {
                    "apply_safe_suggestions_only"
                }
            }
            // wave-20 / task 07 — `sonnet_suggest` reports the deterministic
            // shape under `inference_status` (so observers reading the legacy
            // field still see a meaningful tier) and the Sonnet-specific
            // outcome under `llm_status`. The deterministic block is the
            // same as `preview` for this mode (we never auto-apply).
            InferPlanFieldsMode::SonnetSuggest => {
                if self.inferred.is_empty()
                    && self.suggested.is_empty()
                    && self.conflicts.is_empty()
                {
                    "sonnet_suggest_no_deterministic_signal"
                } else {
                    "sonnet_suggest"
                }
            }
        }
    }

    /// Build the JSON block surfaced under `plan_field_inference` on the
    /// response. Always carries every list (empty when nothing fired) so
    /// observers can pivot on a stable shape.
    pub(super) fn to_response_json(&self, mode: InferPlanFieldsMode) -> Value {
        let inferred: Vec<Value> = self.inferred.iter().map(|f| f.to_json()).collect();
        let suggested: Vec<Value> = self.suggested.iter().map(|f| f.to_json()).collect();
        let conflicts: Vec<Value> = self.conflicts.iter().map(|c| c.to_json()).collect();
        let mut block = json!({
            "mode": mode.as_wire(),
            "inference_status": self.status(mode),
            "inferred_fields": inferred,
            "suggested_fields": suggested,
            "conflicts": conflicts,
            "evidence_sources": self.evidence_sources.iter().map(|s| json!(s)).collect::<Vec<_>>(),
        });
        // wave-20 / task 07 — surface the LLM proposal bundle when it ran.
        // Always emit BOTH `llm_status` AND `llm_proposals[]` for the
        // sonnet_suggest mode so observers can pivot on a stable shape
        // even when the bundle is empty (e.g. LLM returned no usable
        // suggestions). For other modes the keys are omitted entirely so
        // the legacy byte-shape is preserved.
        if matches!(mode, InferPlanFieldsMode::SonnetSuggest) {
            let bundle = self.llm.as_ref();
            let status = bundle
                .map(|b| b.status)
                .unwrap_or(LlmProposalStatus::NotInvoked);
            let proposals: Vec<Value> = bundle
                .map(|b| b.proposals.iter().map(|p| p.to_json()).collect())
                .unwrap_or_default();
            let unavailable_reason = bundle.and_then(|b| b.unavailable_reason.clone());
            let model = bundle.and_then(|b| b.model.clone());
            let request_caller = bundle.and_then(|b| b.request_caller.clone());
            let map = block.as_object_mut().expect("json! object");
            map.insert("llm_status".to_string(), json!(status.as_wire()));
            map.insert("llm_proposals".to_string(), Value::Array(proposals));
            if let Some(reason) = unavailable_reason {
                map.insert("llm_unavailable_reason".to_string(), json!(reason));
            }
            if let Some(model) = model {
                map.insert("llm_model".to_string(), json!(model));
            }
            if let Some(caller) = request_caller {
                map.insert("llm_caller".to_string(), json!(caller));
            }
        }
        block
    }
}

// rules.rs owns deterministic field inference input, rules, and finalize helpers.
mod rules;
pub(super) use rules::{
    caller_bool, caller_str, caller_string_list, compute_plan_field_inference, PlanInferenceInput,
};

// evidence.rs owns evidence-sidecar scanner helpers used by the deterministic inferer.
mod evidence;
use evidence::*;

/// Conservative acceptance-mode canonicaliser. Mirrors the AcceptanceMode
/// allowlist in plan_dag.rs. Returns the wire-form constant or None.
pub(super) fn canonicalize_acceptance_mode(raw: &str) -> Option<&'static str> {
    let lc = raw.trim().to_ascii_lowercase();
    match lc.as_str() {
        "inner_status" | "inner-status" | "innerstatus" => Some("inner_status"),
        "evidence_keys" | "evidence-keys" | "evidencekeys" => Some("evidence_keys"),
        "manual" => Some("manual"),
        _ => None,
    }
}

// apply.rs owns the apply_gate / persisted_apply boundary; this facade
// re-exports it so mission_plan execution callers keep the same API.
mod apply;
pub(super) use apply::*;
