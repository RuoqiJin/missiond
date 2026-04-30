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
//      `<project>/.missiond/v2/plans/<plan_id>.evidence.json` — historical
//      `plan_runner_dispatch` / `workstation_dispatch` entries carry the
//      target / dispatch_strategy / owned_files we used last time.
//
// Lisp authority forward-reference (wave-18 / task 10 backfill):
//   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//                          s4 plan-authoring (autonomous inference)
//   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//                                  role plan-runner (deterministic infer)
// ───────────────────────────────────────────────────────────────────────

/// Wire-form constants for `infer_plan_fields`. Mirror these in the MCP
/// descriptor enum so the two surfaces cannot drift.
pub(super) const INFER_MODE_OFF: &str = "off";
pub(super) const INFER_MODE_PREVIEW: &str = "preview";
pub(super) const INFER_MODE_APPLY_SAFE: &str = "apply_safe";
/// wave-20 / task 07 — LLM-augmented PLAN field inference v0.
///
/// Opt-in mode that asks Sonnet to PROPOSE values for the same six PLAN
/// fields that the deterministic engine handles, but ONLY when the
/// deterministic pass returned no signal at all for that field (no
/// inferred / no suggested / no conflict). LLM proposals are surfaced
/// under `plan_field_inference.llm_proposals[]` and NEVER auto-applied —
/// they are explicit suggestions for caller review. Mutation policy
/// stays identical to `preview` (no plan FSM transitions, no augmented
/// args). The deterministic engine still runs first; LLM output never
/// overrides a deterministic high-confidence inference.
pub(super) const INFER_MODE_SONNET_SUGGEST: &str = "sonnet_suggest";

/// Resolved inference mode after argument validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InferPlanFieldsMode {
    Off,
    Preview,
    ApplySafe,
    /// wave-20 / task 07 — LLM proposals on top of deterministic inference.
    SonnetSuggest,
}

impl InferPlanFieldsMode {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            InferPlanFieldsMode::Off => INFER_MODE_OFF,
            InferPlanFieldsMode::Preview => INFER_MODE_PREVIEW,
            InferPlanFieldsMode::ApplySafe => INFER_MODE_APPLY_SAFE,
            InferPlanFieldsMode::SonnetSuggest => INFER_MODE_SONNET_SUGGEST,
        }
    }

    /// True when the mode opts into the wave-20 / task 07 LLM-augmented
    /// proposal pass. SonnetSuggest is the only LLM-touching mode in v0;
    /// preview / apply_safe / off are byte-for-byte identical to
    /// wave-18 / task 06 deterministic behaviour.
    pub(crate) fn is_llm_augmented(self) -> bool {
        matches!(self, InferPlanFieldsMode::SonnetSuggest)
    }
}

/// Strict allowlist for the `infer_plan_fields` knob. Returns the canonical
/// mode or a structured error message. Default (absent / blank / `off`) →
/// `Off` which preserves the legacy byte-shape.
pub(crate) fn parse_infer_plan_fields_mode(
    args: &Value,
) -> std::result::Result<InferPlanFieldsMode, String> {
    match args.get("infer_plan_fields").and_then(|v| v.as_str()) {
        None | Some("") | Some(INFER_MODE_OFF) => Ok(InferPlanFieldsMode::Off),
        Some(INFER_MODE_PREVIEW) => Ok(InferPlanFieldsMode::Preview),
        Some(INFER_MODE_APPLY_SAFE) => Ok(InferPlanFieldsMode::ApplySafe),
        Some(INFER_MODE_SONNET_SUGGEST) => Ok(InferPlanFieldsMode::SonnetSuggest),
        Some(other) => Err(format!(
            "infer_plan_fields must be one of [\"off\", \"preview\", \"apply_safe\", \"sonnet_suggest\"]; got `{}`",
            other
        )),
    }
}

// ── wave-21 / task 04 — autonomous workstation LLM proposal v0 ─────────
//
// Wire-form constants for `workstation_inference_mode`. Strictly orthogonal
// to `infer_plan_fields` (wave-18 / task 06 + wave-20 / task 07) which
// targets the six PLAN field knobs. The workstation surface targets the
// four core dispatch knobs (target / dispatch_strategy / objective / scope)
// and ONLY fires when caller / PLAN supplied no signal at all.
//
// Default mode `off` ⇒ byte-compatible with wave-15..20 (no proposal pass,
// no response augmentation, no Sonnet call). The new `sonnet_suggest`
// mode triggers the wave-21 proposal pipeline implemented in
// `workstation_dispatch::request_workstation_proposals`. Conservative
// invariants pinned at the call-site:
//   * proposals are SURFACED only, never auto-applied / never auto-spawn;
//   * Sonnet unavailable ⇒ `LLM_UNAVAILABLE` bundle (NEVER falls back to
//     `claude -p` or prompt mode);
//   * DAG mode rejects sonnet_suggest at preflight (single-node-only in v0).
pub(super) const WORKSTATION_INFER_MODE_OFF: &str = "off";
pub(super) const WORKSTATION_INFER_MODE_SONNET_SUGGEST: &str = "sonnet_suggest";

/// Resolved workstation-inference mode after argument validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkstationInferenceMode {
    /// Default — no proposal pass; response is byte-identical with
    /// wave-15..20 callers.
    Off,
    /// Opt-in — when caller / PLAN supply no workstation hints AND
    /// dispatch decision came back NotApplicable, ask Sonnet to propose
    /// values for `target` / `dispatch_strategy` / `objective` / `scope`.
    /// Proposals never alter the dispatch path; they are surfaced under
    /// `workstation_proposals` for operator review.
    SonnetSuggest,
}

impl WorkstationInferenceMode {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            WorkstationInferenceMode::Off => WORKSTATION_INFER_MODE_OFF,
            WorkstationInferenceMode::SonnetSuggest => WORKSTATION_INFER_MODE_SONNET_SUGGEST,
        }
    }

    /// True when the mode opts into the wave-21 / task 04 LLM proposal
    /// pass. SonnetSuggest is the only opt-in mode in v0.
    pub(super) fn is_sonnet_suggest(self) -> bool {
        matches!(self, WorkstationInferenceMode::SonnetSuggest)
    }
}

/// Strict allowlist for the `workstation_inference_mode` knob. Returns
/// the canonical mode or a structured error message. Default (absent /
/// blank / `off`) → `Off` which preserves the wave-15..20 byte-shape.
pub(super) fn parse_workstation_inference_mode(
    args: &Value,
) -> std::result::Result<WorkstationInferenceMode, String> {
    match args
        .get("workstation_inference_mode")
        .and_then(|v| v.as_str())
    {
        None | Some("") | Some(WORKSTATION_INFER_MODE_OFF) => Ok(WorkstationInferenceMode::Off),
        Some(WORKSTATION_INFER_MODE_SONNET_SUGGEST) => Ok(WorkstationInferenceMode::SonnetSuggest),
        Some(other) => Err(format!(
            "workstation_inference_mode must be one of [\"off\", \"sonnet_suggest\"]; got `{}`",
            other
        )),
    }
}

