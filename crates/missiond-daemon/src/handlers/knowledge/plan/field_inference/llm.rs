use super::*;

// ── wave-20 / task 07 — LLM proposal validation + bundle ─────────────────
//
// The Sonnet-augmented mode produces a STRUCTURED proposal list. Every
// proposal carries:
//   * `field`            — one of the six v0 PLAN fields (allowlist below).
//   * `value`            — JSON value matching the expected field shape
//                          (string / boolean / list of strings).
//   * `confidence`       — high|medium|low (apply policy is OFF for v0;
//                          surfaced for caller use).
//   * `evidence`         — short justification string (LLM-supplied).
//   * `conflict_status`  — "none" | "conflicts_with_caller" |
//                          "conflicts_with_deterministic" — never
//                          auto-resolved.
//
// Validation rejects unknown fields, missing keys, value shape mismatch,
// and unknown confidence strings. Failures land on `parse_warnings[]` so
// observers can audit which proposals were dropped without an exception
// killing the rest.

/// Allowlisted PLAN fields the LLM may propose values for (mirrors the
/// six fields handled by the deterministic engine).
pub(in crate::handlers::knowledge::plan) const LLM_ALLOWED_FIELDS: &[&str] = &[
    "target",
    "dispatch_strategy",
    "target_project",
    "owned_files",
    "acceptance_mode",
    "workstation_dispatch",
];

/// Cap applied to LLM proposal lists. Sonnet may attempt to fill every
/// field; the cap pins the list size at a safe upper bound and protects
/// the response payload from accidental blowup. Eight is comfortably
/// above the six allowlisted fields so a well-behaved model never trips
/// the limit.
pub(in crate::handlers::knowledge::plan) const LLM_PROPOSAL_CAP: usize = 8;

/// Token budget for the Sonnet inference call. Plans + evidence digest
/// stay well under 4 KB; we leave headroom for the model to emit one
/// proposal per field with a short justification.
const SONNET_INFER_MAX_TOKENS: u32 = 1024;

/// Caller string surfaced to LLM gateway logging. Mirrors the
/// `plan_compiler` literal used by the wave-12 / 04 plan-compiler so the
/// observability surface stays self-explanatory.
pub(in crate::handlers::knowledge::plan) const SONNET_INFER_CALLER: &str = "plan_field_inference";

/// Wire status describing the outcome of the LLM-augmented pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::handlers::knowledge::plan) enum LlmProposalStatus {
    /// Caller picked a non-LLM mode; the bundle is absent.
    NotInvoked,
    /// LLM was unavailable (gateway not initialised, gate closed, network
    /// failure, etc.). Bundle carries a reason; no proposals.
    Unavailable,
    /// LLM responded with at least one valid proposal.
    Suggested,
    /// LLM responded but no proposal survived validation (zero usable
    /// fields). Bundle may carry parse_warnings to explain why.
    NoSuggestions,
    /// LLM responded with parseable shape but every field was already
    /// covered by the deterministic engine, so we suppressed the
    /// proposal list rather than echo redundant suggestions.
    DeterministicAlreadyComplete,
}

impl LlmProposalStatus {
    pub(in crate::handlers::knowledge::plan) fn as_wire(self) -> &'static str {
        match self {
            LlmProposalStatus::NotInvoked => "not_invoked",
            LlmProposalStatus::Unavailable => "llm_unavailable",
            LlmProposalStatus::Suggested => "suggested",
            LlmProposalStatus::NoSuggestions => "no_suggestions",
            LlmProposalStatus::DeterministicAlreadyComplete => "deterministic_already_complete",
        }
    }
}

/// Conflict tag attached to every LLM proposal. Caller / deterministic
/// agreement is never silently overridden; conflicts surface for review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::handlers::knowledge::plan) enum LlmConflictStatus {
    /// Caller did not specify, deterministic engine returned no signal,
    /// LLM proposal stands alone.
    None,
    /// Caller passed an explicit value differing from the LLM proposal.
    ConflictsWithCaller,
    /// Deterministic engine inferred a different value for the same
    /// field with high confidence.
    ConflictsWithDeterministic,
    /// Deterministic engine surfaced the same field as a suggestion
    /// (medium / low confidence) with a different value. Lower-priority
    /// conflict than caller / deterministic-high.
    OverlapsDeterministicSuggestion,
}

