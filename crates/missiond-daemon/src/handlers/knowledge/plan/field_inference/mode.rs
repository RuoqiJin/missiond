use super::*;

/// Wire-form constants for `infer_plan_fields`. Mirror these in the MCP
/// descriptor enum so the two surfaces cannot drift.
pub(in crate::handlers::knowledge::plan) const INFER_MODE_OFF: &str = "off";
pub(in crate::handlers::knowledge::plan) const INFER_MODE_PREVIEW: &str = "preview";
pub(in crate::handlers::knowledge::plan) const INFER_MODE_APPLY_SAFE: &str = "apply_safe";
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
pub(in crate::handlers::knowledge::plan) const INFER_MODE_SONNET_SUGGEST: &str = "sonnet_suggest";

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
pub(in crate::handlers::knowledge::plan) const WORKSTATION_INFER_MODE_OFF: &str = "off";
pub(in crate::handlers::knowledge::plan) const WORKSTATION_INFER_MODE_SONNET_SUGGEST: &str =
    "sonnet_suggest";

/// Resolved workstation-inference mode after argument validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::handlers::knowledge::plan) enum WorkstationInferenceMode {
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
    pub(in crate::handlers::knowledge::plan) fn as_wire(self) -> &'static str {
        match self {
            WorkstationInferenceMode::Off => WORKSTATION_INFER_MODE_OFF,
            WorkstationInferenceMode::SonnetSuggest => WORKSTATION_INFER_MODE_SONNET_SUGGEST,
        }
    }

    /// True when the mode opts into the wave-21 / task 04 LLM proposal
    /// pass. SonnetSuggest is the only opt-in mode in v0.
    pub(in crate::handlers::knowledge::plan) fn is_sonnet_suggest(self) -> bool {
        matches!(self, WorkstationInferenceMode::SonnetSuggest)
    }
}

/// Strict allowlist for the `workstation_inference_mode` knob. Returns
/// the canonical mode or a structured error message. Default (absent /
/// blank / `off`) → `Off` which preserves the wave-15..20 byte-shape.
pub(in crate::handlers::knowledge::plan) fn parse_workstation_inference_mode(
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
pub(in crate::handlers::knowledge::plan) fn refuse_workstation_inference_in_dag_mode(
    args: &Value,
) -> Option<ToolResult> {
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
