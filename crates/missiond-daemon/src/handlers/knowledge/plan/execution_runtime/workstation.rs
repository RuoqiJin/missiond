use super::*;

/// wave-21 / task 04 — compute the workstation proposal bundle for this
/// execute call. Returns `None` for the default `Off` mode so callers
/// observe byte-identical wave-15..20 behaviour. Returns `Some(bundle)`
/// when the operator opted in via `workstation_inference_mode="sonnet_suggest"`,
/// regardless of whether the gate fired (the bundle reports
/// `PlanHintsPresent` when the gate suppressed the Sonnet call).
pub(in crate::handlers::knowledge::plan) async fn compute_workstation_proposal_bundle(
    state: &AppState,
    mode: WorkstationInferenceMode,
    args: &Value,
    plan: &Plan,
    hints: &ParsedPlanHints,
) -> Option<super::super::super::workstation_dispatch::WorkstationProposalBundle> {
    if !mode.is_sonnet_suggest() {
        return None;
    }
    // Gate: caller silent + PLAN silent + no `:workstation-dispatch` opt-in.
    let merged_hints_for_gate = hints.to_workstation_hints().merge_args(args);
    let plan_hints_present_signal = plan_hints_carry_workstation_signal(hints);
    let caller_string = |k: &str| {
        args.get(k)
            .and_then(|v| v.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    };
    let gate = super::super::super::workstation_dispatch::WorkstationProposalGate {
        caller_target_present: caller_string("target"),
        caller_dispatch_strategy_present: caller_string("dispatch_strategy"),
        caller_objective_present: caller_string("objective"),
        caller_scope_present: caller_string("scope"),
        // owned_files presence is derived from the merged hints: if the
        // caller passed any non-empty list AND the merged set retained at
        // least one entry, that counts as a signal. We deliberately ignore
        // PLAN-supplied owned_files here because the plan-side list is
        // already covered by `plan_hints_present_signal`.
        caller_owned_files_present: !merged_hints_for_gate.owned_files.is_empty()
            && args.get("owned_files").is_some(),
        caller_project_signal_present: caller_string("target_project")
            || caller_string("requested_cwd")
            || caller_string("cwd"),
        plan_hints_present: plan_hints_present_signal,
        plan_workstation_opt_in: hints.workstation_dispatch_opt_in(),
        _marker: std::marker::PhantomData,
    };
    if gate.is_fully_silent() {
        // Fully silent ⇒ ask Sonnet to propose. Failure surfaces as an
        // Unavailable bundle; we NEVER fall back to claude -p / prompt
        // mode (the unavailable_reason text pins this invariant).
        Some(
            super::super::super::workstation_dispatch::request_workstation_proposals(
                state,
                &plan.sexp_text,
                plan.compiled_from.as_deref(),
            )
            .await,
        )
    } else {
        // Some signal present ⇒ skip the Sonnet pass and emit a typed
        // PlanHintsPresent bundle so the response surface stays uniform.
        Some(
            super::super::super::workstation_dispatch::WorkstationProposalBundle::plan_hints_present(
                gate.signal_summary(),
            ),
        )
    }
}

/// wave-21 / task 04 — true when the parsed PLAN.lisp hints carry any
/// workstation-relevant signal. Used by the proposal gate to decide
/// whether to suppress the Sonnet pass (signal already exists ⇒ surface
/// `PlanHintsPresent` instead).
///
/// "Signal" here means any of the eight workstation knobs the wave-15
/// parser exposes via `to_workstation_hints` PLUS the explicit
/// `:workstation-dispatch` flag (which `workstation_dispatch_opt_in`
/// reads separately).
pub(in crate::handlers::knowledge::plan) fn plan_hints_carry_workstation_signal(
    h: &ParsedPlanHints,
) -> bool {
    let nonblank = |o: &Option<String>| o.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false);
    nonblank(&h.objective)
        || nonblank(&h.summary)
        || nonblank(&h.scope)
        || nonblank(&h.owned_files_raw)
        || nonblank(&h.forbidden_files_raw)
        || nonblank(&h.acceptance_commands_raw)
        || nonblank(&h.commit_policy)
        || nonblank(&h.target_project)
        || nonblank(&h.requested_cwd)
        || nonblank(&h.dispatch_strategy)
}

