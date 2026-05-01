use super::super::{evidence_collector, plan_dag, workflow};
use super::*;

// ───────────────────────────────────────────────────────────────────────
// wave-18 / task 05 — cross-plan distill chain v0
//
// Conservative chain orchestrator that runs AFTER the wave-17 / task 05
// `finalize_plan` + `distill_on_success` pass. The chain knobs let a
// caller mark this plan as a contributor to a named workflow distillation
// chain that spans multiple successful plans. We never overwrite prior
// chain entries (they live in OTHER plans' sidecars; this plan's own
// sidecar is purely additive); we never invoke workflow distill outside
// the explicit `dry_run` / `sonnet` modes; and we never downgrade the
// underlying plan finalization on chain failure (the finalization block
// from `plan_dag::action_execute_dag_v1` is preserved verbatim — chain
// failures only surface a non-fatal `warning` on the chain block).
//
// Lisp authority forward-reference (wave-18 / task 10 will backfill):
//   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//                          s8 workflow-distillation (chain extension)
//   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//                                  role workflow-distiller (chain mode)
// ───────────────────────────────────────────────────────────────────────

/// Canonical chain mode strings. Mirror these in the MCP descriptor's
/// enum so the two surfaces cannot drift on a typo.
pub(super) const DISTILL_CHAIN_MODE_RECORD_ONLY: &str = "record_only";
pub(super) const DISTILL_CHAIN_MODE_DRY_RUN: &str = "dry_run";
pub(super) const DISTILL_CHAIN_MODE_SONNET: &str = "sonnet";

/// Evidence `kind` tag for the chain-record sidecar entry. Distinct from
/// the wave-17 / task 05 `dag_finalized` row so audit dashboards can
/// pivot on chain participation without re-deriving it from the
/// surrounding state_transition.
pub(super) const CHAIN_RECORD_KIND: &str = "distill_chain_record";

/// Status strings surfaced on the `distill_chain_status` response field.
/// Kept as constants so callers can pin the wire form in tests / audit
/// queries without scraping a string literal.
pub(super) const CHAIN_STATUS_RECORDED: &str = "recorded";
pub(super) const CHAIN_STATUS_RECORDED_WITH_DISTILL: &str = "recorded_with_distill";
pub(super) const CHAIN_STATUS_RECORDED_DISTILL_WARNING: &str = "recorded_with_distill_warning";
pub(super) const CHAIN_STATUS_SKIPPED_PLAN_NOT_SUCCEEDED: &str = "skipped_plan_not_succeeded";
pub(super) const CHAIN_STATUS_SKIPPED_NO_FINALIZATION: &str = "skipped_no_finalization";
pub(super) const CHAIN_STATUS_NOT_REQUESTED: &str = "not_requested";
pub(super) const CHAIN_STATUS_RECORD_FAILED: &str = "record_failed";