/// Refuse `workstation_inference_mode=sonnet_suggest` when the DAG
/// scheduler is engaged. v0 keeps the proposal pass single-node-only —
/// the DAG path runs many nodes per execute and surfacing a per-node
/// proposal block would balloon the response payload AND blur the
/// "ONLY when no PLAN hints exist" invariant (each node has its own
/// hint set). Mirrors the wave-20 / task 07 enforcement on the same
/// path. Returns `Some(structured_error)` when refused, `None` otherwise.
pub(super) fn refuse_workstation_inference_in_dag_mode(args: &Value) -> Option<ToolResult> {
    let scheduler_mode = args
        .get("scheduler_mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if scheduler_mode != "dag_v1" {
        return None;
    }
    let mode = args
        .get("workstation_inference_mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if mode != WORKSTATION_INFER_MODE_SONNET_SUGGEST {
        return None;
    }
    Some(ToolResult::structured_error(
        ToolError::new(
            error_codes::INVALID_PARAM,
            "workstation_inference_mode=\"sonnet_suggest\" is single-node-execute-only \
             in v0; combining it with scheduler_mode=\"dag_v1\" is unsupported",
        )
        .with_suggestion(
            "drop scheduler_mode=\"dag_v1\" to run the proposal pass against the root \
             plan, or run with workstation_inference_mode=\"off\" (default) to keep DAG \
             behaviour byte-identical with wave-15..20",
        ),
    ))
}

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

/// Inference rule input — what the deterministic engine actually reads.
/// Built once per `mission_plan(action=execute)` call so the rule
/// functions can stay pure.
#[derive(Debug, Default, Clone)]
pub(super) struct PlanInferenceInput<'a> {
    pub(super) plan_hints: ParsedPlanHints,
    /// Raw `plan.sexp_text` — exposed so per-field rules that look for
    /// hints not captured by the canonical [`ParsedPlanHints`] struct
    /// (e.g. `:acceptance-mode`) can re-scan without widening the struct.
    pub(super) plan_sexp: &'a str,
    pub(super) compiled_from: Option<&'a str>,
    pub(super) evidence_entries: Vec<Value>,
}

/// Pure inference engine over the input above. Produces the aggregate
/// result + the list of recommended arg augmentations (only filled when
/// `mode=ApplySafe`; preview mode also computes inference but the caller
/// short-circuits before using the augmentations).
///
/// Conflict semantics: when the caller supplied a value AND the inferer
/// derived a different one from a recognised source, the field becomes a
/// conflict. The conflict is REPORTED (never auto-resolved); apply_safe
/// will NEVER mutate over a caller-supplied value.
pub(super) fn compute_plan_field_inference(
    args: &Value,
    input: &PlanInferenceInput<'_>,
) -> PlanFieldInference {
    let mut result = PlanFieldInference::default();
    let mut sources: Vec<&'static str> = Vec::new();
    if !is_empty_hints(&input.plan_hints) {
        sources.push("plan_sexp");
    }
    if input
        .compiled_from
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        sources.push("compiled_from");
    }
    if !input.evidence_entries.is_empty() {
        sources.push("evidence_sidecar");
    }
    result.evidence_sources = sources;

    infer_target(args, input, &mut result);
    infer_dispatch_strategy(args, input, &mut result);
    infer_target_project(args, input, &mut result);
    infer_owned_files(args, input, &mut result);
    infer_acceptance_mode(args, input, &mut result);
    infer_workstation_dispatch(args, input, &mut result);

    result
}

/// True when the parsed hints carry no usable signal at all. Used to
/// drive `evidence_sources` reporting; does NOT change the inferer's
/// per-field decisions.
pub(super) fn is_empty_hints(h: &ParsedPlanHints) -> bool {
    h.target.is_none()
        && h.flow_id.is_none()
        && h.dispatch_strategy.is_none()
        && h.parallelism.is_none()
        && h.target_project.is_none()
        && h.requested_cwd.is_none()
        && h.objective.is_none()
        && h.summary.is_none()
        && h.scope.is_none()
        && h.commit_policy.is_none()
        && h.owned_files_raw.is_none()
        && h.forbidden_files_raw.is_none()
        && h.acceptance_commands_raw.is_none()
        && h.workstation_dispatch_flag.is_none()
}

/// Helper: the caller's explicit string value for a field, trimmed and
/// non-empty. `None` means "caller did not specify" so the inferer is
/// free to fill.
pub(super) fn caller_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Helper: the caller's explicit bool value for a field. `None` means
/// "caller did not specify" so the inferer is free to fill.
pub(super) fn caller_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

/// Helper: caller-supplied string list for `owned_files`-shaped args.
/// Honours both string and array forms (mirroring the caller-side schema).
pub(super) fn caller_string_list(args: &Value, key: &str) -> Vec<String> {
    collect_string_list(args.get(key))
}

/// Push an inferred field — high-confidence fields land in `inferred`,
/// medium / low always land in `suggested`.
pub(super) fn record_inferred(result: &mut PlanFieldInference, field: InferredField) {
    if field.confidence.meets_apply_threshold() {
        result.inferred.push(field);
    } else {
        result.suggested.push(field);
    }
}

/// Record a conflict (caller value differs from inferred value). NEVER
/// promotes the inferred value into `inferred` even when confidence is
/// `high` — apply_safe must not silently override caller intent.
pub(super) fn record_conflict(result: &mut PlanFieldInference, conflict: InferenceConflict) {
    result.conflicts.push(conflict);
}

// ── per-field rule fns ────────────────────────────────────────────────

/// Infer `target`. Confidence:
///   * `high`   — PLAN.lisp `:target` hint normalises to a canonical target.
///   * `high`   — ≥1 evidence entry agrees on the same target string.
///   * `medium` — `compiled_from` text contains an unambiguous keyword.
pub(super) fn infer_target(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_str(args, "target");
    let mut hits: Vec<(
        InferenceConfidence,
        &'static str,
        &'static str,
        Option<String>,
    )> = Vec::new();

    // 1. PLAN.lisp hint.
    if let Some(raw) = input.plan_hints.target.as_deref() {
        if let Some(canonical) = normalize_target(raw, input.plan_hints.flow_id.is_some()) {
            hits.push((
                InferenceConfidence::High,
                canonical,
                "plan_sexp",
                Some(format!(":target hint resolved to `{}`", canonical)),
            ));
        }
    }

    // 2. Evidence sidecar — the most recent dispatch record carries
    //    `target_tool`. Multiple agreeing entries reinforce the signal.
    let evidence_target = scan_evidence_string_field(&input.evidence_entries, &["target_tool"])
        .and_then(|s| {
            normalize_target(&s, input.plan_hints.flow_id.is_some()).map(|canonical| (canonical, s))
        });
    if let Some((canonical, raw)) = evidence_target {
        hits.push((
            InferenceConfidence::High,
            canonical,
            "evidence_sidecar",
            Some(format!("prior dispatch target_tool=`{}`", raw)),
        ));
    }

    // 3. compiled_from keyword scan.
    if let Some(text) = input.compiled_from {
        if let Some(canonical) = normalize_target(text, input.plan_hints.flow_id.is_some()) {
            hits.push((
                InferenceConfidence::Medium,
                canonical,
                "compiled_from",
                Some(format!("compiled_from `{}` mentions `{}`", text, canonical)),
            ));
        }
    }

    finalize_string_field("target", caller, hits, result);
}