impl LlmConflictStatus {
    pub(in crate::handlers::knowledge::plan) fn as_wire(self) -> &'static str {
        match self {
            LlmConflictStatus::None => "none",
            LlmConflictStatus::ConflictsWithCaller => "conflicts_with_caller",
            LlmConflictStatus::ConflictsWithDeterministic => "conflicts_with_deterministic",
            LlmConflictStatus::OverlapsDeterministicSuggestion => {
                "overlaps_deterministic_suggestion"
            }
        }
    }
}

/// One validated LLM proposal. The `field` is interned to a static
/// allowlist string so downstream consumers can switch on it cheaply.
#[derive(Debug, Clone)]
pub(in crate::handlers::knowledge::plan) struct LlmProposal {
    pub(in crate::handlers::knowledge::plan) field: &'static str,
    pub(in crate::handlers::knowledge::plan) value: Value,
    pub(in crate::handlers::knowledge::plan) confidence: InferenceConfidence,
    pub(in crate::handlers::knowledge::plan) evidence: String,
    pub(in crate::handlers::knowledge::plan) conflict_status: LlmConflictStatus,
}

impl LlmProposal {
    pub(in crate::handlers::knowledge::plan) fn to_json(&self) -> Value {
        json!({
            "field": self.field,
            "value": self.value.clone(),
            "confidence": self.confidence.as_wire(),
            "evidence": self.evidence,
            "conflict_status": self.conflict_status.as_wire(),
            // Pin the never-applied invariant so observers can `assert
            // proposal.applied == false` without reading the whole
            // task contract.
            "applied": false,
        })
    }
}

/// Bundle of LLM-side data attached to [`PlanFieldInference`]. Always
/// carries the status (so observers see whether the gateway was
/// reachable) plus the validated proposals. `parse_warnings[]` records
/// per-field validation drops for caller debugging without aborting the
/// response.
#[derive(Debug, Clone)]
pub(in crate::handlers::knowledge::plan) struct LlmProposalBundle {
    pub(in crate::handlers::knowledge::plan) status: LlmProposalStatus,
    pub(in crate::handlers::knowledge::plan) proposals: Vec<LlmProposal>,
    pub(in crate::handlers::knowledge::plan) parse_warnings: Vec<String>,
    pub(in crate::handlers::knowledge::plan) unavailable_reason: Option<String>,
    pub(in crate::handlers::knowledge::plan) model: Option<String>,
    pub(in crate::handlers::knowledge::plan) request_caller: Option<String>,
}

impl LlmProposalBundle {
    pub(in crate::handlers::knowledge::plan) fn unavailable(reason: impl Into<String>) -> Self {
        LlmProposalBundle {
            status: LlmProposalStatus::Unavailable,
            proposals: Vec::new(),
            parse_warnings: Vec::new(),
            unavailable_reason: Some(reason.into()),
            model: None,
            request_caller: Some(SONNET_INFER_CALLER.to_string()),
        }
    }
}