/// Parse the optional `distill_chain_id` arg. Returns `None` when absent
/// or blank — the chain orchestrator generates a deterministic fallback
/// (`chain:auto:plan-<plan_id>`) in that case so the audit row never
/// carries an empty id.
pub(super) fn parse_distill_chain_id(args: &Value) -> Option<String> {
    args.get("distill_chain_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Parse the optional `distill_chain_name` arg (free-form human-readable
/// label, e.g. "wave18-finalize-loop"). Blank collapses to `None`.
pub(super) fn parse_distill_chain_name(args: &Value) -> Option<String> {
    args.get("distill_chain_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Strict allowlist for the `distill_chain_mode` knob. Default is
/// `record_only` (no workflow distill call — the chain entry is only
/// recorded in the evidence sidecar). `dry_run` and `sonnet` forward to
/// `mission_workflow(action=distill, distill_mode=…)` with the
/// corresponding mode. Returns the canonical string or an error message.
pub(super) fn parse_distill_chain_mode(args: &Value) -> std::result::Result<&'static str, String> {
    match args.get("distill_chain_mode").and_then(|v| v.as_str()) {
        None | Some("") | Some(DISTILL_CHAIN_MODE_RECORD_ONLY) => {
            Ok(DISTILL_CHAIN_MODE_RECORD_ONLY)
        }
        Some(DISTILL_CHAIN_MODE_DRY_RUN) => Ok(DISTILL_CHAIN_MODE_DRY_RUN),
        Some(DISTILL_CHAIN_MODE_SONNET) => Ok(DISTILL_CHAIN_MODE_SONNET),
        Some(other) => Err(format!(
            "distill_chain_mode must be one of [\"record_only\", \"dry_run\", \"sonnet\"]; got `{}`",
            other
        )),
    }
}

/// Returns true when ANY of the chain knobs were supplied. Used to gate
/// validation + the post-finalize chain run — callers that did not opt in
/// see byte-identical wave-17 / task 05 responses.
pub(super) fn distill_chain_requested(args: &Value) -> bool {
    args.get("distill_chain_id").is_some()
        || args.get("distill_chain_mode").is_some()
        || args.get("distill_chain_name").is_some()
}

/// Pre-flight validation for the wave-18 / task 05 chain knobs. Returns
/// `Some(error_result)` for the call site to early-return; `None` when
/// the args pass.
///
/// Cross-field rules:
///
///   * Any chain knob requires `finalize_plan=true` — the chain is gated
///     on a successful finalization, so silently dropping a chain
///     request would mask the caller's intent.
///   * `distill_chain_mode` must be on the strict allowlist — even when
///     no other chain knob was passed, a typo on the mode alone surfaces
///     immediately rather than on the next live caller's run.
pub(super) fn validate_distill_chain_args(args: &Value) -> Option<ToolResult> {
    if let Err(msg) = parse_distill_chain_mode(args) {
        return Some(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            msg,
        )));
    }
    if distill_chain_requested(args) && !plan_dag::parse_finalize_plan(args) {
        return Some(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                "distill_chain_* knobs require finalize_plan=true",
            )
            .with_suggestion(
                "the cross-plan distill chain only fires AFTER a successful finalization; \
                 set finalize_plan=true or drop the distill_chain_* knobs",
            ),
        ));
    }
    // wave-21 / task 07 — strict-shape validation of the auto-sonnet
    // apply-gate knobs. Workflow.rs validates again as a defense-in-depth
    // layer, but failing fast at the plan entry keeps the diagnostic
    // close to the caller's invocation site.
    if let Some(v) = args.get("auto_sonnet") {
        if !v.is_boolean() {
            return Some(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "auto_sonnet must be a boolean (true|false); got {}",
                        json_shape_label(v)
                    ),
                )
                .with_suggestion(
                    "auto_sonnet is the wave-21 / task 07 apply-gate opt-in; \
                     pass true or false (no string).",
                ),
            ));
        }
    }
    if let Some(v) = args.get("auto_sonnet_approved") {
        if !v.is_boolean() {
            return Some(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "auto_sonnet_approved must be a boolean (true|false); got {}",
                        json_shape_label(v)
                    ),
                )
                .with_suggestion(
                    "auto_sonnet_approved is the wave-21 / task 07 caller-approval flag; \
                     pass true or false (no string).",
                ),
            ));
        }
    }
    // wave-22 / task 06 — closed-enum strict-shape validation of the
    // policy v2 knob. Workflow.rs validates again as a defense-in-depth
    // layer, but failing fast at the plan entry keeps the diagnostic
    // close to the caller's invocation site (mirrors wave-21/07 dual
    // opt-in validation).
    if let Some(v) = args.get("auto_sonnet_policy") {
        if !v.is_null() {
            let s = match v.as_str() {
                Some(s) => s,
                None => {
                    return Some(ToolResult::structured_error(
                        ToolError::new(
                            error_codes::INVALID_PARAM,
                            format!(
                                "auto_sonnet_policy must be a string (one of [\"off\",\"safe_after_rules\",\"dry_run\"]); got {}",
                                json_shape_label(v)
                            ),
                        )
                        .with_suggestion(
                            "auto_sonnet_policy is the wave-22 / task 06 v2 closed-enum policy; \
                             pass one of [\"off\",\"safe_after_rules\",\"dry_run\"] (no boolean / number).",
                        ),
                    ));
                }
            };
            if !matches!(s, "" | "off" | "safe_after_rules" | "dry_run") {
                return Some(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "auto_sonnet_policy must be one of [\"off\",\"safe_after_rules\",\"dry_run\"]; got `{}`",
                            s
                        ),
                    )
                    .with_suggestion(
                        "auto_sonnet_policy is the wave-22 / task 06 v2 closed-enum policy; \
                         pass one of [\"off\",\"safe_after_rules\",\"dry_run\"].",
                    ),
                ));
            }
        }
    }
    None
}

