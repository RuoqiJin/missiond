use super::*;

/// Helper: extract a proposal's value-as-string (proposals are always
/// string-shaped in v0; the helper exists so the gate evaluator can
/// stay terse).
fn proposal_value_str(p: &WorkstationProposal) -> Option<String> {
    p.value.as_str().map(|s| s.trim().to_string())
}

/// Helper: extract the proposed `target` value from the bundle (or
/// `None` when no `target` proposal is present). Used by the gate to
/// pin the spawn target before validating against the
/// `mission_task_delegate` whitelist.
fn extract_proposed_target(bundle: &WorkstationProposalBundle) -> Option<String> {
    bundle
        .proposals
        .iter()
        .find(|p| p.field == "target")
        .and_then(proposal_value_str)
        .filter(|s| !s.is_empty())
}

/// Strict pre-flight for the wave-22 / task 05 auto-spawn gate. Runs
/// the fail-fast hash-missing / hash-mismatch checks BEFORE any spawn
/// substrate dispatch. Returns `Ok(())` when:
///   * caller did not opt in (`auto_spawn=false`);
///   * caller opted in AND supplied a hash that matches.
/// Returns `Err((code, message))` for the two contract-mandated
/// structured errors:
///   * `AUTO_SPAWN_MISSING_PROPOSAL_HASH` — `auto_spawn=true` without a hash.
///   * `AUTO_SPAWN_PROPOSAL_HASH_MISMATCH` — `auto_spawn=true` with a hash
///     that does not match the bundle.
///
/// The handler converts the Err into [`ToolResult::structured_error`]
/// BEFORE calling the wave-15 substrate dispatch, satisfying the
/// contract: "On hash mismatch / missing, return structured error and
/// do not spawn."
pub(crate) fn enforce_auto_spawn_preflight(
    input: &WorkstationAutoSpawnInput,
    bundle: Option<&WorkstationProposalBundle>,
) -> std::result::Result<(), (String, String)> {
    if !input.auto_spawn {
        return Ok(());
    }
    let bundle = match bundle {
        Some(b) => b,
        None => {
            // Without a bundle we cannot compute a hash. Surface the
            // missing-hash code so the caller knows to opt into
            // `workstation_inference_mode="sonnet_suggest"` first (or
            // to drop the auto_spawn flag).
            if input.proposal_hash.is_none() {
                return Err((
                    AUTO_SPAWN_MISSING_PROPOSAL_HASH.to_string(),
                    "auto_spawn=true requires `workstation_proposal_hash` AND a workstation \
                     proposal bundle to apply against; bundle is absent (set \
                     workstation_inference_mode=\"sonnet_suggest\" first)"
                        .to_string(),
                ));
            }
            return Err((
                AUTO_SPAWN_PROPOSAL_HASH_MISMATCH.to_string(),
                "auto_spawn=true with `workstation_proposal_hash` but no workstation \
                 proposal bundle is available to compare against"
                    .to_string(),
            ));
        }
    };
    let hash = compute_workstation_proposal_hash(bundle);
    match input.proposal_hash.as_deref() {
        None => Err((
            AUTO_SPAWN_MISSING_PROPOSAL_HASH.to_string(),
            format!(
                "auto_spawn=true requires `workstation_proposal_hash`; expected `{}` (echoed under \
                 `workstation_auto_spawn_gate.computed_proposal_hash` in the propose-only response)",
                hash,
            ),
        )),
        Some(s) if s.eq_ignore_ascii_case(&hash) => Ok(()),
        Some(s) => Err((
            AUTO_SPAWN_PROPOSAL_HASH_MISMATCH.to_string(),
            format!(
                "auto_spawn=true with `workstation_proposal_hash=`{}`` does not match bundle hash `{}`",
                s, hash,
            ),
        )),
    }
}