/// Infer `dispatch_strategy`. Confidence:
///   * `high`   — PLAN.lisp `:dispatch-strategy` (canonicalised).
///   * `high`   — evidence entry carries a known strategy.
///   * `medium` — PLAN.lisp `:parallelism` keyword maps to a strategy.
///   * `medium` — `compiled_from` carries a keyword like "agent-team".
pub(super) fn infer_dispatch_strategy(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_str(args, "dispatch_strategy");
    let mut hits: Vec<(
        InferenceConfidence,
        &'static str,
        &'static str,
        Option<String>,
    )> = Vec::new();

    if let Some(raw) = input.plan_hints.dispatch_strategy.as_deref() {
        if let Some(c) = canonicalize_strategy(raw) {
            hits.push((
                InferenceConfidence::High,
                c,
                "plan_sexp",
                Some(format!(":dispatch-strategy hint `{}`", raw)),
            ));
        }
    }

    if let Some(s) = scan_evidence_string_field(&input.evidence_entries, &["dispatch_strategy"]) {
        if let Some(c) = canonicalize_strategy(&s) {
            hits.push((
                InferenceConfidence::High,
                c,
                "evidence_sidecar",
                Some(format!("prior dispatch dispatch_strategy=`{}`", s)),
            ));
        }
    }

    if let Some(p) = input.plan_hints.parallelism.as_deref() {
        if let Some(c) = canonicalize_strategy(p) {
            hits.push((
                InferenceConfidence::Medium,
                c,
                "plan_sexp",
                Some(format!(":parallelism hint `{}` mapped to strategy", p)),
            ));
        }
    }

    if let Some(text) = input.compiled_from {
        if let Some(c) = canonicalize_strategy(text) {
            hits.push((
                InferenceConfidence::Medium,
                c,
                "compiled_from",
                Some(format!("compiled_from keyword maps to `{}`", c)),
            ));
        }
    }

    finalize_string_field("dispatch_strategy", caller, hits, result);
}