/// Render the JSON shape of a value as a stable label for diagnostic
/// messages. Mirrors `workflow::shape_label` so the two surfaces emit
/// identical wording on shape rejections.
pub(super) fn json_shape_label(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Deterministic fallback chain id when the caller did not supply one.
/// Anchored on the plan id so re-runs against the same plan land on the
/// same chain bucket — auditors can correlate without rolling a UUID.
pub(super) fn derive_fallback_chain_id(plan_id: uuid::Uuid) -> String {
    format!("chain:auto:plan-{}", plan_id)
}

/// Inspect the wave-17 / task 05 finalization block on the inner DAG
/// payload to decide chain eligibility. Returns `Some("…")` reason when
/// chain MUST be skipped (with the canonical `distill_chain_status`
/// label), or `None` when the chain can proceed.
pub(super) fn chain_eligibility_skip_reason(payload: &Value) -> Option<&'static str> {
    let finalization = match payload.get("finalization") {
        Some(v) => v,
        None => return Some(CHAIN_STATUS_SKIPPED_NO_FINALIZATION),
    };
    let final_plan_status = finalization
        .get("final_plan_status")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if final_plan_status != "succeeded" {
        return Some(CHAIN_STATUS_SKIPPED_PLAN_NOT_SUCCEEDED);
    }
    None
}

/// Build the chain block surfaced under `finalization.distill_chain` on
/// the response. Always carries `triggered` / `status` / `chain_id` /
/// `chain_mode` so observers can pivot on a single shape; optional
/// `chain_name` / `distill_result` / `warning` / `chain_index_in_plan` /
/// `evidence_path` / `evidence_error` are only added when present.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_distill_chain_block(
    triggered: bool,
    status: &str,
    chain_id: &str,
    chain_id_source: &str,
    chain_mode: &str,
    chain_name: Option<&str>,
    chain_index_in_plan: Option<usize>,
    distill_result: Option<Value>,
    warning: Option<&str>,
    evidence_path: Option<&str>,
    evidence_error: Option<&str>,
) -> Value {
    let mut block = json!({
        "triggered": triggered,
        "status": status,
        "chain_id": chain_id,
        "chain_id_source": chain_id_source,
        "chain_mode": chain_mode,
    });
    if let Some(n) = chain_name {
        block["chain_name"] = json!(n);
    }
    if let Some(idx) = chain_index_in_plan {
        block["chain_index_in_plan"] = json!(idx);
    }
    if let Some(r) = distill_result {
        block["distill_result"] = r;
    }
    if let Some(w) = warning {
        block["warning"] = json!(w);
    }
    if let Some(p) = evidence_path {
        block["evidence_path"] = json!(p);
    }
    if let Some(e) = evidence_error {
        block["evidence_error"] = json!(e);
    }
    block
}