/// Validate a Sonnet response into a list of [`LlmProposal`] entries.
/// The expected shape is `{"proposals": [{...}, ...]}` (object with a
/// `proposals` array) OR a bare top-level array; both forms are
/// accepted because Sonnet sometimes elides the wrapper.
///
/// Rejected proposals land on `parse_warnings[]` so the caller can audit
/// what survived. The validator is PURE so the unit tests can pin every
/// edge case without touching the LLM.
pub(in crate::handlers::knowledge::plan) fn parse_llm_proposals(
    raw: &str,
) -> (Vec<LlmProposal>, Vec<String>) {
    let mut warnings: Vec<String> = Vec::new();
    let trimmed = raw.trim();
    let trimmed = strip_code_fence(trimmed);
    let parsed: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(err) => {
            warnings.push(format!("LLM response was not valid JSON: {}", err));
            return (Vec::new(), warnings);
        }
    };
    let raw_proposals = match &parsed {
        Value::Array(arr) => arr.clone(),
        Value::Object(map) => match map.get("proposals") {
            Some(Value::Array(arr)) => arr.clone(),
            Some(other) => {
                warnings.push(format!(
                    "`proposals` must be an array, got {}",
                    json_kind(other)
                ));
                return (Vec::new(), warnings);
            }
            None => {
                warnings.push("LLM response object missing required `proposals` array".to_string());
                return (Vec::new(), warnings);
            }
        },
        other => {
            warnings.push(format!(
                "LLM response top-level must be array or object, got {}",
                json_kind(other)
            ));
            return (Vec::new(), warnings);
        }
    };
    let mut out: Vec<LlmProposal> = Vec::new();
    let mut seen_fields: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    for (idx, raw) in raw_proposals.iter().enumerate() {
        if out.len() >= LLM_PROPOSAL_CAP {
            warnings.push(format!(
                "proposal cap of {} reached; dropping remaining entries",
                LLM_PROPOSAL_CAP
            ));
            break;
        }
        let obj = match raw.as_object() {
            Some(o) => o,
            None => {
                warnings.push(format!(
                    "proposals[{}] must be an object, got {}",
                    idx,
                    json_kind(raw)
                ));
                continue;
            }
        };
        let field_raw = obj
            .get("field")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .unwrap_or("");
        let field = match LLM_ALLOWED_FIELDS
            .iter()
            .find(|allowed| allowed.eq_ignore_ascii_case(field_raw))
            .copied()
        {
            Some(f) => f,
            None => {
                warnings.push(format!(
                    "proposals[{}] field `{}` not in allowlist",
                    idx, field_raw
                ));
                continue;
            }
        };
        if seen_fields.contains(field) {
            warnings.push(format!(
                "proposals[{}] duplicate field `{}` ignored",
                idx, field
            ));
            continue;
        }
        let value_raw = match obj.get("value") {
            Some(v) => v.clone(),
            None => {
                warnings.push(format!("proposals[{}] missing required `value`", idx));
                continue;
            }
        };
        let value = match coerce_proposal_value(field, &value_raw) {
            Ok(v) => v,
            Err(err) => {
                warnings.push(format!("proposals[{}] {}", idx, err));
                continue;
            }
        };
        let confidence = match obj.get("confidence").and_then(|v| v.as_str()) {
            Some(s) => match s.trim().to_ascii_lowercase().as_str() {
                "high" => InferenceConfidence::High,
                "medium" => InferenceConfidence::Medium,
                "low" => InferenceConfidence::Low,
                other => {
                    warnings.push(format!(
                        "proposals[{}] confidence `{}` not in [high, medium, low]",
                        idx, other
                    ));
                    continue;
                }
            },
            None => {
                warnings.push(format!("proposals[{}] missing required `confidence`", idx));
                continue;
            }
        };
        let evidence = obj
            .get("evidence")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if evidence.is_empty() {
            warnings.push(format!(
                "proposals[{}] missing required `evidence` justification",
                idx
            ));
            continue;
        }
        seen_fields.insert(field);
        out.push(LlmProposal {
            field,
            value,
            confidence,
            evidence,
            // conflict_status is reconciled against the deterministic
            // result + caller args by `reconcile_llm_conflicts` once the
            // proposal list is parsed. Default to `None` here so a
            // failing reconciliation step never accidentally promotes a
            // conflict-free state.
            conflict_status: LlmConflictStatus::None,
        });
    }
    (out, warnings)
}

/// Strip a Markdown code fence (```json ... ``` or ``` ... ```) if the
/// model wrapped its JSON output. Returns the inner trimmed content. The
/// validator runs after this so a fenced response still parses cleanly.
pub(in crate::handlers::knowledge::plan) fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    let stripped = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```JSON"))
        .or_else(|| s.strip_prefix("```"));
    let Some(rest) = stripped else {
        return s;
    };
    // Drop the opening newline (if any) and the closing fence.
    let rest = rest.trim_start_matches('\n');
    let rest = rest.strip_suffix("```").unwrap_or(rest);
    rest.trim()
}