/// Pure evaluator of the wave-22 / task 05 auto-spawn gate. Does NOT
/// mutate state, does NOT spawn a workstation, does NOT call any
/// substrate. Computes the structured outcome the response carries;
/// the handler reads `outcome.status.was_spawned()` to decide whether
/// to attach the substrate dispatch payload.
///
/// Hash mismatch / missing is allowed to surface here as a SKIP
/// (`SkippedCallerNotApproved`) so unit tests that bypass the
/// preflight still get a sane outcome — production paths run the
/// preflight FIRST and hit the structured-error return BEFORE this
/// evaluator.
///
/// Inputs:
///   * `input`               — caller-supplied gate args (parsed via
///                              `parse_workstation_auto_spawn_input`).
///   * `bundle`              — proposal bundle from
///                              `request_workstation_proposals`.
///   * `parsed_contract`     — task-contract v1 already parsed by the
///                              caller, OR `None` when the caller did
///                              not supply / failed to load the path.
///   * `contract_load_error` — typed parse failure when present (used
///                              to distinguish "missing path" from
///                              "malformed file" in the gate output).
pub(crate) fn evaluate_workstation_auto_spawn_gate(
    input: &WorkstationAutoSpawnInput,
    bundle: Option<&WorkstationProposalBundle>,
    parsed_contract: Option<&ParsedTaskContract>,
    contract_load_error: Option<&str>,
) -> WorkstationAutoSpawnGateOutcome {
    let mut gate_results: Vec<String> = Vec::new();

    // Compute the hash + hash status up-front so observers always see
    // the deterministic verdict (regardless of whether the gate ran).
    let (computed_hash, hash_status) = match bundle {
        Some(b) if !b.proposals.is_empty() => {
            let hash = compute_workstation_proposal_hash(b);
            let status = match input.proposal_hash.as_deref() {
                None => WorkstationProposalHashStatus::NotSupplied,
                Some(s) if s.eq_ignore_ascii_case(&hash) => WorkstationProposalHashStatus::Matches,
                Some(_) => WorkstationProposalHashStatus::Mismatch,
            };
            (Some(hash), status)
        }
        Some(_) | None => (None, WorkstationProposalHashStatus::NoProposalAvailable),
    };

    let proposed_target = bundle.and_then(extract_proposed_target);

    // G1 — caller opted in. Default short-circuit returns NotRequested
    // so the response stays byte-identical with wave-21 / task 04
    // callers that never see the knob.
    if !input.auto_spawn {
        return WorkstationAutoSpawnGateOutcome::not_requested();
    }
    gate_results.push("rule:g1_auto_spawn_opt_in:true".to_string());

    // Build a partial outcome for the SKIP branches below — every
    // SKIP carries the deterministic hash verdict + caller flags so
    // dashboards can pivot uniformly.
    let mk_skip = |status: WorkstationAutoSpawnStatus,
                   gate_results: Vec<String>,
                   substrate_reason: Option<String>|
     -> WorkstationAutoSpawnGateOutcome {
        WorkstationAutoSpawnGateOutcome {
            requested: true,
            status,
            spawn_target: proposed_target.clone(),
            task_contract_path: input.task_contract_path.clone(),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash.clone(),
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            preflight_status_acceptable: input.preflight_status_acceptable,
            gate_results,
            substrate_reason,
        }
    };

    // G2 — proposal bundle present and Suggested.
    let bundle = match bundle {
        Some(b) => b,
        None => {
            gate_results.push(
                "rule:g2_bundle_status:absent (workstation_inference_mode=\"sonnet_suggest\" \
                 not set; nothing to spawn against)"
                    .to_string(),
            );
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedNoProposals,
                gate_results,
                None,
            );
        }
    };
    match bundle.status {
        WorkstationProposalStatus::Unavailable => {
            gate_results.push(
                "rule:g2_bundle_status:llm_unavailable (Sonnet gateway unavailable; gate refuses \
                 fallback to claude -p / prompt mode)"
                    .to_string(),
            );
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedUnavailable,
                gate_results,
                None,
            );
        }
        WorkstationProposalStatus::NotInvoked
        | WorkstationProposalStatus::NoSuggestions
        | WorkstationProposalStatus::PlanHintsPresent => {
            gate_results.push(format!(
                "rule:g2_bundle_status:{} (no proposals to spawn against)",
                bundle.status.as_wire(),
            ));
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedNoProposals,
                gate_results,
                None,
            );
        }
        WorkstationProposalStatus::Suggested => {
            gate_results.push("rule:g2_bundle_status:suggested".to_string());
        }
    }
    if bundle.proposals.is_empty() {
        gate_results.push("rule:g2_bundle_status:suggested_but_empty (defensive)".to_string());
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedNoProposals,
            gate_results,
            None,
        );
    }

    // G3 — proposal hash matches.
    match hash_status {
        WorkstationProposalHashStatus::Matches => {
            gate_results.push("rule:g3_proposal_hash:matches".to_string());
        }
        WorkstationProposalHashStatus::NotSupplied => {
            gate_results.push(
                "rule:g3_proposal_hash:not_supplied (gate requires explicit hash echo; \
                 production path runs preflight first and fail-fasts BEFORE this evaluator)"
                    .to_string(),
            );
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedCallerNotApproved,
                gate_results,
                None,
            );
        }
        WorkstationProposalHashStatus::Mismatch => {
            gate_results.push(
                "rule:g3_proposal_hash:mismatch (caller-supplied hash does not match bundle; \
                 production path runs preflight first and fail-fasts BEFORE this evaluator)"
                    .to_string(),
            );
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedCallerNotApproved,
                gate_results,
                None,
            );
        }
        WorkstationProposalHashStatus::NoProposalAvailable => {
            // Already handled by G2 above; defensive.
            gate_results
                .push("rule:g3_proposal_hash:no_proposal_available (defensive)".to_string());
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedNoProposals,
                gate_results,
                None,
            );
        }
    }

    // G4 — every proposal carries safety_status=safe (wave-21
    // whitelist enforcement).
    let unsafe_proposals: Vec<&WorkstationProposal> = bundle
        .proposals
        .iter()
        .filter(|p| p.safety_status != WorkstationProposalSafetyStatus::Safe)
        .collect();
    if !unsafe_proposals.is_empty() {
        let summary = unsafe_proposals
            .iter()
            .map(|p| format!("{}={}", p.field, p.safety_status.as_wire()))
            .collect::<Vec<_>>()
            .join(",");
        gate_results.push(format!(
            "rule:g4_safety_status:non_safe_proposals=[{}] (wave-21 whitelist refuses to spawn \
             against ambiguous_value / unsupported_target / invalid_strategy)",
            summary,
        ));
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedUnsafeProposal,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g4_safety_status:all_safe".to_string());

    // G5 — every proposal carries confidence=high.
    let low_confidence: Vec<&WorkstationProposal> = bundle
        .proposals
        .iter()
        .filter(|p| p.confidence != WorkstationProposalConfidence::High)
        .collect();
    if !low_confidence.is_empty() {
        let summary = low_confidence
            .iter()
            .map(|p| format!("{}={}", p.field, p.confidence.as_wire()))
            .collect::<Vec<_>>()
            .join(",");
        gate_results.push(format!(
            "rule:g5_confidence:non_high_proposals=[{}] (auto-spawn gate is deliberately \
             stricter than propose-only)",
            summary,
        ));
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedConfidenceTooLow,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g5_confidence:all_high".to_string());

    // G6 — caller_approved double opt-in.
    if !input.caller_approved {
        gate_results.push(
            "rule:g6_caller_approved:false (apply gate requires the explicit \
             workstation_caller_approved=true confirmation)"
                .to_string(),
        );
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedCallerNotApproved,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g6_caller_approved:true".to_string());

    // G7 — preflight_status_acceptable opt-in. The daemon does NOT
    // run hooks itself; this is the explicit operator confirmation
    // surface.
    if !input.preflight_status_acceptable {
        gate_results.push(
            "rule:g7_preflight_status_acceptable:false (gate refuses to spawn without explicit \
             operator confirmation that hooks / preflight state is acceptable)"
                .to_string(),
        );
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedPreflightUnacceptable,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g7_preflight_status_acceptable:true".to_string());

    // G8 — task_contract_path supplied.
    if input.task_contract_path.is_none() {
        gate_results.push(
            "rule:g8_task_contract_path:missing (contract is the SSOT for what the spawn is \
             allowed to touch; no contract ⇒ no spawn)"
                .to_string(),
        );
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedMissingTaskContractPath,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g8_task_contract_path:supplied".to_string());

    // G9 — task_contract loaded successfully.
    let contract = match parsed_contract {
        Some(c) => c,
        None => {
            let detail = contract_load_error
                .map(|e| format!(": {}", e))
                .unwrap_or_default();
            gate_results.push(format!(
                "rule:g9_task_contract_load:failed{} (gate refuses to spawn without a valid \
                 task-contract v1 file)",
                detail,
            ));
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedMalformedTaskContract,
                gate_results,
                contract_load_error.map(|s| s.to_string()),
            );
        }
    };
    gate_results.push("rule:g9_task_contract_load:ok".to_string());

    // G10 — write_scope non-empty.
    if contract.write_scope.is_empty() {
        gate_results.push(
            "rule:g10_write_scope:empty (refusing to spawn against a contract with no \
             :write-scope — there is nothing for the spawn to own)"
                .to_string(),
        );
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedEmptyWriteScope,
            gate_results,
            None,
        );
    }
    gate_results.push(format!(
        "rule:g10_write_scope:non_empty (count={})",
        contract.write_scope.len(),
    ));

    // G11 — :must-not-touch must NOT overlap with :write-scope.
    let overlap: Vec<String> = contract
        .write_scope
        .iter()
        .filter(|p| contract.must_not_touch.iter().any(|f| f.trim() == p.trim()))
        .cloned()
        .collect();
    if !overlap.is_empty() {
        gate_results.push(format!(
            "rule:g11_forbidden_scope_overlap:[{}] (defensive refusal against a contract that \
             contradicts itself — :write-scope intersects :must-not-touch)",
            overlap.join(","),
        ));
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedForbiddenScopeOverlap,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g11_forbidden_scope_overlap:none".to_string());

    // G12 — proposed target must be mission_task_delegate. The wave-15
    // substrate (`run_workstation_dispatch_with_contract`) ONLY wraps
    // `mission_task_delegate`; we pin the same invariant here so the
    // gate refuses BEFORE the substrate would.
    let target_value = match proposed_target.as_deref() {
        Some(s) => s,
        None => {
            // No `target` proposal ⇒ caller's intent is ambiguous.
            // Refuse defensively.
            gate_results.push(
                "rule:g12_spawn_target:missing (no `target` proposal in the bundle — refusing \
                 to spawn against an ambiguous target)"
                    .to_string(),
            );
            return mk_skip(
                WorkstationAutoSpawnStatus::SkippedUnsupportedTarget,
                gate_results,
                None,
            );
        }
    };
    if !target_value.eq_ignore_ascii_case("mission_task_delegate") {
        gate_results.push(format!(
            "rule:g12_spawn_target:unsupported (proposed target=`{}` — auto-spawn always \
             routes through mission_task_delegate substrate, never claude -p)",
            target_value,
        ));
        return mk_skip(
            WorkstationAutoSpawnStatus::SkippedUnsupportedTarget,
            gate_results,
            None,
        );
    }
    gate_results.push("rule:g12_spawn_target:mission_task_delegate".to_string());

    // All gates passed. The handler will run the wave-15 substrate
    // dispatch with the validated contract and update this outcome
    // to status=Spawned (or SkippedSubstrate{Refused,InnerError}
    // depending on the substrate result).
    gate_results.push(
        "rule:auto_spawn_gate_satisfied (G1..G12 all green; handler may run \
         run_workstation_dispatch_with_contract through mission_task_delegate substrate)"
            .to_string(),
    );
    WorkstationAutoSpawnGateOutcome {
        requested: true,
        status: WorkstationAutoSpawnStatus::Spawned,
        spawn_target: Some("mission_task_delegate".to_string()),
        task_contract_path: input.task_contract_path.clone(),
        proposal_hash_status: hash_status,
        computed_proposal_hash: computed_hash,
        supplied_proposal_hash: input.proposal_hash.clone(),
        caller_approved: input.caller_approved,
        preflight_status_acceptable: input.preflight_status_acceptable,
        gate_results,
        substrate_reason: None,
    }
}