/// Read the existing evidence sidecar (if any) and count the prior
/// chain-record entries with the matching `chain_id`. Returns 0 when
/// the sidecar does not exist or carries no prior chain rows for this
/// id. Pure read — never writes.
///
/// Failures (resolve / read / parse) collapse to 0 because the chain
/// orchestrator's "do not overwrite prior evidence" invariant is
/// satisfied by the writer (`append_plan_evidence_entry` only appends);
/// the count is purely a UX hint surfaced as `chain_index_in_plan`.
async fn count_prior_chain_entries_in_plan_sidecar(
    state: &AppState,
    plan_id: uuid::Uuid,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
    chain_id: &str,
) -> usize {
    let project_root = match resolve_project_root(
        &state.project_registry,
        project_arg,
        cwd_arg,
        target_project_arg,
    )
    .await
    {
        Ok(p) => p,
        Err(_) => return 0,
    };
    let path = existing_plan_evidence_sidecar_path(&project_root, plan_id);
    if !path.exists() {
        return 0;
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let bundle: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let entries = match bundle.get("entries").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return 0,
    };
    entries
        .iter()
        .filter(|e| {
            e.get("kind").and_then(|v| v.as_str()) == Some(CHAIN_RECORD_KIND)
                && e.get("chain_id").and_then(|v| v.as_str()) == Some(chain_id)
        })
        .count()
}