/// Short json kind name for diagnostics (e.g. `"string"`, `"object"`).
pub(in crate::handlers::knowledge::plan) fn json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Coerce a raw LLM-emitted value into the canonical shape expected for
/// `field`. Rejects shape mismatches with a human-readable reason that
/// lands on `parse_warnings[]`. Empty / blank strings and arrays are
/// rejected because they carry no usable signal.
pub(in crate::handlers::knowledge::plan) fn coerce_proposal_value(
    field: &str,
    raw: &Value,
) -> std::result::Result<Value, String> {
    match field {
        "workstation_dispatch" => match raw {
            Value::Bool(b) => Ok(Value::Bool(*b)),
            Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => Ok(Value::Bool(true)),
                "false" | "no" | "off" | "0" => Ok(Value::Bool(false)),
                other => Err(format!(
                    "value for `workstation_dispatch` must be bool, got string `{}`",
                    other
                )),
            },
            other => Err(format!(
                "value for `workstation_dispatch` must be bool, got {}",
                json_kind(other)
            )),
        },
        "owned_files" => match raw {
            Value::Array(items) => {
                let mut out: Vec<String> = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let Some(s) = item.as_str() else {
                        return Err(format!(
                            "value.owned_files[{}] must be string, got {}",
                            i,
                            json_kind(item)
                        ));
                    };
                    let t = s.trim();
                    if !t.is_empty() {
                        out.push(t.to_string());
                    }
                }
                if out.is_empty() {
                    Err("value for `owned_files` must contain at least one path".to_string())
                } else {
                    Ok(json!(out))
                }
            }
            other => Err(format!(
                "value for `owned_files` must be string array, got {}",
                json_kind(other)
            )),
        },
        // The remaining four fields are string-shaped.
        "target" | "dispatch_strategy" | "target_project" | "acceptance_mode" => match raw {
            Value::String(s) => {
                let t = s.trim();
                if t.is_empty() {
                    Err(format!("value for `{}` must be non-empty string", field))
                } else {
                    Ok(json!(t))
                }
            }
            other => Err(format!(
                "value for `{}` must be string, got {}",
                field,
                json_kind(other)
            )),
        },
        _ => Err(format!(
            "field `{}` is not supported by LLM proposals",
            field
        )),
    }
}

/// Reconcile parsed LLM proposals against the deterministic inference
/// result and the caller-supplied args. Tags every proposal with a
/// [`LlmConflictStatus`] so observers can pivot on it without recomputing.
///
/// Conflict precedence (highest first):
///   1. Caller-supplied differing value          → ConflictsWithCaller.
///   2. Deterministic high-confidence value      → ConflictsWithDeterministic.
///   3. Deterministic suggestion (medium / low)  → OverlapsDeterministicSuggestion.
///   4. Otherwise                                → None.
///
/// This function NEVER mutates state; it only annotates the proposal
/// bundle so the caller can decide whether to act.
pub(in crate::handlers::knowledge::plan) fn reconcile_llm_conflicts(
    proposals: &mut [LlmProposal],
    deterministic: &PlanFieldInference,
    args: &Value,
) {
    for p in proposals.iter_mut() {
        // 1. Caller-supplied values always win for conflict tagging.
        if let Some(caller_val) = caller_value_for_field(args, p.field) {
            if !values_equivalent(p.field, &caller_val, &p.value) {
                p.conflict_status = LlmConflictStatus::ConflictsWithCaller;
                continue;
            }
            // Caller agrees with LLM proposal — leave conflict_status
            // at None; the proposal still carries useful evidence.
        }
        // 2. Deterministic high-confidence inference for the same field.
        if let Some(det) = deterministic.inferred.iter().find(|f| f.field == p.field) {
            if !values_equivalent(p.field, &det.value, &p.value) {
                p.conflict_status = LlmConflictStatus::ConflictsWithDeterministic;
                continue;
            }
        }
        // 3. Deterministic suggestion (medium / low) overlap.
        if let Some(sug) = deterministic.suggested.iter().find(|f| f.field == p.field) {
            if !values_equivalent(p.field, &sug.value, &p.value) {
                p.conflict_status = LlmConflictStatus::OverlapsDeterministicSuggestion;
                continue;
            }
        }
    }
}