/// wave-21 / task 04 — splice the `workstation_proposals` bundle onto a
/// successful response. Mirrors `attach_inference_block`: errors and
/// pre-existing keys are preserved untouched. The bundle is response-
/// only metadata; nothing reads it on the daemon side.
pub(in crate::handlers::knowledge::plan) fn attach_workstation_proposals_block(
    mut result: ToolResult,
    bundle: Option<&super::super::super::workstation_dispatch::WorkstationProposalBundle>,
) -> ToolResult {
    let Some(bundle) = bundle else {
        return result;
    };
    if result.is_error.unwrap_or(false) {
        // Don't decorate structured errors with the proposal block — the
        // caller needs the error path uncluttered.
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
        // Preserve any pre-existing block by NEVER overwriting (future
        // DAG / resume paths may carry their own).
        map.entry("workstation_proposals".to_string())
            .or_insert_with(|| bundle.to_response_json());
        // Mode echo so observers can pivot on the wire string without
        // re-deriving it from the bundle status.
        map.entry("workstation_inference_mode".to_string())
            .or_insert_with(|| json!(WORKSTATION_INFER_MODE_SONNET_SUGGEST));
    }
    result.content = vec![ToolContent::Text {
        text: serde_json::to_string_pretty(&payload).unwrap_or(text),
    }];
    result
}

