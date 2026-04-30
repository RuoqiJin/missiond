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

// apply.rs owns the apply_gate / persisted_apply boundary; this facade
// re-exports it so mission_plan execution callers keep the same API.
mod apply;
pub(super) use apply::*;