/// Caller value for a field in caller args, normalised to a `Value` so it
/// can be compared with LLM proposals. Strings are trimmed; empty
/// strings collapse to `None`. Arrays of strings (for `owned_files`)
/// flow through unchanged.
pub(in crate::handlers::knowledge::plan) fn caller_value_for_field(
    args: &Value,
    field: &str,
) -> Option<Value> {
    match field {
        "workstation_dispatch" => caller_bool(args, field).map(Value::Bool),
        "owned_files" => {
            let v = caller_string_list(args, field);
            if v.is_empty() {
                None
            } else {
                Some(json!(v))
            }
        }
        _ => caller_str(args, field).map(|s| json!(s)),
    }
}

/// Field-aware equality check. Strings compare ascii-case-insensitively
/// (matching the deterministic engine); arrays compare order-independent
/// for `owned_files`; bools / others compare with `==`.
pub(in crate::handlers::knowledge::plan) fn values_equivalent(
    field: &str,
    a: &Value,
    b: &Value,
) -> bool {
    match field {
        "owned_files" => {
            let a_list: Vec<String> = a
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let b_list: Vec<String> = b
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let mut a_sorted = a_list.clone();
            let mut b_sorted = b_list.clone();
            a_sorted.sort();
            b_sorted.sort();
            a_sorted == b_sorted
        }
        "workstation_dispatch" => a.as_bool() == b.as_bool(),
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
            _ => a == b,
        },
    }
}

/// Compose the system + user prompts for the Sonnet inference call. The
/// system prompt pins the strict JSON schema, the user prompt embeds the
/// PLAN sexp + compiled_from + evidence digest. Pure function so the
/// unit tests can lock the prompt shape.
pub(in crate::handlers::knowledge::plan) fn build_llm_inference_prompt(
    plan_sexp: &str,
    compiled_from: Option<&str>,
    evidence_entries: &[Value],
    deterministic: &PlanFieldInference,
    caller_args: &Value,
) -> (String, String) {
    let system = String::from(
        "You are MissionD's plan field inference assistant. Inspect the supplied PLAN.lisp \
         sexp, the directive provenance string, and a small evidence digest, then propose \
         values for any of the following six PLAN fields that are NOT already covered by the \
         deterministic engine: target, dispatch_strategy, target_project, owned_files, \
         acceptance_mode, workstation_dispatch.\n\n\
         Reply with STRICT JSON ONLY (no Markdown fences, no prose) matching this shape:\n\
         {\n  \"proposals\": [\n    {\n      \"field\": \"<one of the six fields>\",\n      \"value\": <string|bool|string array depending on field>,\n      \"confidence\": \"high\"|\"medium\"|\"low\",\n      \"evidence\": \"<one short sentence justifying the proposal>\"\n    }\n  ]\n}\n\n\
         Rules:\n\
         - Never propose a value that already appears in the deterministic block. The caller will \
           tag your proposal with `conflict_status` if it disagrees with caller args or the \
           deterministic engine; do not pre-flag conflicts yourself.\n\
         - Omit a field rather than fabricate one. An empty proposals array is a valid response.\n\
         - `target` must be one of: mission_execution | mission_task_delegate | mission_flow_run.\n\
         - `dispatch_strategy` must be one of: resident-lisp | fresh-code-alignment | agent-team | mixed | prompt-fallback.\n\
         - `acceptance_mode` must be one of: inner_status | evidence_keys | manual.\n\
         - `owned_files` must be a non-empty array of repo-relative paths.\n\
         - `workstation_dispatch` must be a boolean.\n\
         - Confidence `high` is reserved for unambiguous evidence. When in doubt, use `medium` or omit.\n\
         - Never include keys outside the listed schema.",
    );
    let evidence_digest: Vec<Value> = evidence_entries.iter().rev().take(8).cloned().collect();
    let deterministic_block = deterministic.to_response_json(InferPlanFieldsMode::Preview);
    let user = format!(
        "PLAN.lisp sexp:\n```lisp\n{plan}\n```\n\ncompiled_from: {compiled}\n\nrecent evidence (newest first, capped):\n```json\n{evidence}\n```\n\ndeterministic inference already produced:\n```json\n{deterministic}\n```\n\ncaller-supplied args (already-set fields you must NOT override):\n```json\n{caller}\n```",
        plan = plan_sexp,
        compiled = compiled_from.unwrap_or("(none)"),
        evidence = serde_json::to_string_pretty(&evidence_digest).unwrap_or_else(|_| "[]".into()),
        deterministic =
            serde_json::to_string_pretty(&deterministic_block).unwrap_or_else(|_| "{}".into()),
        caller = serde_json::to_string_pretty(caller_args).unwrap_or_else(|_| "{}".into()),
    );
    (system, user)
}