/// Drive the optional cross-plan distill chain. Pure orchestration:
/// validation already ran in `validate_distill_chain_args` so here we
/// only branch on the runtime payload + chain mode.
///
/// Returns the same `dag_result` byte-for-byte when no chain knob was
/// supplied. Otherwise injects a `distill_chain` block under the
/// existing `finalization` map (or under a new top-level
/// `distill_chain` key when finalization was not requested — in that
/// case the chain is also skipped, but we still surface the skip
/// reason so callers can detect the missed opt-in).
pub(super) async fn apply_distill_chain(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    dag_result: ToolResult,
) -> ToolResult {
    if !distill_chain_requested(args) {
        return dag_result;
    }
    // Inner DAG result may itself be a structured error (e.g. validation
    // rejected on the wave-17 path). Surface chain="not_requested" on
    // the same envelope so the caller still sees a stable shape, but do
    // NOT overwrite the error payload.
    if dag_result.is_error.unwrap_or(false) {
        return dag_result;
    }

    // Mode is already validated; unwrap is safe.
    let chain_mode = parse_distill_chain_mode(args).unwrap_or(DISTILL_CHAIN_MODE_RECORD_ONLY);
    let chain_name = parse_distill_chain_name(args);
    let (chain_id, chain_id_source): (String, &'static str) = match parse_distill_chain_id(args) {
        Some(id) => (id, "explicit_arg"),
        None => (derive_fallback_chain_id(plan.id), "derived_from_plan_id"),
    };

    // Re-parse the inner payload so we can inspect the wave-17 / task 05
    // `finalization` block and (when chain runs) augment it with our
    // `distill_chain` sub-block.
    let mut payload = tool_result_payload(&dag_result);

    // Eligibility gate: chain only fires when the inner finalization
    // block reports `final_plan_status="succeeded"`. Any other state
    // (failed / paused / unchanged / no finalization) collapses to a
    // skipped chain block — recorded on the response so the caller can
    // see the skip reason but with NO sidecar write and NO distill call.
    if let Some(skip_reason) = chain_eligibility_skip_reason(&payload) {
        let block = build_distill_chain_block(
            false,
            skip_reason,
            &chain_id,
            chain_id_source,
            chain_mode,
            chain_name.as_deref(),
            None,
            None,
            None,
            None,
            None,
        );
        attach_distill_chain_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Eligibility passed → run the chain. Order:
    //   1. Count prior chain entries in this plan's sidecar (UX hint).
    //   2. Optionally invoke `mission_workflow(action=distill)` for
    //      `dry_run` / `sonnet` modes.
    //   3. Append exactly ONE chain-record evidence row tagged with
    //      chain_id / chain_name / chain_mode / distill summary.
    //   4. Return the augmented response.
    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());

    let prior_count = count_prior_chain_entries_in_plan_sidecar(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        &chain_id,
    )
    .await;
    let chain_index_in_plan = prior_count + 1;

    // Step 2 — optional workflow distill call. `record_only` skips this
    // entirely. The brief explicitly forbids invoking sonnet without an
    // explicit mode, so we route on the canonical mode string.
    let (distill_result, distill_warning, triggered_distill): (
        Option<Value>,
        Option<String>,
        bool,
    ) = match chain_mode {
        DISTILL_CHAIN_MODE_RECORD_ONLY => (None, None, false),
        DISTILL_CHAIN_MODE_DRY_RUN | DISTILL_CHAIN_MODE_SONNET => {
            let mut distill_args = serde_json::Map::new();
            distill_args.insert("action".to_string(), json!("distill"));
            distill_args.insert("plan_id".to_string(), json!(plan.id.to_string()));
            distill_args.insert("distill_mode".to_string(), json!(chain_mode));
            if let Some(p) = project_arg {
                distill_args.insert("project".to_string(), json!(p));
            }
            if let Some(c) = cwd_arg {
                distill_args.insert("cwd".to_string(), json!(c));
            }
            if let Some(tp) = target_project_arg {
                distill_args.insert("target_project".to_string(), json!(tp));
            }
            if let Some(name) = chain_name.as_deref() {
                // Forward the chain name as the workflow `name` so a
                // persisted distill row carries the chain label.
                // Caller can still override by passing an explicit
                // `name` arg (we do NOT overwrite an existing key).
                distill_args
                    .entry("name".to_string())
                    .or_insert_with(|| json!(name));
            }
            // wave-21 / task 07 — forward the auto-sonnet apply-gate
            // knobs into the workflow.distill sub-call so plan-side
            // callers can opt into the gate without re-shaping the
            // arg envelope. The gate is strictly opt-in (default
            // off); the workflow surface validates shape +
            // enforces all six wave-20 safety rules + caller
            // approval before invoking Sonnet. We forward both
            // `auto_sonnet*` knobs AND `auto_chain_trigger` /
            // `auto_trigger_min_evidence` because the auto-sonnet
            // gate is layered on top of the wave-20 trigger and
            // refuses to operate without it (`skipped_no_trigger`).
            //
            // wave-22 / task 06 — forward the v2 closed-enum
            // `auto_sonnet_policy` knob alongside the v1 dual
            // opt-in flags so plan-side callers can opt into
            // either surface (or both — the workflow layer
            // attaches an `auto_sonnet_policy` block in addition
            // to the legacy `auto_sonnet` block when both are
            // requested).
            for key in [
                "auto_sonnet",
                "auto_sonnet_approved",
                "auto_sonnet_policy",
                "auto_chain_trigger",
                "auto_trigger_min_evidence",
            ] {
                if let Some(v) = args.get(key).cloned() {
                    distill_args.insert(key.to_string(), v);
                }
            }
            let call_args = Value::Object(distill_args);
            match workflow::handle(state, "mission_workflow", call_args).await {
                Ok(tr) => {
                    let inner_payload = tool_result_payload(&tr);
                    let inner_is_error = tr.is_error.unwrap_or(false);
                    let warning = if inner_is_error {
                        Some(
                            "distill chain workflow call returned an error; \
                                 plan finalization preserved"
                                .to_string(),
                        )
                    } else {
                        None
                    };
                    (Some(inner_payload), warning, true)
                }
                Err(e) => {
                    // Handler-level Result::Err → treat as a warning,
                    // never as a finalization downgrade. Mirrors
                    // `plan_dag::maybe_run_distill_trigger`'s policy.
                    tracing::warn!(
                        plan_id = %plan.id,
                        chain_id = %chain_id,
                        error = %e,
                        "distill_chain: workflow handler returned error"
                    );
                    (
                        Some(json!({"error": e.to_string()})),
                        Some(format!(
                            "distill chain workflow handler error: {}; \
                                 plan finalization preserved",
                            e
                        )),
                        true,
                    )
                }
            }
        }
        // Defensive: validator already rejected anything else.
        _ => (
            None,
            Some(format!(
                "distill_chain_mode `{}` reached chain runner unexpectedly",
                chain_mode
            )),
            false,
        ),
    };

    // Step 3 — append the chain-record evidence row. Built via the
    // typed evidence_collector so it carries the canonical
    // schema_version / source / kind stamps the wave-17 finalize entry
    // also uses. The append is purely additive (the underlying writer
    // never overwrites) so prior chain entries (in this OR other plans'
    // sidecars) are preserved by construction.
    let mut entry = evidence_collector::EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        CHAIN_RECORD_KIND,
    )
    .with_state_transition("distill_chain_appended")
    .with_extra("event_kind", json!("plan_dag_distill_chain"))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("plan_version", json!(plan.version))
    .with_extra("chain_id", json!(chain_id))
    .with_extra("chain_id_source", json!(chain_id_source))
    .with_extra("chain_mode", json!(chain_mode))
    .with_extra("chain_index_in_plan", json!(chain_index_in_plan))
    .with_extra("triggered_workflow_distill", json!(triggered_distill));
    if let Some(name) = chain_name.as_deref() {
        entry = entry.with_extra("chain_name", json!(name));
    }
    if let Some(ref result) = distill_result {
        entry = entry.with_extra("distill_result", result.clone());
    }
    if let Some(ref w) = distill_warning {
        entry = entry.with_extra("distill_warning", json!(w));
    }
    let append_outcome = evidence_collector::append(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        entry,
    )
    .await;
    let (evidence_path, evidence_error) = match append_outcome {
        evidence_collector::AppendOutcome::Written { path, .. } => {
            (Some(path.display().to_string()), None)
        }
        evidence_collector::AppendOutcome::Failed { error } => {
            tracing::warn!(
                plan_id = %plan.id,
                chain_id = %chain_id,
                error = %error,
                "distill_chain: evidence sidecar append failed"
            );
            (None, Some(error))
        }
    };

    // Step 4 — derive final status. Order of precedence:
    //   * sidecar write failed       → `record_failed` (still keep plan
    //                                    finalization durable; chain
    //                                    just couldn't persist)
    //   * triggered workflow distill that warned → `recorded_with_distill_warning`
    //   * triggered workflow distill ok          → `recorded_with_distill`
    //   * record-only                            → `recorded`
    let status = if evidence_error.is_some() {
        CHAIN_STATUS_RECORD_FAILED
    } else if triggered_distill {
        if distill_warning.is_some() {
            CHAIN_STATUS_RECORDED_DISTILL_WARNING
        } else {
            CHAIN_STATUS_RECORDED_WITH_DISTILL
        }
    } else {
        CHAIN_STATUS_RECORDED
    };

    let block = build_distill_chain_block(
        triggered_distill || evidence_error.is_none(),
        status,
        &chain_id,
        chain_id_source,
        chain_mode,
        chain_name.as_deref(),
        Some(chain_index_in_plan),
        distill_result,
        distill_warning.as_deref(),
        evidence_path.as_deref(),
        evidence_error.as_deref(),
    );
    attach_distill_chain_to_payload(&mut payload, block);
    ToolResult::json_pretty(&payload)
}