/// wave-22 / task 05 — compute the auto-spawn gate outcome for this
/// execute call. Returns `None` when the caller did not opt in
/// (`auto_spawn=false` / absent) so observers see byte-identical
/// wave-21 / task 04 behaviour. Returns `Some(outcome)` when the
/// gate ran (whether it spawned or skipped) so the response can
/// surface the structured decision.
///
/// When the gate would have spawned (G1..G12 all green), this helper
/// ALSO calls the wave-15 substrate
/// (`run_workstation_dispatch_with_contract`) to perform the actual
/// dispatch — there is NEVER a `claude -p` shell-out. The substrate's
/// outcome (Dispatched / SafeDescriptor / DryRun / InnerError) is
/// folded back into the gate outcome's status:
///   * Dispatched ⇒ status=Spawned (load-bearing success)
///   * SafeDescriptor ⇒ status=SkippedSubstrateRefused + reason
///   * InnerError ⇒ status=SkippedSubstrateInnerError + reason
///   * DryRun ⇒ status=Spawned (no real dispatch happened, but the
///     gate decision was load-bearing — we treat dry runs as the
///     spawn decision having been made; the brief preview is
///     surfaced through the substrate's standard response fields).
pub(in crate::handlers::knowledge::plan) async fn compute_workstation_auto_spawn_gate(
    state: &AppState,
    input: &super::super::super::workstation_dispatch::WorkstationAutoSpawnInput,
    plan: &Plan,
    hints: &ParsedPlanHints,
    bundle: Option<&super::super::super::workstation_dispatch::WorkstationProposalBundle>,
) -> Option<super::super::super::workstation_dispatch::WorkstationAutoSpawnGateOutcome> {
    if !input.auto_spawn {
        // Caller did not opt in; gate block omitted from response so
        // wave-21 / task 04 byte-shape is preserved exactly.
        return None;
    }

    // Pre-load the contract so the gate evaluator can check
    // `:write-scope` / `:must-not-touch` BEFORE any spawn substrate
    // runs. We resolve relative paths against the same project anchor
    // the substrate would use; the substrate re-resolves on its own
    // path so this is purely defensive (the gate refuses early if
    // the file is malformed, instead of letting the substrate get
    // partway through dispatch).
    let (parsed_contract, contract_load_error): (
        Option<super::super::super::workstation_dispatch::ParsedTaskContract>,
        Option<String>,
    ) = if let Some(raw) = input.task_contract_path.as_deref() {
        let raw_path = std::path::Path::new(raw);
        // Use the daemon's process cwd as the anchor for relative
        // paths in the gate; the substrate re-anchors against the
        // resolved project root, which may differ — but for the
        // gate's purposes (checking write_scope shape + non-overlap)
        // the resolution does not matter, because the contract file
        // itself is the SSOT and parses identically regardless.
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
        let resolved =
            super::super::super::workstation_dispatch::resolve_contract_path_public(raw_path, &cwd);
        match super::super::super::workstation_dispatch::load_task_contract(&resolved) {
            Ok(c) => (Some(c), None),
            Err(e) => (None, Some(e.reason())),
        }
    } else {
        (None, None)
    };

    // Pure evaluator — no substrate dispatch yet.
    let mut outcome =
        super::super::super::workstation_dispatch::evaluate_workstation_auto_spawn_gate(
            &input,
            bundle,
            parsed_contract.as_ref(),
            contract_load_error.as_deref(),
        );

    // If the pure gate decided to spawn, run the substrate dispatch
    // through the wave-15 path. The gate's contract is the SSOT for
    // the spawn — we use ONLY the PLAN-derived hints (no caller-arg
    // overlay) so the spawn surface matches what the gate evaluated
    // (caller args are intentionally NOT load-bearing on the auto-
    // spawn path: the gate's authority comes from the validated
    // contract, not from any caller-supplied workstation knob).
    if outcome.status.was_spawned() {
        let merged_hints = hints.to_workstation_hints();
        // The gate already pinned spawn_target = mission_task_delegate.
        // dispatch_strategy is taken from the contract / merged hints
        // (the wave-15 substrate honours both).
        let dispatch_strategy = merged_hints
            .dispatch_strategy
            .clone()
            .unwrap_or_else(|| "agent-team".to_string());
        let raw_path = input
            .task_contract_path
            .as_deref()
            .map(std::path::PathBuf::from);
        let substrate_outcome =
            super::super::super::workstation_dispatch::run_workstation_dispatch_with_contract(
                state,
                plan,
                "mission_task_delegate",
                &dispatch_strategy,
                merged_hints,
                false, // dry_run=false: this is the real spawn surface
                raw_path.as_deref(),
            )
            .await;
        match substrate_outcome {
            super::super::super::workstation_dispatch::WorkstationDispatchOutcome::Dispatched {
                ..
            }
            | super::super::super::workstation_dispatch::WorkstationDispatchOutcome::DryRun { .. } => {
                // Spawn decision was load-bearing — keep status=Spawned.
                outcome.gate_results.push(
                    "rule:substrate_dispatch:ok (mission_task_delegate substrate accepted the spawn)"
                        .to_string(),
                );
            }
            super::super::super::workstation_dispatch::WorkstationDispatchOutcome::SafeDescriptor {
                reason,
                ..
            } => {
                let detail = format!(
                    "substrate refused: {} (status={})",
                    reason.detail(),
                    reason.status(),
                );
                outcome.gate_results.push(format!(
                    "rule:substrate_dispatch:safe_descriptor:{}",
                    detail
                ));
                outcome.status =
                    super::super::super::workstation_dispatch::WorkstationAutoSpawnStatus::SkippedSubstrateRefused;
                outcome.substrate_reason = Some(detail);
            }
            super::super::super::workstation_dispatch::WorkstationDispatchOutcome::InnerError {
                inner_payload,
                ..
            } => {
                let detail = format!(
                    "mission_task_delegate inner handler returned an error result: {}",
                    inner_payload
                );
                outcome
                    .gate_results
                    .push(format!("rule:substrate_dispatch:inner_error:{}", detail));
                outcome.status =
                    super::super::super::workstation_dispatch::WorkstationAutoSpawnStatus::SkippedSubstrateInnerError;
                outcome.substrate_reason = Some(detail);
            }
        }
    }

    Some(outcome)
}

/// wave-22 / task 05 — splice the `workstation_auto_spawn_gate` bundle
/// onto a successful response. Mirrors `attach_workstation_proposals_block`:
/// errors and pre-existing keys are preserved untouched. The block is
/// response-only metadata; nothing reads it on the daemon side.
pub(in crate::handlers::knowledge::plan) fn attach_workstation_auto_spawn_gate_block(
    mut result: ToolResult,
    outcome: Option<&super::super::super::workstation_dispatch::WorkstationAutoSpawnGateOutcome>,
) -> ToolResult {
    let Some(outcome) = outcome else {
        return result;
    };
    if result.is_error.unwrap_or(false) {
        // Don't decorate structured errors with the gate block — the
        // caller needs the error path uncluttered.
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
        // Preserve any pre-existing block by NEVER overwriting (future
        // DAG / resume paths may carry their own).
        map.entry("workstation_auto_spawn_gate".to_string())
            .or_insert_with(|| outcome.to_response_json());
    }
    result.content = vec![ToolContent::Text {
        text: serde_json::to_string_pretty(&payload).unwrap_or(text),
    }];
    result
}