/// Run the Sonnet inference call. Returns a [`LlmProposalBundle`] in
/// every code path so the caller can pivot on the bundle status without
/// branching on Result. Sonnet unavailability surfaces as
/// `LlmProposalStatus::Unavailable` with an explanatory reason — never
/// as a silent fallback to deterministic-only.
pub(in crate::handlers::knowledge::plan) async fn request_llm_proposals(
    state: &AppState,
    plan_sexp: &str,
    compiled_from: Option<&str>,
    evidence_entries: &[Value],
    deterministic: &PlanFieldInference,
    caller_args: &Value,
) -> LlmProposalBundle {
    let Some(sonnet) = state.sonnet.as_ref() else {
        return LlmProposalBundle::unavailable(
            "Sonnet gateway not initialized; LLM-augmented PLAN inference unavailable",
        );
    };
    let (system, user) = build_llm_inference_prompt(
        plan_sexp,
        compiled_from,
        evidence_entries,
        deterministic,
        caller_args,
    );
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user,
        },
    ];
    let raw = match sonnet
        .call_interactive(messages, Some(SONNET_INFER_MAX_TOKENS), SONNET_INFER_CALLER)
        .await
    {
        Ok(s) => s,
        Err(err) => {
            return LlmProposalBundle::unavailable(format!(
                "Sonnet inference call failed: {}",
                err
            ));
        }
    };
    let (mut proposals, parse_warnings) = parse_llm_proposals(&raw);
    reconcile_llm_conflicts(&mut proposals, deterministic, caller_args);
    let compiler_model = match load_sonnet_compiler_model() {
        Ok(model) => model,
        Err(err) => return LlmProposalBundle::unavailable(err.to_string()),
    };
    let status = if proposals.is_empty() {
        // Distinguish between "deterministic already covered every field"
        // and "model returned no usable suggestions". The first is a
        // success signal (no work to do); the second is debug-worthy.
        if deterministic_covers_all_fields(deterministic) {
            LlmProposalStatus::DeterministicAlreadyComplete
        } else {
            LlmProposalStatus::NoSuggestions
        }
    } else {
        LlmProposalStatus::Suggested
    };
    LlmProposalBundle {
        status,
        proposals,
        parse_warnings,
        unavailable_reason: None,
        model: Some(compiler_model),
        request_caller: Some(SONNET_INFER_CALLER.to_string()),
    }
}

/// True when the deterministic engine produced a high-confidence
/// inference for every allowlisted field. Used to label the LLM bundle
/// status so observers can tell whether an empty `llm_proposals[]` means
/// "model declined" vs "nothing left to suggest".
pub(in crate::handlers::knowledge::plan) fn deterministic_covers_all_fields(
    deterministic: &PlanFieldInference,
) -> bool {
    for field in LLM_ALLOWED_FIELDS {
        let covered = deterministic
            .inferred
            .iter()
            .any(|f| f.field == *field && f.confidence.meets_apply_threshold());
        if !covered {
            return false;
        }
    }
    true
}

/// Read up to the most-recent N evidence sidecar entries for a plan.
/// Returns an empty vec when the sidecar does not exist or cannot be
/// resolved — this is a best-effort hint source, NEVER load-bearing for
/// dispatch. Read-only; never writes.
pub(in crate::handlers::knowledge::plan) async fn read_recent_evidence_entries(
    state: &AppState,
    plan_id: uuid::Uuid,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
    cap: usize,
) -> Vec<Value> {
    let project_root = match resolve_project_root(
        &state.project_registry,
        project_arg,
        cwd_arg,
        target_project_arg,
    )
    .await
    {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let path = existing_plan_evidence_sidecar_path(&project_root, plan_id);
    if !path.exists() {
        return Vec::new();
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let bundle: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let entries = bundle
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if entries.len() <= cap {
        entries
    } else {
        entries[entries.len() - cap..].to_vec()
    }
}