/// Insert the `distill_chain` block under `finalization.distill_chain`
/// when the wave-17 finalization block exists; otherwise surface it at
/// the top level under `distill_chain`. Either way the response also
/// carries top-level `distill_chain_status` / `distill_chain_id`
/// shortcuts so callers can grep one place.
pub(super) fn attach_distill_chain_to_payload(payload: &mut Value, block: Value) {
    let status = block
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let chain_id = block
        .get("chain_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if let Some(obj) = payload.as_object_mut() {
        if let Some(finalization) = obj.get_mut("finalization") {
            if let Some(fobj) = finalization.as_object_mut() {
                fobj.insert("distill_chain".to_string(), block.clone());
            }
        } else {
            obj.insert("distill_chain".to_string(), block.clone());
        }
        // Top-level shortcuts so the caller can pivot without diving
        // into the finalization block. `distill_chain_status` /
        // `distill_chain_id` mirror what the brief lists under "response".
        obj.insert("distill_chain_status".to_string(), json!(status));
        obj.insert("distill_chain_id".to_string(), json!(chain_id));
        // `distill_result` shortcut (the brief calls out `distill_result
        // or warning` on the response). We only surface on success +
        // dry_run/sonnet — record_only has nothing to show.
        if let Some(result) = block.get("distill_result") {
            obj.insert("distill_result".to_string(), result.clone());
        }
        if let Some(warning) = block.get("warning") {
            obj.insert("distill_chain_warning".to_string(), warning.clone());
        }
    }
}