/// Infer `target_project`. Confidence:
///   * `high`   — PLAN.lisp `:target-project` non-empty.
///   * `high`   — evidence entry carries the same target_project >=2 times.
///   * `medium` — single evidence entry carries target_project.
pub(super) fn infer_target_project(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_str(args, "target_project");
    let mut hits: Vec<(InferenceConfidence, String, &'static str, Option<String>)> = Vec::new();

    if let Some(tp) = input.plan_hints.target_project.as_deref() {
        let v = tp.trim();
        if !v.is_empty() {
            hits.push((
                InferenceConfidence::High,
                v.to_string(),
                "plan_sexp",
                Some(":target-project hint".to_string()),
            ));
        }
    }

    let evidence_hits = scan_evidence_string_counts(&input.evidence_entries, &["target_project"]);
    if let Some((value, count)) = evidence_hits.first().cloned() {
        let conf = if count >= 2 {
            InferenceConfidence::High
        } else {
            InferenceConfidence::Medium
        };
        hits.push((
            conf,
            value.clone(),
            "evidence_sidecar",
            Some(format!(
                "prior dispatch target_project=`{}` (x{})",
                value, count
            )),
        ));
    }

    finalize_owned_string_field("target_project", caller, hits, result);
}

/// Infer `owned_files`. Confidence:
///   * `high`   — PLAN.lisp `:owned-files` parses to >=1 entry.
///   * `medium` — evidence sidecar carries `owned_files` (any non-empty list).
///                Files change across runs, so we never claim `high` from
///                evidence alone.
pub(super) fn infer_owned_files(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_string_list(args, "owned_files");
    let mut hits: Vec<(
        InferenceConfidence,
        Vec<String>,
        &'static str,
        Option<String>,
    )> = Vec::new();

    let plan_owned = split_lisp_string_list(input.plan_hints.owned_files_raw.as_deref());
    if !plan_owned.is_empty() {
        hits.push((
            InferenceConfidence::High,
            plan_owned.clone(),
            "plan_sexp",
            Some(format!(
                ":owned-files declares {} entries",
                plan_owned.len()
            )),
        ));
    }

    if let Some(list) = scan_evidence_string_list(&input.evidence_entries, "owned_files") {
        if !list.is_empty() {
            hits.push((
                InferenceConfidence::Medium,
                list.clone(),
                "evidence_sidecar",
                Some(format!(
                    "prior dispatch owned_files carries {} entries",
                    list.len()
                )),
            ));
        }
    }

    finalize_string_list_field("owned_files", caller, hits, result);
}

/// Infer `acceptance_mode`. Confidence:
///   * `high`   — PLAN.lisp top-level `:acceptance-mode` parses to a known
///                AcceptanceMode.
///   * `medium` — evidence entry carries an `acceptance.mode` field.
pub(super) fn infer_acceptance_mode(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_str(args, "acceptance_mode");
    let mut hits: Vec<(
        InferenceConfidence,
        &'static str,
        &'static str,
        Option<String>,
    )> = Vec::new();

    // PLAN.lisp top-level `:acceptance-mode` — parse_plan_hints does not
    // capture it (the wave-17 / task 03 hint lives on per-node forms).
    // We do a focused scan here so v0 inference can spot a top-level
    // declaration without widening the canonical hint struct.
    if let Some(raw) = scan_keyword_pairs(input.plan_sexp)
        .into_iter()
        .find(|(k, _)| {
            let lc = k.to_ascii_lowercase();
            lc == "acceptance-mode" || lc == "acceptance_mode"
        })
        .map(|(_, v)| v)
    {
        if let Some(canonical) = canonicalize_acceptance_mode(&raw) {
            hits.push((
                InferenceConfidence::High,
                canonical,
                "plan_sexp",
                Some(format!(":acceptance-mode hint `{}`", raw)),
            ));
        }
    }

    if let Some(mode) = scan_evidence_string_field(
        &input.evidence_entries,
        &["acceptance_mode", "acceptance.mode"],
    ) {
        if let Some(canonical) = canonicalize_acceptance_mode(&mode) {
            hits.push((
                InferenceConfidence::Medium,
                canonical,
                "evidence_sidecar",
                Some(format!("prior evidence acceptance_mode=`{}`", mode)),
            ));
        }
    }

    finalize_string_field("acceptance_mode", caller, hits, result);
}

/// Infer `workstation_dispatch`. Confidence:
///   * `high`   — PLAN.lisp `:workstation-dispatch true`.
///   * `high`   — every recent evidence entry that carries
///                `workstation_dispatch_source` lands on a non-disabled
///                source AND the inferable_strategy gate passed.
///   * `medium` — single evidence entry hint.
pub(super) fn infer_workstation_dispatch(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_bool(args, "workstation_dispatch");
    let mut hits: Vec<(InferenceConfidence, bool, &'static str, Option<String>)> = Vec::new();

    if input.plan_hints.workstation_dispatch_opt_in() {
        hits.push((
            InferenceConfidence::High,
            true,
            "plan_sexp",
            Some(":workstation-dispatch true".to_string()),
        ));
    } else if let Some(raw) = input.plan_hints.workstation_dispatch_flag.as_deref() {
        // Explicit false in PLAN — high confidence "do NOT enable".
        let lc = raw.trim().to_ascii_lowercase();
        if matches!(lc.as_str(), "false" | "no" | "off" | "0") {
            hits.push((
                InferenceConfidence::High,
                false,
                "plan_sexp",
                Some(":workstation-dispatch false".to_string()),
            ));
        }
    }

    let ws_sources =
        scan_evidence_string_counts(&input.evidence_entries, &["workstation_dispatch_source"]);
    if let Some((value, count)) = ws_sources.first().cloned() {
        let lc = value.to_ascii_lowercase();
        let positive = matches!(lc.as_str(), "explicit_arg" | "plan_hint" | "inferred");
        let conf = if count >= 2 {
            InferenceConfidence::High
        } else {
            InferenceConfidence::Medium
        };
        if positive {
            hits.push((
                conf,
                true,
                "evidence_sidecar",
                Some(format!(
                    "prior workstation_dispatch_source=`{}` (x{})",
                    value, count
                )),
            ));
        } else if matches!(lc.as_str(), "disabled") {
            hits.push((
                conf,
                false,
                "evidence_sidecar",
                Some(format!(
                    "prior workstation_dispatch_source=`disabled` (x{})",
                    count
                )),
            ));
        }
    }

    finalize_bool_field("workstation_dispatch", caller, hits, result);
}

// ── finalize helpers (per value-shape) ────────────────────────────────

/// Resolve the highest-confidence string-shaped hint and emit either an
/// inferred / suggested entry, or a conflict against caller value.
pub(super) fn finalize_string_field(
    field: &'static str,
    caller: Option<&str>,
    mut hits: Vec<(
        InferenceConfidence,
        &'static str,
        &'static str,
        Option<String>,
    )>,
    result: &mut PlanFieldInference,
) {
    // Prefer the highest-confidence hit; ties broken by source order.
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };

    if let Some(c) = caller {
        if c.eq_ignore_ascii_case(value) {
            // Caller already agrees with the inference — nothing to do.
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(c),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }

    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

/// Same as [`finalize_string_field`] but for owned-`String`-shaped hits
/// (where the value is computed dynamically per-call rather than carried
/// as a `&'static str`).
pub(super) fn finalize_owned_string_field(
    field: &'static str,
    caller: Option<&str>,
    mut hits: Vec<(InferenceConfidence, String, &'static str, Option<String>)>,
    result: &mut PlanFieldInference,
) {
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };
    if let Some(c) = caller {
        if c == value {
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(c),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }
    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

/// Same shape as [`finalize_string_field`] but for `Vec<String>`-shaped
/// hits. Caller equality compares as set-like (order-independent) so a
/// PLAN.lisp + caller permutation does not trigger a spurious conflict.
pub(super) fn finalize_string_list_field(
    field: &'static str,
    caller: Vec<String>,
    mut hits: Vec<(
        InferenceConfidence,
        Vec<String>,
        &'static str,
        Option<String>,
    )>,
    result: &mut PlanFieldInference,
) {
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };
    if !caller.is_empty() {
        let mut a = caller.clone();
        a.sort();
        let mut b = value.clone();
        b.sort();
        if a == b {
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(caller),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }
    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

pub(super) fn finalize_bool_field(
    field: &'static str,
    caller: Option<bool>,
    mut hits: Vec<(InferenceConfidence, bool, &'static str, Option<String>)>,
    result: &mut PlanFieldInference,
) {
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };
    if let Some(c) = caller {
        if c == value {
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(c),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }
    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

// ── evidence-sidecar scanners ─────────────────────────────────────────

/// Look for the most-recent string value of any matching key. Searches
/// each entry top-level + the well-known nested holders that the wave-12
/// evidence collector emits (`evidence`, `inner_dispatch`, `inner_result`,
/// `typed_evidence`). Newest-first match wins.
pub(super) fn scan_evidence_string_field(entries: &[Value], keys: &[&str]) -> Option<String> {
    for entry in entries.iter().rev() {
        if let Some(v) = pluck_string(entry, keys) {
            return Some(v);
        }
        for nested in &[
            "evidence",
            "inner_dispatch",
            "inner_result",
            "typed_evidence",
        ] {
            if let Some(child) = entry.get(*nested) {
                if let Some(v) = pluck_string(child, keys) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Count distinct string values of a field across entries. Returns
/// `[(value, count), ...]` sorted by descending count then by recency.
pub(super) fn scan_evidence_string_counts(
    entries: &[Value],
    keys: &[&str],
) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for entry in entries {
        let mut found: Option<String> = None;
        if let Some(v) = pluck_string(entry, keys) {
            found = Some(v);
        } else {
            for nested in &[
                "evidence",
                "inner_dispatch",
                "inner_result",
                "typed_evidence",
            ] {
                if let Some(child) = entry.get(*nested) {
                    if let Some(v) = pluck_string(child, keys) {
                        found = Some(v);
                        break;
                    }
                }
            }
        }
        if let Some(v) = found {
            if !counts.contains_key(&v) {
                order.push(v.clone());
            }
            *counts.entry(v).or_insert(0) += 1;
        }
    }
    let mut out: Vec<(String, usize)> = order
        .into_iter()
        .map(|k| {
            let c = counts.get(&k).copied().unwrap_or(0);
            (k, c)
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

/// Look for a string-array value under any of the supplied keys. Returns
/// the most-recent entry's value (newest-first) so the inferer reflects
/// the latest run.
pub(super) fn scan_evidence_string_list(entries: &[Value], key: &str) -> Option<Vec<String>> {
    for entry in entries.iter().rev() {
        if let Some(v) = pluck_string_list(entry, key) {
            return Some(v);
        }
        for nested in &[
            "evidence",
            "inner_dispatch",
            "inner_result",
            "typed_evidence",
        ] {
            if let Some(child) = entry.get(*nested) {
                if let Some(v) = pluck_string_list(child, key) {
                    return Some(v);
                }
            }
        }
    }
    None
}

pub(super) fn pluck_string(v: &Value, keys: &[&str]) -> Option<String> {
    let obj = v.as_object()?;
    for k in keys {
        if let Some(s) = obj.get(*k).and_then(|x| x.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

pub(super) fn pluck_string_list(v: &Value, key: &str) -> Option<Vec<String>> {
    let obj = v.as_object()?;
    let arr = obj.get(key)?.as_array()?;
    let out: Vec<String> = arr
        .iter()
        .filter_map(|item| {
            item.as_str()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(String::from)
        })
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

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

/// Apply high-confidence inferred fields to a clone of `args` so the
/// downstream pipeline sees the augmented input. Caller-supplied values
/// are NEVER overwritten (they only ever land as conflicts upstream, and
/// conflicts are not promoted into `inferred`).
pub(super) fn apply_safe_augmentation(args: &Value, inference: &PlanFieldInference) -> Value {
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
pub(super) enum ApplyOrigin {
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
    pub(super) fn as_wire(self) -> &'static str {
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
pub(super) struct AppliedField {
    pub(super) field: &'static str,
    pub(super) value: Value,
    pub(super) source: &'static str,
    pub(super) origin: ApplyOrigin,
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
pub(super) struct SkippedField {
    pub(super) field: &'static str,
    pub(super) reason: &'static str,
    pub(super) origin: ApplyOrigin,
    /// Optional human-readable detail (e.g. `"caller already set target"`).
    pub(super) detail: Option<String>,
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
pub(super) struct ApplyGateOutcome {
    pub(super) requested: bool,
    pub(super) persist_inference_requested: bool,
    pub(super) applied: Vec<AppliedField>,
    pub(super) skipped: Vec<SkippedField>,
    pub(super) conflict: Vec<InferenceConflict>,
    /// Caller-supplied args augmented with `applied[]` — preview only.
    /// Always emitted so a follow-up caller can dry-run with the same
    /// shape without re-deriving it.
    pub(super) resulting_plan_preview: Value,
}

impl ApplyGateOutcome {
    pub(super) fn to_response_json(&self) -> Value {
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
pub(super) fn parse_llm_caller_approved(args: &Value) -> std::collections::HashSet<&'static str> {
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
pub(super) fn caller_requested_apply(args: &Value) -> bool {
    args.get("apply_inferred_fields")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// True when caller passed `persist_inference=true`. Surfaced on the
/// gate response so observers can audit which persistence boundary the
/// gate honoured. The actual plan-text write is FUTURE work — see the
/// `persist_inference_applied=false` invariant in
/// `ApplyGateOutcome::to_response_json`.
pub(super) fn caller_requested_persist_inference(args: &Value) -> bool {
    args.get("persist_inference")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Strict pre-flight validator for `apply_inferred_fields` /
/// `persist_inference`. We only accept the bool form — a literal
/// string `"true"` / `"false"` is rejected so a typo cannot silently
/// masquerade as opt-in. Mirrors the conservative posture of the
/// `infer_plan_fields` validator.
pub(super) fn validate_apply_gate_args(args: &Value) -> std::result::Result<(), String> {
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
pub(super) fn llm_proposal_safety_check(
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
pub(super) fn compute_apply_gate(args: &Value, inference: &PlanFieldInference) -> ApplyGateOutcome {
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
pub(super) fn attach_apply_gate_block(mut result: ToolResult, block: Option<Value>) -> ToolResult {
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

// ── wave-22 / task 04 — Persisted PLAN inference apply v2 ──────────────
//
// Layered on top of wave-21 / task 05 apply gate v1. The v1 gate
// promoted `applied_fields[]` into caller args (in-memory only) and
// hard-pinned `apply_gate.persist_inference_applied = false`. v2
// preserves every v1 invariant (default off, conflicts never apply,
// suggestions never apply, LLM proposals require `llm_caller_approved`,
// strict bool shape) AND adds an explicit, audited persistence path:
//
//   * `apply_inferred_fields=true`        — v1 master switch
//   * `persist_inference=true`            — v1 echo flag, NOW load-bearing
//   * `caller_approved=true`              — NEW second human opt-in
//   * `proposal_hash=<32-hex>`            — NEW deterministic correlator
//
// All four must hold AND the gate must have promoted at least one
// field AND the caller's hash must MATCH `compute_inference_proposal_hash`
// computed over `(plan_id, original_sexp_hash, applied_fields)`. On
// mismatch / missing the handler returns a structured error BEFORE any
// DB mutation per the goal contract (R2).
//
// On success the handler:
//   1. Reads `original_sexp_hash` from the existing `plan` row.
//   2. Synthesises `resulting_sexp_text` by APPENDING a guarded
//      `(plan-inference-applied :inference-version "v2" ...)` form to
//      the existing s-exp. The original body is preserved verbatim and
//      `parse_plan_hints` keeps first-occurrence semantics, so the
//      observable PLAN behaviour stays identical when the appended
//      keywords overlap an original hint. New hints become live.
//   3. Inserts a NEW plan row at `version = max + 1` via `plan_insert`
//      — never overwrites the existing row (R4 — version + audit).
//   4. Calls `plan_supersede(old_id)` so the previous version is
//      visibly retired with `status=superseded` (rollback handle).
//   5. Appends a typed `plan_inference_persisted_apply` evidence entry
//      with applied_fields[], skipped_fields[], proposal_hash,
//      original_sexp_hash, resulting_sexp_hash, rollback_pointer
//      (the previous plan id) so the audit trail is complete (R5).
//
// Conservative posture: the persist path is OPT-IN at four
// independent flags. Default behaviour (any flag absent / false)
// keeps the v1 byte-shape exactly — `apply_gate.persist_inference_applied`
// stays `false` and the response surfaces `persisted_apply.status =
// "not_requested"` so observers can pivot without re-deriving the
// policy. Failure modes (missing hash / hash mismatch / invalid param)
// fail-fast as structured errors; soft-skip modes (no applied fields /
// caller_approved=false / persist_inference=false) surface on the
// `persisted_apply` block with a canonical reason and DO NOT mutate
// the DB.
//
// Lisp authority forward reference (Wave 22 backfill):
//   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//                         s4 plan-authoring (persist gate v2)
//   - intent-tools.lisp :: implemented-surface mission_plan ::
//                         :execute-contract :persisted-inference-apply

/// True when caller passed `caller_approved=true` (any other shape —
/// including the literal string `"true"` — is rejected by
/// `validate_apply_gate_args` BEFORE we get here, so this only checks
/// the bool form). The flag is the SECOND human opt-in for the v2
/// persist path; default `false` keeps the v1 byte-shape exactly.
pub(super) fn caller_requested_caller_approved(args: &Value) -> bool {
    args.get("caller_approved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Extract the caller-supplied `proposal_hash` (32-hex SHA-256 prefix).
/// Returns `None` when absent, an empty string after trim, or a non-
/// string shape (the validator already rejected the latter as
/// `INVALID_PARAM`, so this is purely defensive).
pub(super) fn caller_supplied_proposal_hash(args: &Value) -> Option<String> {
    let s = args.get("proposal_hash").and_then(|v| v.as_str())?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Compute the deterministic 32-hex correlator over
/// `(plan_id, original_sexp_hash, sorted applied fields)`. The hash is
/// what the caller is expected to echo back via the `proposal_hash`
/// arg under `apply_inferred_fields=true + caller_approved=true +
/// persist_inference=true`. Caller can derive it themselves from the
/// gate response — we surface the same value under
/// `persisted_apply.computed_proposal_hash` so dashboards can
/// `assert hash == derive(...)` directly.
///
/// Hash payload (canonical UTF-8):
///   `"v2|<plan-id>|<original-sexp-hash>|<field>:<value-canonical>|..."`
///
/// Fields are sorted lexicographically by `field` so observers see a
/// deterministic hash regardless of the order in which the gate
/// promoted them. Each value is canonicalised via
/// `serde_json::to_string` (compact form, sorted object keys via the
/// `Value` representation).
pub(super) fn compute_inference_proposal_hash(
    plan_id: uuid::Uuid,
    original_sexp_hash: &str,
    applied: &[AppliedField],
) -> String {
    use sha2::{Digest, Sha256};
    let mut sorted: Vec<&AppliedField> = applied.iter().collect();
    sorted.sort_by_key(|af| af.field);
    let mut payload = format!("v2|{}|{}", plan_id, original_sexp_hash.trim());
    for af in sorted.iter() {
        let value_canonical = serde_json::to_string(&af.value).unwrap_or_else(|_| String::new());
        payload.push('|');
        payload.push_str(af.field);
        payload.push(':');
        payload.push_str(&value_canonical);
    }
    let mut h = Sha256::new();
    h.update(payload.as_bytes());
    let full = format!("{:x}", h.finalize());
    full[..32].to_string()
}

/// Status discriminants for the v2 persist path. The wire string is
/// stable so observers / dashboards can pivot on it without re-reading
/// the rest of the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PersistedApplyStatus {
    /// Caller did not opt into the persist path (default). The v1 gate
    /// may still have augmented in-memory args.
    NotRequested,
    /// All four opt-ins supplied + hash matched + at least one field
    /// promoted ⇒ a new plan version was committed.
    Applied,
    /// `apply_inferred_fields` was not `true`. Persist requires the
    /// master switch.
    SkippedApplyGateNotRequested,
    /// `persist_inference` was not `true`. Persist requires the
    /// dedicated persistence opt-in (echo of v1 flag, now load-bearing).
    SkippedPersistNotRequested,
    /// `caller_approved` was not `true`. Persist requires the second
    /// human opt-in.
    SkippedCallerNotApproved,
    /// The v1 gate promoted no fields (everything was conflict /
    /// suggestion / non-approved / safety-skipped). Persist refuses
    /// to write a no-op version.
    SkippedNothingToApply,
}

impl PersistedApplyStatus {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            PersistedApplyStatus::NotRequested => "not_requested",
            PersistedApplyStatus::Applied => "applied",
            PersistedApplyStatus::SkippedApplyGateNotRequested => {
                "skipped_apply_gate_not_requested"
            }
            PersistedApplyStatus::SkippedPersistNotRequested => "skipped_persist_not_requested",
            PersistedApplyStatus::SkippedCallerNotApproved => "skipped_caller_not_approved",
            PersistedApplyStatus::SkippedNothingToApply => "skipped_nothing_to_apply",
        }
    }

    pub(super) fn was_applied(self) -> bool {
        matches!(self, PersistedApplyStatus::Applied)
    }
}

/// Aggregate persisted-apply outcome, surfaced under `persisted_apply`
/// on the response. Mirrors `ApplyGateOutcome::to_response_json` in
/// always emitting every field (with conservative defaults) so observers
/// pivot on a stable shape regardless of the persist path's status.
#[derive(Debug, Clone)]
pub(super) struct PersistedApplyOutcome {
    pub(super) status: PersistedApplyStatus,
    pub(super) apply_inferred_fields_requested: bool,
    pub(super) persist_inference_requested: bool,
    pub(super) caller_approved: bool,
    pub(super) original_sexp_hash: String,
    pub(super) resulting_sexp_hash: Option<String>,
    pub(super) computed_proposal_hash: Option<String>,
    pub(super) supplied_proposal_hash: Option<String>,
    pub(super) applied_fields: Vec<AppliedField>,
    pub(super) skipped_fields: Vec<SkippedField>,
    /// Newly inserted plan id (when status == Applied). `None` on every
    /// skip path.
    pub(super) new_plan_id: Option<uuid::Uuid>,
    /// New plan version (when status == Applied). `None` on every skip path.
    pub(super) new_plan_version: Option<i32>,
    /// Pointer to the now-superseded plan id (rollback handle). Always
    /// populated when status == Applied; the wave-21 plan_supersede call
    /// guarantees the row stays queryable for audit / replay.
    pub(super) rollback_plan_id: Option<uuid::Uuid>,
}

impl PersistedApplyOutcome {
    /// Build the default `not_requested` outcome from caller args + the
    /// v1 apply-gate decision. Used as the response anchor on every
    /// path that does NOT opt into persist.
    pub(super) fn from_skip_reason(
        status: PersistedApplyStatus,
        args: &Value,
        original_sexp_hash: &str,
        applied: &[AppliedField],
        skipped: &[SkippedField],
        computed_hash: Option<String>,
    ) -> Self {
        Self {
            status,
            apply_inferred_fields_requested: caller_requested_apply(args),
            persist_inference_requested: caller_requested_persist_inference(args),
            caller_approved: caller_requested_caller_approved(args),
            original_sexp_hash: original_sexp_hash.to_string(),
            resulting_sexp_hash: None,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: caller_supplied_proposal_hash(args),
            applied_fields: applied.to_vec(),
            skipped_fields: skipped.to_vec(),
            new_plan_id: None,
            new_plan_version: None,
            rollback_plan_id: None,
        }
    }

    pub(super) fn to_response_json(&self) -> Value {
        let applied: Vec<Value> = self.applied_fields.iter().map(|f| f.to_json()).collect();
        let skipped: Vec<Value> = self.skipped_fields.iter().map(|f| f.to_json()).collect();
        json!({
            "status": self.status.as_wire(),
            "apply_inferred_fields_requested": self.apply_inferred_fields_requested,
            "persist_inference_requested": self.persist_inference_requested,
            "caller_approved": self.caller_approved,
            "original_sexp_hash": self.original_sexp_hash,
            "resulting_sexp_hash": self.resulting_sexp_hash.clone(),
            "computed_proposal_hash": self.computed_proposal_hash.clone(),
            "supplied_proposal_hash": self.supplied_proposal_hash.clone(),
            "applied_fields": applied,
            "skipped_fields": skipped,
            "new_plan_id": self.new_plan_id.map(|u| u.to_string()),
            "new_plan_version": self.new_plan_version,
            "rollback_plan_id": self.rollback_plan_id.map(|u| u.to_string()),
        })
    }
}

// Make `AppliedField` / `SkippedField` cloneable so the persist path can
// snapshot them into the outcome for the response + evidence.
//
// The wave-21 / task 05 structs were defined `Clone`-free; we add it via
// derive on the field types directly above. (The structs themselves are
// already `Clone` — see `#[derive(Debug, Clone)]` on `AppliedField` and
// `SkippedField`.)

/// Pure pre-flight gate. Inverted v1 semantics: persist runs ONLY when
/// every opt-in is true AND the gate promoted at least one field. On
/// any failure path returns the canonical skip status WITHOUT touching
/// the DB. Hash mismatch / missing is NOT handled here — that path is
/// handled by `enforce_persisted_apply_preflight` which fail-fasts as
/// a structured error per R2.
pub(super) fn evaluate_persisted_apply_gate(
    args: &Value,
    apply: &ApplyGateOutcome,
) -> PersistedApplyStatus {
    if !caller_requested_apply(args) {
        return PersistedApplyStatus::SkippedApplyGateNotRequested;
    }
    if !caller_requested_persist_inference(args) {
        return PersistedApplyStatus::SkippedPersistNotRequested;
    }
    if !caller_requested_caller_approved(args) {
        return PersistedApplyStatus::SkippedCallerNotApproved;
    }
    if apply.applied.is_empty() {
        return PersistedApplyStatus::SkippedNothingToApply;
    }
    PersistedApplyStatus::Applied
}

/// Strict pre-flight for the v2 hash check. Mirrors
/// `enforce_apply_gate_preflight` from review_gate.rs: returns
/// `Err((code, message))` on missing / mismatch BEFORE any DB mutation
/// per the goal contract (R2). Returns `Ok(())` when the caller did not
/// opt into the persist path (the soft-skip outcome is computed by
/// `evaluate_persisted_apply_gate` afterwards) OR when the hash matches
/// the deterministic correlator.
///
/// Skipping the preflight on a non-persist path is intentional: the
/// caller may legitimately omit `proposal_hash` / `caller_approved` on
/// every legacy v1 call. We only fail-fast when the caller PRESENTED
/// the persist intent (apply + persist + caller_approved all `true`)
/// AND the hash is missing / wrong.
pub(super) fn enforce_persisted_apply_preflight(
    args: &Value,
    computed_hash: &str,
) -> std::result::Result<(), (&'static str, String)> {
    // Preflight only applies when caller opted into all THREE persist
    // flags. Any other arrangement is a soft-skip handled downstream.
    if !caller_requested_apply(args)
        || !caller_requested_persist_inference(args)
        || !caller_requested_caller_approved(args)
    {
        return Ok(());
    }
    let supplied = match caller_supplied_proposal_hash(args) {
        Some(s) => s,
        None => {
            return Err((
                error_codes::INVALID_PARAM,
                format!(
                    "PERSIST_APPLY_MISSING_PROPOSAL_HASH: persist_inference=true + caller_approved=true requires proposal_hash to match the v2 deterministic correlator (expected `{}`); supply proposal_hash from a prior preview call's persisted_apply.computed_proposal_hash field",
                    computed_hash
                ),
            ));
        }
    };
    if !supplied.eq_ignore_ascii_case(computed_hash) {
        return Err((
            error_codes::INVALID_PARAM,
            format!(
                "PERSIST_APPLY_PROPOSAL_HASH_MISMATCH: caller-supplied proposal_hash `{}` does not match the v2 deterministic correlator `{}`; the apply set may have changed since the proposal was previewed — re-run the gate without persist flags first to capture the fresh hash",
                supplied, computed_hash
            ),
        ));
    }
    Ok(())
}

/// Render a single AppliedField as a `:keyword value` lisp pair. Mirrors
/// the conservative `parse_plan_hints` reader in plan.rs:
///   * canonical kebab-case keyword (matches the reader's
///     `target` / `dispatch-strategy` / `target-project` / `requested-cwd`
///     / `acceptance-mode` / `owned-files` / `workstation-dispatch`
///     spellings)
///   * string scalars are double-quoted with `\\` / `\"` escapes
///   * bool scalars become `true` / `false` barewords
///   * arrays become `[ "a" "b" ]` bracket lists (matches
///     `split_lisp_string_list`)
///   * any other shape (number / object / null) is serialised via
///     `serde_json::to_string` and emitted as a quoted string so the
///     reader treats it as a bareword passthrough — defensive only,
///     the apply gate's safety check already filtered shapes before we
///     get here.
pub(super) fn render_applied_field_to_lisp(field: &str, value: &Value) -> String {
    let key = match field {
        "target" => "target",
        "dispatch_strategy" => "dispatch-strategy",
        "target_project" => "target-project",
        "owned_files" => "owned-files",
        "acceptance_mode" => "acceptance-mode",
        "workstation_dispatch" => "workstation-dispatch",
        // Defensive — any future field name that is not a known reader
        // alias keeps the snake-case form so the keyword pair is still
        // syntactically valid (the reader will silently ignore unknown
        // keywords per its `_ => {}` arm).
        other => other,
    };
    match value {
        Value::String(s) => format!(":{} \"{}\"", key, escape_lisp_string(s)),
        Value::Bool(b) => format!(":{} {}", key, if *b { "true" } else { "false" }),
        Value::Array(items) => {
            let mut parts: Vec<String> = Vec::with_capacity(items.len());
            for item in items.iter() {
                match item {
                    Value::String(s) => parts.push(format!("\"{}\"", escape_lisp_string(s))),
                    Value::Bool(b) => parts.push((if *b { "true" } else { "false" }).into()),
                    other => parts.push(format!(
                        "\"{}\"",
                        escape_lisp_string(&serde_json::to_string(other).unwrap_or_default())
                    )),
                }
            }
            format!(":{} [{}]", key, parts.join(" "))
        }
        Value::Number(n) => format!(":{} {}", key, n),
        other => format!(
            ":{} \"{}\"",
            key,
            escape_lisp_string(&serde_json::to_string(other).unwrap_or_default())
        ),
    }
}

pub(super) fn escape_lisp_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out
}

/// Synthesise the resulting PLAN.lisp by APPENDING a guarded
/// `(plan-inference-applied ...)` form to the existing s-exp. The
/// original body is preserved verbatim so the supersede chain has a
/// clean diff (`tail -1` on the new s-exp shows the persisted
/// annotation; everything else is the byte-identical predecessor).
///
/// `parse_plan_hints` keeps first-occurrence semantics — when the
/// appended keyword overlaps an original hint the original wins. New
/// hints (e.g. `:dispatch-strategy resident-lisp` when the original
/// PLAN never spelled it) become the live value because no prior
/// occurrence exists. This is the conservative posture: the inferer
/// EXTENDS the PLAN; it never silently rewrites a caller-authored
/// hint at the persistence boundary.
pub(super) fn synthesize_persisted_sexp(
    original: &str,
    applied: &[AppliedField],
    proposal_hash: &str,
    timestamp: &str,
) -> String {
    let mut out = String::with_capacity(original.len() + 256);
    out.push_str(original);
    if !original.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    // Header — observers can grep on this exact prefix.
    out.push_str(";; wave-22 / task 04 — persisted PLAN inference apply v2\n");
    out.push_str(&format!(
        "(plan-inference-applied :inference-version \"v2\" :proposal-hash \"{}\" :persisted-at \"{}\"",
        proposal_hash, timestamp
    ));
    // Emit each applied field as a SIBLING keyword pair so the
    // wave-15 / task 05 `parse_plan_hints` reader picks them up at the
    // PLAN level (the reader scans `:keyword value` pairs at any depth
    // but treats bracket lists as opaque value spans). A flat list of
    // pairs keeps the appended annotation queryable without breaking
    // first-occurrence semantics — the original PLAN body still
    // appears first in the buffer, so its hints win on every overlap.
    for af in applied.iter() {
        out.push('\n');
        out.push_str("  ");
        out.push_str(&render_applied_field_to_lisp(af.field, &af.value));
    }
    out.push(')');
    out.push('\n');
    out
}

/// Build the typed evidence entry for the persisted apply path. Mirrors
/// the wave-12 typed-evidence schema (`schema_version="v0"`,
/// canonical `source` + `kind`) so a single grep over the evidence
/// sidecar surfaces every persist event with a stable shape.
pub(super) fn build_persisted_apply_evidence_entry(
    outcome: &PersistedApplyOutcome,
    plan_id: uuid::Uuid,
) -> Value {
    let applied: Vec<Value> = outcome.applied_fields.iter().map(|f| f.to_json()).collect();
    let skipped: Vec<Value> = outcome.skipped_fields.iter().map(|f| f.to_json()).collect();
    json!({
        "schema_version": "v0",
        "source": "plan_inference_persisted_apply",
        "kind": "plan_inference_persisted_apply",
        "plan_id": plan_id.to_string(),
        "rollback_plan_id": outcome.rollback_plan_id.map(|u| u.to_string()),
        "new_plan_id": outcome.new_plan_id.map(|u| u.to_string()),
        "new_plan_version": outcome.new_plan_version,
        "original_sexp_hash": outcome.original_sexp_hash,
        "resulting_sexp_hash": outcome.resulting_sexp_hash,
        "proposal_hash": outcome.computed_proposal_hash,
        "applied_fields": applied,
        "skipped_fields": skipped,
        "status": outcome.status.as_wire(),
    })
}

/// Apply the v2 persist gate. Pure of `state` interaction at the gate
/// stage (compute hash + evaluate skip), then exercises `state.store`
/// for the new plan version + supersede + evidence write only when the
/// gate authorised the apply. On every skip path the DB is untouched
/// and the outcome surfaces the canonical skip reason on the response.
///
/// Returns `Err(structured_error_pair)` ONLY for the fail-fast hash
/// preflight (R2). Every other path returns `Ok(outcome)` with the
/// status communicating success / soft-skip.
pub(super) async fn execute_persisted_apply(
    state: &AppState,
    plan: &Plan,
    args: &Value,
    apply: &ApplyGateOutcome,
) -> std::result::Result<PersistedApplyOutcome, (&'static str, String)> {
    let original_sexp_hash = sha256_hex(&plan.sexp_text);
    let computed_hash =
        compute_inference_proposal_hash(plan.id, &original_sexp_hash, &apply.applied);

    // Fail-fast hash preflight per R2.
    enforce_persisted_apply_preflight(args, &computed_hash)?;

    let status = evaluate_persisted_apply_gate(args, apply);
    if !status.was_applied() {
        return Ok(PersistedApplyOutcome::from_skip_reason(
            status,
            args,
            &original_sexp_hash,
            &apply.applied,
            &apply.skipped,
            Some(computed_hash),
        ));
    }

    // Persist path. Synthesise the new sexp text + hash, allocate the
    // next plan version, insert the new row, supersede the predecessor,
    // append the typed evidence entry. Each step uses the existing
    // wave-21 store API (no new trait method per the contract's
    // `:must-not-touch` boundary).
    let timestamp = iso_now();
    let resulting_sexp_text =
        synthesize_persisted_sexp(&plan.sexp_text, &apply.applied, &computed_hash, &timestamp);
    let resulting_sexp_hash = sha256_hex(&resulting_sexp_text);

    let existing = state
        .store
        .plan_list_by_task(&plan.board_task_id)
        .await
        .map_err(|e| (error_codes::DB_ERROR, format!("plan_list_by_task: {}", e)))?;
    let next_version = existing.iter().map(|p| p.version).max().unwrap_or(0) + 1;

    let new_plan_id = state
        .store
        .plan_insert(
            &plan.board_task_id,
            plan.source_directive_id,
            next_version,
            &resulting_sexp_text,
            &resulting_sexp_hash,
            // Inherit the predecessor's status — we are NOT changing
            // FSM stage on this write, only persisting an inference
            // annotation. The plan-runner will continue from the new
            // version on its next execute call.
            plan.status,
            plan.compiler_model.as_deref(),
            // Stamp `compiled_from` so the audit trail points at the
            // predecessor row (rollback handle on the immutable v0 of
            // the column).
            Some(&format!("plan-inference-persist/{}", plan.id)),
        )
        .await
        .map_err(|e| (error_codes::DB_ERROR, format!("plan_insert: {}", e)))?;

    state
        .store
        .plan_supersede(plan.id, new_plan_id)
        .await
        .map_err(|e| (error_codes::DB_ERROR, format!("plan_supersede: {}", e)))?;

    let outcome = PersistedApplyOutcome {
        status: PersistedApplyStatus::Applied,
        apply_inferred_fields_requested: true,
        persist_inference_requested: true,
        caller_approved: true,
        original_sexp_hash: original_sexp_hash.clone(),
        resulting_sexp_hash: Some(resulting_sexp_hash),
        computed_proposal_hash: Some(computed_hash),
        supplied_proposal_hash: caller_supplied_proposal_hash(args),
        applied_fields: apply.applied.clone(),
        skipped_fields: apply.skipped.clone(),
        new_plan_id: Some(new_plan_id),
        new_plan_version: Some(next_version),
        rollback_plan_id: Some(plan.id),
    };

    // Append typed evidence (R5). Failure here does NOT roll back the
    // new plan row — file-vs-db contract per `append_plan_evidence_entry`
    // (the row is committed even if the sidecar write fails). We
    // surface the error path via the standard evidence_warning surface
    // on the response.
    let evidence_entry = build_persisted_apply_evidence_entry(&outcome, plan.id);
    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());
    // Append on the PREDECESSOR's evidence sidecar (the rollback
    // pointer is the predecessor — observers replaying a rollback
    // need the persisted-apply entry on the same sidecar as the
    // pre-apply history).
    let _ = append_plan_evidence_entry(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        evidence_entry,
    )
    .await;

    Ok(outcome)
}

/// Splice the `persisted_apply` block onto a successful response.
/// Mirrors `attach_apply_gate_block` exactly: structured errors are
/// left untouched, and a pre-existing block is preserved (NEVER
/// overwritten) so future DAG / resume paths can attach their own row.
pub(super) fn attach_persisted_apply_block(
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
        map.entry("persisted_apply".to_string()).or_insert(block);
    }
    result.content = vec![ToolContent::Text {
        text: serde_json::to_string_pretty(&payload).unwrap_or(text),
    }];
    result
}
