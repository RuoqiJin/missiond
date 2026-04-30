use super::*;

// ───────────────────────────────────────────────────────────────────────
// execute — plan-runner v0
//
// execute_mode=bridge (default): return next_call descriptor, do NOT dispatch.
// execute_mode=internal: dispatch the chosen target handler inside MissionD,
//                        append plan_runner_dispatch evidence, mark plan
//                        executing on success.
//
// Lisp authority for the internal path:
//   intent-intent-layer.lisp :: section unified-entry-pipeline :: role plan-runner
//   intent-tools.lisp        :: implemented-surface mission_plan :: :execute-contract
//   intent-flow.lisp         :: F-intent-alignment-plan-execution-loop :: s6 execution-runner
//
// TODO(plan-runner): mission_execution companion-log persistence of
// dispatch_strategy is still future per
// `intent-tools.lisp :: workstation-dispatch-record`. We surface it in this
// tool's response and the evidence sidecar so the audit trail is complete
// even before the schema-side field exists.
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_execute(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;

    let execute_mode = args
        .get("execute_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("bridge");
    if !matches!(execute_mode, "bridge" | "internal") {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("execute_mode `{}` not supported", execute_mode),
            )
            .with_suggestion("execute_mode ∈ {bridge, internal}; default is bridge"),
        ));
    }

    // wave-24 / task 04 — pre-flight `router_policy_mode` validation.
    // Default `off` ⇒ byte-compatible with wave-15..23 (no recommendation
    // block emitted). `dry_run` ⇒ compute an advisory router recommendation
    // block AFTER the dispatch path resolves; the recommendation NEVER
    // alters target / dispatch_strategy / workstation_dispatch /
    // auto_spawn / evidence — `applied` is hard-coded `false`. Any other
    // value (including `apply` / `auto`) returns INVALID_PARAM here, BEFORE
    // any plan lookup, so a typo cannot silently route a recommendation
    // through a runtime path that doesn't exist.
    let router_policy_mode = match router_policy_dry_run::parse_router_policy_mode(args) {
        Ok(m) => m,
        Err(err) => return Ok(err),
    };

    // wave-28 / task 04 — pre-flight `task_runner_mode` validation. Same
    // posture as `router_policy_mode`: default `off` ⇒ byte-compatible with
    // wave-15..27 (no task_runner block emitted, no file I/O even when
    // `task_runner_manifest_path` is supplied). `dry_run` ⇒ compute an
    // advisory task-runner manifest projection AFTER dispatch resolves;
    // dispatch is NEVER altered, `applied` is hard-coded `false`. Any
    // other value (including `apply` / `auto` / unknown strings / non-
    // string types) returns INVALID_PARAM here, BEFORE any plan lookup,
    // so a typo cannot silently route a recommendation through an
    // unimplemented surface.
    let task_runner_mode = match task_runner_dry_run::parse_task_runner_mode(args) {
        Ok(m) => m,
        Err(err) => return Ok(err),
    };

    // wave-18 / task 06 — pre-flight `infer_plan_fields` validation. Runs
    // BEFORE the plan lookup so a typo (`infer_plan_fields="aply"`) fails
    // fast instead of after a DB read.
    let infer_mode = match parse_infer_plan_fields_mode(args) {
        Ok(m) => m,
        Err(msg) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                msg,
            )))
        }
    };

    // wave-21 / task 05 — pre-flight `apply_inferred_fields` /
    // `persist_inference` / `llm_caller_approved` shape validation. Runs
    // BEFORE the plan lookup so a typo (`apply_inferred_fields="ture"`)
    // fails fast instead of silently being ignored. Conservative: only
    // bool / object / array shapes are accepted; string `"true"` is
    // rejected so the gate never opens by accident.
    if let Err(msg) = validate_apply_gate_args(args) {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            msg,
        )));
    }

    // wave-21 / task 04 — pre-flight `workstation_inference_mode`
    // validation. Strictly orthogonal to `infer_plan_fields`: the wave-21
    // surface targets the four workstation knobs (target /
    // dispatch_strategy / objective / scope) and ONLY fires when caller /
    // PLAN supplied no signal. A typo (`workstation_inference_mode="sonet"`)
    // fails fast here rather than after the DB read.
    let workstation_infer_mode = match parse_workstation_inference_mode(args) {
        Ok(m) => m,
        Err(msg) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                msg,
            )))
        }
    };
    // DAG mode rejects sonnet_suggest at preflight (single-node-only in
    // v0). Mirrors the wave-20 / task 07 enforcement on the plan-field
    // surface.
    if let Some(err) = refuse_workstation_inference_in_dag_mode(args) {
        return Ok(err);
    }

    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("plan `{}` not found", id),
            )))
        }
    };
    if !matches!(plan.status, PlanStatus::Approved | PlanStatus::Executing) {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            format!(
                "plan status `{}` is not executable; approve it first via action=approve",
                plan.status.as_str()
            ),
        )));
    }

    // wave-18 / task 06 — autonomous PLAN field inference. We always run
    // the inference engine when `infer_plan_fields != off` and short-
    // circuit (preview / sonnet_suggest) or augment caller args (apply_safe).
    // When the mode is `off`, the variable below stays `None` and the
    // downstream pipeline observes byte-identical legacy behaviour.
    //
    // wave-20 / task 07 — `sonnet_suggest` extends the same engine with
    // an LLM proposal pass. The deterministic block runs unchanged FIRST
    // (so high-confidence determinism is never overridden by the model);
    // then Sonnet is asked to fill remaining fields. Proposals are
    // SURFACED, never auto-applied.
    //
    // wave-21 / task 05 — apply gate v1. Layered on top of the wave-18 /
    // wave-20 inference output. The new `apply_inferred_fields=true`
    // flag opts the caller into a CONTROLLED apply path (suggest-only
    // by default). The gate is suggest-only when the flag is absent so
    // the wave-18 byte-shape stays intact for back-compat callers.
    //
    // wave-22 / task 04 — persisted apply v2 splices in here as well.
    // When the caller opts into BOTH `apply_inferred_fields=true` AND
    // `persist_inference=true` AND `caller_approved=true` AND a matching
    // `proposal_hash`, the gate ALSO writes a new plan version + supersede
    // + typed evidence row. Preflight (hash mismatch / missing) fails
    // FAST as a structured error BEFORE any DB mutation per the contract.
    // Default behaviour (any flag absent / false) keeps the v1 byte-shape
    // exactly — `persisted_apply.status="not_requested"` lands on the
    // response so observers can pivot without re-deriving the policy.
    let mut effective_plan: Plan = plan.clone();
    let (effective_args, inference_block, apply_gate_block, persisted_apply_block): (
        Value,
        Option<Value>,
        Option<Value>,
        Option<Value>,
    ) = if matches!(infer_mode, InferPlanFieldsMode::Off) {
        // wave-22 / task 04 — emit a stable `not_requested`
        // persisted_apply block even when inference is OFF so
        // observers can pivot on a single shape regardless of
        // mode. The hash field defaults to a deterministic
        // placeholder (sha256 of the un-augmented sexp) so
        // dashboards can still cross-check provenance.
        let original_hash = sha256_hex(&plan.sexp_text);
        let not_requested = PersistedApplyOutcome::from_skip_reason(
            PersistedApplyStatus::NotRequested,
            args,
            &original_hash,
            &[],
            &[],
            None,
        );
        (
            args.clone(),
            None,
            None,
            Some(not_requested.to_response_json()),
        )
    } else {
        let project_arg = args.get("project").and_then(|v| v.as_str());
        let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
        let target_project_arg = args.get("target_project").and_then(|v| v.as_str());
        // 16 entries is the soft cap — recent dispatches dominate the
        // inferer's signal; older entries are rarely useful.
        let evidence_entries =
            read_recent_evidence_entries(state, id, project_arg, cwd_arg, target_project_arg, 16)
                .await;
        let plan_hints = parse_plan_hints(&plan.sexp_text);
        let input = PlanInferenceInput {
            plan_hints,
            plan_sexp: &plan.sexp_text,
            compiled_from: plan.compiled_from.as_deref(),
            evidence_entries: evidence_entries.clone(),
        };
        let mut inference = compute_plan_field_inference(args, &input);

        // wave-20 / task 07 — Sonnet pass. Runs only under
        // `sonnet_suggest`; failure surfaces as an `Unavailable` bundle
        // (NOT a silent fallback to deterministic-only). Proposals never
        // mutate caller args.
        if infer_mode.is_llm_augmented() {
            let bundle = request_llm_proposals(
                state,
                &plan.sexp_text,
                plan.compiled_from.as_deref(),
                &evidence_entries,
                &inference,
                args,
            )
            .await;
            inference.llm = Some(bundle);
        }

        let block = inference.to_response_json(infer_mode);

        // wave-21 / task 05 — compute the apply gate over the
        // inference result + caller args. Suggest-only when
        // `apply_inferred_fields` is absent (default false). When
        // opted in, deterministic high-confidence + no-conflict
        // fields are promoted into `applied_fields[]`; LLM
        // proposals are promoted only when caller approved them
        // explicitly via `llm_caller_approved`.
        let apply_outcome = compute_apply_gate(args, &inference);
        let gate_block = apply_outcome.to_response_json();

        // wave-22 / task 04 — persisted apply v2. Computes the v2
        // gate (4 opt-ins + matching hash) BEFORE the Preview /
        // SonnetSuggest short-circuit so the response always
        // carries a stable `persisted_apply` block — preview
        // callers can derive the deterministic correlator and
        // capture-and-replay against the persist path on a
        // follow-up call. Hash mismatch / missing fails FAST as
        // a structured error BEFORE any DB mutation per R2.
        let persist_outcome =
            match execute_persisted_apply(state, &plan, args, &apply_outcome).await {
                Ok(o) => o,
                Err((code, msg)) => {
                    return Ok(ToolResult::structured_error(ToolError::new(code, msg)));
                }
            };
        let persist_block = persist_outcome.to_response_json();
        // Refresh the plan snapshot when the persist path inserted
        // a new row, so downstream dispatch / evidence reads see
        // the post-persist version. plan_get keeps the same FSM
        // status (we inherit predecessor.status on insert), so
        // the Approved / Executing precondition is preserved.
        if persist_outcome.status.was_applied() {
            if let Some(new_id) = persist_outcome.new_plan_id {
                if let Ok(Some(refreshed)) = state.store.plan_get(new_id).await {
                    effective_plan = refreshed;
                }
            }
        }

        if matches!(
            infer_mode,
            InferPlanFieldsMode::Preview | InferPlanFieldsMode::SonnetSuggest
        ) {
            // Preview / sonnet_suggest short-circuit: never dispatch.
            // The apply gate AND the v2 persisted_apply block both
            // surface here so a preview caller can see what WOULD
            // apply / persist when the flags are flipped on a
            // follow-up call. Note that the persist path ITSELF
            // still ran (when all 4 opt-ins were supplied + hash
            // matched) — preview short-circuit means "no dispatch",
            // not "no persistence". Conservative: the short-circuit
            // only fires for the `Preview` / `SonnetSuggest`
            // inference modes; the ApplySafe mode falls through to
            // the dispatch pipeline below.
            let runner_status = if matches!(infer_mode, InferPlanFieldsMode::SonnetSuggest) {
                "inference_sonnet_suggest_no_dispatch"
            } else {
                "inference_preview_no_dispatch"
            };
            let status_label = if matches!(infer_mode, InferPlanFieldsMode::SonnetSuggest) {
                "inference_sonnet_suggest"
            } else {
                "inference_preview"
            };
            let payload = json!({
                "status": status_label,
                "execute_mode": execute_mode,
                "runner_status": runner_status,
                "plan_id": effective_plan.id,
                "board_task_id": effective_plan.board_task_id,
                "plan_field_inference": block,
                "apply_gate": gate_block,
                "persisted_apply": persist_block,
            });
            return Ok(ToolResult::json_pretty(&payload));
        }

        // ApplySafe path. When the wave-21 gate is REQUESTED, drive
        // the dispatch from the structured `applied_fields[]` (LLM
        // approvals included); otherwise keep the wave-18 byte-shape
        // by augmenting from the deterministic high-confidence slots
        // alone. Either way, the gate block lands on the response so
        // observers can audit the decision.
        let augmented = if apply_outcome.requested {
            let mut out = args.clone();
            if let Some(map) = out.as_object_mut() {
                for af in &apply_outcome.applied {
                    // Preserve caller-supplied values defensively —
                    // `compute_apply_gate` already routes those into
                    // `skipped_fields[]` with reason
                    // `caller_value_already_set`, so the slot here
                    // should already be empty. We double-check at
                    // the mutation site so a future regression is
                    // loud.
                    let already = map
                        .get(af.field)
                        .map(|v| match v {
                            Value::Null => false,
                            Value::String(s) => !s.trim().is_empty(),
                            Value::Array(a) => !a.is_empty(),
                            Value::Bool(_) => true,
                            _ => true,
                        })
                        .unwrap_or(false);
                    if already {
                        continue;
                    }
                    map.insert(af.field.to_string(), af.value.clone());
                }
            }
            out
        } else {
            apply_safe_augmentation(args, &inference)
        };
        (
            augmented,
            Some(block),
            Some(gate_block),
            Some(persist_block),
        )
    };
    let args = &effective_args;
    let plan = effective_plan;

    // wave-17 / task 01 — explicit PLAN-DAG paused-node resume hook.
    // When the caller supplies `resume_review_question_id` (with
    // `resume_review_decision`), we route through the dedicated resume
    // helper instead of the standard execute pipeline. The helper only
    // resumes one paused node — downstream nodes that were left
    // pending after the original paused dispatch stay pending until a
    // follow-up `mission_plan(execute)` call. This is NOT general
    // auto-approve: only ids whose envelope round-trips to a
    // paused-eligible node carry through.
    let resume_input = match parse_plan_node_resume_input(args) {
        Ok(r) => r,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                e.code(),
                e.message(),
            )))
        }
    };
    if let Some(input) = resume_input {
        if execute_mode != "internal" {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    "resume_review_question_id requires execute_mode=internal",
                )
                .with_suggestion(
                    "the paused-node resume hook dispatches inside the daemon; pass execute_mode=\"internal\"",
                ),
            ));
        }
        return super::super::plan_dag::action_execute_resume(state, args, &plan, input).await;
    }

    // scheduler_mode hook (Wave 12 / Task 02): when the caller asks for the
    // DAG scheduler, hand off to the dedicated module. The DAG scheduler only
    // makes sense in `execute_mode="internal"` (bridge mode is the v0
    // single-call descriptor and does not encode multi-node fan-out).
    match super::super::plan_dag::detect_scheduler_mode(args) {
        Ok(true) => {
            if execute_mode != "internal" {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        "scheduler_mode=dag_v1 requires execute_mode=internal",
                    )
                    .with_suggestion(
                        "DAG scheduler dispatches inside the daemon; pass execute_mode=\"internal\"",
                    ),
                ));
            }
            // wave-20 / task 07 — refuse infer_plan_fields=sonnet_suggest
            // here so the caller does not silently lose the LLM proposal
            // block when the DAG short-circuits ahead of any inference
            // pass. v0 keeps LLM-augmented inference single-node-only.
            if let Some(err) = super::super::plan_dag::refuse_llm_inference_in_dag_mode(args) {
                return Ok(err);
            }
            // wave-18 / task 05 — pre-flight cross-plan distill chain knobs
            // BEFORE the DAG runs so a typo (`distill_chain_mode="sonnett"`)
            // or an invalid combo (chain knobs without `finalize_plan=true`)
            // fails fast rather than after a long DAG execution. Validation
            // is pure (no AppState reads) so we can short-circuit here.
            if let Some(err) = validate_distill_chain_args(args) {
                return Ok(err);
            }
            let dag_result =
                super::super::plan_dag::action_execute_dag_v1(state, args, &plan).await?;
            // wave-18 / task 05 — augment the DAG result with the
            // cross-plan distill chain block (and an evidence sidecar
            // entry recording this plan's contribution to the chain).
            // No-op when chain knobs were not supplied.
            return Ok(apply_distill_chain(state, args, &plan, dag_result).await);
        }
        Ok(false) => {}
        Err(structured) => return Ok(structured),
    }

    // plan-runner auto-selection v1: parse hints up front so caller-omitted
    // target / dispatch knobs can be derived from PLAN.lisp itself.
    let hints = parse_plan_hints(&plan.sexp_text);

    let explicit_target = args
        .get("target")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let (target, target_source): (&'static str, &'static str) = if let Some(s) = explicit_target {
        match s {
            "mission_execution" => ("mission_execution", "explicit_arg"),
            "mission_task_delegate" => ("mission_task_delegate", "explicit_arg"),
            "mission_flow_run" => ("mission_flow_run", "explicit_arg"),
            _ => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!("execute target `{}` is not supported", s),
                    )
                    .with_suggestion(
                        "supported targets: mission_execution | mission_task_delegate | mission_flow_run",
                    ),
                ));
            }
        }
    } else if let Some(t) = hints
        .target
        .as_deref()
        .and_then(|s| normalize_target(s, hints.flow_id.is_some()))
    {
        (t, "plan_hint")
    } else {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::MISSING_PARAM,
                "execute requires `target` (mission_execution|mission_task_delegate|mission_flow_run); \
                 plan.sexp_text did not contain a usable :target / :target-tool / :tool hint",
            )
            .with_suggestion(
                "pass `target` explicitly, or add a :target hint (and :flow-id when targeting flow_run) to PLAN.lisp",
            ),
        ));
    };

    let explicit_ds = args
        .get("dispatch_strategy")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let (dispatch_strategy, dispatch_strategy_source) =
        resolve_dispatch_strategy(explicit_ds, &hints);

    let resolved = ResolvedExec {
        target,
        target_source,
        dispatch_strategy,
        dispatch_strategy_source,
        plan_hint_summary: hints.to_summary_json(),
    };

    // wave-21 / task 04 — autonomous workstation LLM proposal v0.
    // Compute the proposal bundle BEFORE the dispatch path runs so it
    // attaches uniformly to whichever response branch the dispatch lands
    // on (executing / dispatch_skipped / dry_run / inner_error / safe-
    // descriptor / bridge). The bundle is response-only metadata and
    // NEVER alters the dispatch path. Default mode `Off` ⇒ no bundle,
    // no Sonnet call, byte-compatible with wave-15..20.
    let workstation_proposal_bundle =
        compute_workstation_proposal_bundle(state, workstation_infer_mode, args, &plan, &hints)
            .await;

    // wave-22 / task 05 — autonomous workstation TRUE spawn v1. Layered
    // on top of wave-21 / task 04 propose-only. Default `auto_spawn=false`
    // ⇒ byte-compatible with wave-21 / task 04 (no gate block on the
    // response, no spawn). When `auto_spawn=true` the gate runs a strict
    // 12-rule matrix (G1..G12) and either:
    //   * spawns through the wave-15 substrate
    //     (`run_workstation_dispatch_with_contract`) when ALL gates pass, OR
    //   * skips with a structured SafeDescriptor-style outcome on the
    //     `workstation_auto_spawn_gate` block (NO spawn, NO mutation).
    //
    // Order of operations (mirrors wave-22 / task 03 / 04):
    //   1. Parse input — fail-fast on shape errors
    //      (`AUTO_SPAWN_INVALID_PARAM`).
    //   2. Hash preflight — fail-fast on missing / mismatch
    //      (`AUTO_SPAWN_MISSING_PROPOSAL_HASH` /
    //      `AUTO_SPAWN_PROPOSAL_HASH_MISMATCH`) BEFORE any substrate
    //      dispatch can run.
    //   3. Compute the gate decision (pure evaluator) and, when all 12
    //      gates pass, run the wave-15 substrate dispatch through
    //      `mission_task_delegate`. NEVER `claude -p`.
    let auto_spawn_input =
        match super::super::workstation_dispatch::parse_workstation_auto_spawn_input(args) {
            Ok(i) => i,
            Err((code, msg)) => {
                return Ok(ToolResult::structured_error(ToolError::new(
                    code.as_str(),
                    msg,
                )));
            }
        };
    if let Err((code, msg)) = super::super::workstation_dispatch::enforce_auto_spawn_preflight(
        &auto_spawn_input,
        workstation_proposal_bundle.as_ref(),
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            code.as_str(),
            msg,
        )));
    }
    let auto_spawn_gate_outcome = compute_workstation_auto_spawn_gate(
        state,
        &auto_spawn_input,
        &plan,
        &hints,
        workstation_proposal_bundle.as_ref(),
    )
    .await;

    let final_result = if execute_mode == "bridge" {
        action_execute_bridge(&plan, &resolved)
    } else {
        action_execute_internal(state, args, &plan, &resolved, &hints).await?
    };

    let final_result =
        attach_workstation_proposals_block(final_result, workstation_proposal_bundle.as_ref());

    // wave-22 / task 05 — splice the auto-spawn gate block onto the
    // response. No-op when the caller did not opt in (status=NotRequested
    // ⇒ block omitted so wave-21 / task 04 byte-shape is preserved).
    let final_result =
        attach_workstation_auto_spawn_gate_block(final_result, auto_spawn_gate_outcome.as_ref());

    let final_result = attach_inference_block(final_result, inference_block);
    let final_result = attach_apply_gate_block(final_result, apply_gate_block);
    let final_result = attach_persisted_apply_block(final_result, persisted_apply_block);

    // wave-24 / task 04 — splice the dry-run-only router recommendation
    // block onto the response. No-op when `router_policy_mode=off` (the
    // default) so wave-15..23 callers observe byte-identical behaviour.
    // The recommendation is INFORMATIONAL only — `applied` is hard-coded
    // `false` and the block sits alongside the existing dispatch fields
    // without altering them.
    let final_result = router_policy_dry_run::attach_router_recommendation_block(
        final_result,
        router_policy_mode,
        args,
        &resolved,
        &plan,
    );

    // wave-28 / task 04 — splice the dry-run-only task-runner manifest
    // block onto the response. No-op when `task_runner_mode=off` (the
    // default) so all wave-15..27 callers observe byte-identical
    // behaviour even when `task_runner_manifest_path` is supplied. The
    // block is INFORMATIONAL only — `applied` is hard-coded `false` and
    // the block sits alongside the existing dispatch + recommendation
    // fields without altering them.
    let final_result =
        task_runner_dry_run::attach_task_runner_block(final_result, task_runner_mode, args);
    Ok(final_result)
}

mod bridge;
mod internal;
mod workstation;
#[allow(unused_imports)]
pub(super) use workstation::{
    attach_workstation_auto_spawn_gate_block, attach_workstation_proposals_block,
    compute_workstation_auto_spawn_gate, compute_workstation_proposal_bundle,
    plan_hints_carry_workstation_signal,
};

pub(super) use bridge::{action_execute_bridge, attach_inference_block};
pub(super) use internal::action_execute_internal;
pub(super) use internal::PLAN_RUNNER_EVENT_REF_UNAVAILABLE_REASON;
