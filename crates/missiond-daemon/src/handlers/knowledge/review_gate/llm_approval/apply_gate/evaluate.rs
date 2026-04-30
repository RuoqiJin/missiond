use super::*;

/// Pure evaluator of the wave-22 / task 03 apply-gate. Does NOT mutate
/// state, does NOT compare hashes against a strict-error code (the
/// hash-mismatch / hash-missing fail-fast path runs in
/// [`enforce_apply_gate_preflight`] BEFORE this evaluator). This helper
/// produces the structured outcome the response carries; the handler
/// reads `outcome.status.should_apply()` to decide whether to run the
/// DB transition.
///
/// Inputs:
///   * `input`     — caller-supplied gate args (parsed via
///                    [`parse_llm_approve_apply_gate_input`]).
///   * `bundle`    — proposal bundle from
///                    [`request_*_auto_approve_proposal`].
///   * `action`    — review action label (folded into hash + destructive
///                    rule).
///   * `artifact_id` / `version` — artifact identity (folded into hash).
pub(crate) fn evaluate_llm_approve_apply_gate(
    input: &LlmApproveApplyGateInput,
    bundle: &LlmAutoApproveProposalBundle,
    action: &str,
    artifact_id: &str,
    version: i32,
) -> LlmApproveApplyGateOutcome {
    let mut rule_results: Vec<String> = Vec::new();

    // Compute the hash + hash status up-front so observers always see
    // the deterministic verdict (regardless of whether the gate ran).
    let (computed_hash, hash_status) = match bundle.proposal.as_ref() {
        Some(p) => {
            let hash = compute_proposal_hash(action, artifact_id, version, p);
            let status = match input.proposal_hash.as_deref() {
                None => ProposalHashStatus::NotSupplied,
                Some(s) if s.eq_ignore_ascii_case(&hash) => ProposalHashStatus::Matches,
                Some(_) => ProposalHashStatus::Mismatch,
            };
            (Some(hash), status)
        }
        None => (None, ProposalHashStatus::NoProposalAvailable),
    };

    // G1 — caller opted in. Default short-circuit returns
    // `NotRequested` so the response stays byte-identical with
    // wave-21 / task 06 callers.
    if !input.apply {
        return LlmApproveApplyGateOutcome {
            requested: false,
            status: LlmApproveApplyStatus::NotRequested,
            applied_decision: bundle.proposal.as_ref().map(|p| p.decision),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            safety_rule_results: rule_results,
        };
    }

    // G4 (early) — destructive action ALWAYS skips, regardless of any
    // other gate outcome (invariant I2). Pinned BEFORE bundle status so
    // the rule-result list shows the deterministic refusal even if the
    // bundle is `DestructiveBlocked` (which already says the same thing
    // — but having BOTH layers loud means a future regression that
    // forgot to short-circuit destructive in `request_*` cannot sneak
    // through to apply).
    let destructive = is_destructive_review_action(action);
    if destructive {
        rule_results.push(format!(
            "rule:destructive_action:`{}` is on the destructive list (archive|supersede|remove); apply gate refuses to promote (invariant I2)",
            action.trim().to_ascii_lowercase()
        ));
    } else {
        rule_results.push(format!(
            "rule:non_destructive_action:`{}` is not on the destructive list",
            action.trim().to_ascii_lowercase()
        ));
    }

    // G2 — proposal hash status. Note the strict pre-flight in
    // `enforce_apply_gate_preflight` already rejected mismatch / missing
    // BEFORE we got here; surfacing the rule result keeps the audit
    // trail loud even in test paths that bypass the preflight.
    match hash_status {
        ProposalHashStatus::Matches => {
            rule_results.push("rule:proposal_hash:matches".to_string());
        }
        ProposalHashStatus::NotSupplied => {
            rule_results.push(
                "rule:proposal_hash:not_supplied (apply gate requires explicit hash echo)"
                    .to_string(),
            );
        }
        ProposalHashStatus::Mismatch => {
            rule_results.push(
                "rule:proposal_hash:mismatch (caller-supplied hash does not match bundle)"
                    .to_string(),
            );
        }
        ProposalHashStatus::NoProposalAvailable => {
            rule_results.push(
                "rule:proposal_hash:no_proposal_available (bundle has no proposal to hash)"
                    .to_string(),
            );
        }
    }

    // G3 — caller_approved double opt-in.
    if input.caller_approved {
        rule_results.push("rule:caller_approved:true".to_string());
    } else {
        rule_results.push(
            "rule:caller_approved:false (apply gate requires the explicit caller_approved=true confirmation)"
                .to_string(),
        );
    }

    // Bundle-status branches.
    match bundle.status {
        LlmAutoApproveProposalStatus::NotInvoked => {
            // The caller opted into the gate but the bundle was never
            // invoked (proposer mode is off / absent). This is a config
            // bug on the caller side — the gate cannot apply without a
            // proposal. We surface it as `skipped_no_proposal`.
            rule_results.push(
                "rule:bundle_status:not_invoked (proposer mode `off` / absent — no proposal to apply)"
                    .to_string(),
            );
            return LlmApproveApplyGateOutcome {
                requested: true,
                status: LlmApproveApplyStatus::SkippedNoProposal,
                applied_decision: None,
                proposal_hash_status: hash_status,
                computed_proposal_hash: computed_hash,
                supplied_proposal_hash: input.proposal_hash.clone(),
                caller_approved: input.caller_approved,
                safety_rule_results: rule_results,
            };
        }
        LlmAutoApproveProposalStatus::Unavailable => {
            // Invariant I4 — Sonnet unavailable; gate refuses to
            // synthesise a fallback.
            rule_results.push(
                "rule:bundle_status:unavailable (Sonnet gateway unavailable; gate refuses fallback per invariant I4)"
                    .to_string(),
            );
            return LlmApproveApplyGateOutcome {
                requested: true,
                status: LlmApproveApplyStatus::SkippedUnavailable,
                applied_decision: None,
                proposal_hash_status: hash_status,
                computed_proposal_hash: computed_hash,
                supplied_proposal_hash: input.proposal_hash.clone(),
                caller_approved: input.caller_approved,
                safety_rule_results: rule_results,
            };
        }
        LlmAutoApproveProposalStatus::NoSuggestion => {
            rule_results.push(
                "rule:bundle_status:no_suggestion (Sonnet returned an unparseable / empty response)"
                    .to_string(),
            );
            return LlmApproveApplyGateOutcome {
                requested: true,
                status: LlmApproveApplyStatus::SkippedNoProposal,
                applied_decision: None,
                proposal_hash_status: hash_status,
                computed_proposal_hash: computed_hash,
                supplied_proposal_hash: input.proposal_hash.clone(),
                caller_approved: input.caller_approved,
                safety_rule_results: rule_results,
            };
        }
        LlmAutoApproveProposalStatus::DestructiveBlocked => {
            // Invariant I2 (already pinned by G4 above) — we don't even
            // reach the proposal here in v0; the request_* helper short-
            // circuited. Belt-and-braces refusal.
            rule_results.push(
                "rule:bundle_status:destructive_blocked (invariant I2 short-circuited the proposer)"
                    .to_string(),
            );
            return LlmApproveApplyGateOutcome {
                requested: true,
                status: LlmApproveApplyStatus::SkippedDestructiveAction,
                applied_decision: bundle.proposal.as_ref().map(|p| p.decision),
                proposal_hash_status: hash_status,
                computed_proposal_hash: computed_hash,
                supplied_proposal_hash: input.proposal_hash.clone(),
                caller_approved: input.caller_approved,
                safety_rule_results: rule_results,
            };
        }
        LlmAutoApproveProposalStatus::Suggested => {
            rule_results.push(
                "rule:bundle_status:suggested (proposal survived parser + invariants)".to_string(),
            );
        }
    }

    let proposal = bundle.proposal.as_ref().expect(
        "Suggested status guarantees proposal is Some — see LlmAutoApproveProposalBundle invariants",
    );

    // G4 (deterministic re-check) — destructive action.
    if destructive {
        return LlmApproveApplyGateOutcome {
            requested: true,
            status: LlmApproveApplyStatus::SkippedDestructiveAction,
            applied_decision: Some(proposal.decision),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            safety_rule_results: rule_results,
        };
    }

    // Invariant I5 cross-check — the proposal's destructive_check field
    // is informational, but if the model lied (claimed non_destructive
    // for an actually destructive action — unreachable here because
    // destructive short-circuits above, BUT we keep the check as a
    // belt-and-braces guard in case a future caller bypasses the
    // request_* helper) we surface the lie loudly.
    let model_says_destructive = proposal.destructive_check.starts_with("destructive:");
    if destructive != model_says_destructive {
        rule_results.push(format!(
            "rule:invariant_i5:deterministic_override (deterministic destructive={} but proposal.destructive_check=`{}`; gate trusts deterministic verdict)",
            destructive,
            proposal.destructive_check.split(':').next().unwrap_or("?"),
        ));
    } else {
        rule_results.push(format!(
            "rule:invariant_i5:proposal_matches_deterministic (destructive={})",
            destructive,
        ));
    }

    // G2 — hash status terminal check. Mismatch / missing = SKIP. Note
    // the strict pre-flight in `enforce_apply_gate_preflight` should
    // have already converted these into structured errors; surfacing
    // the SKIP here means the test path (which calls the evaluator
    // directly without the preflight) still gets a sane outcome.
    match hash_status {
        ProposalHashStatus::Matches => {}
        ProposalHashStatus::NotSupplied
        | ProposalHashStatus::Mismatch
        | ProposalHashStatus::NoProposalAvailable => {
            return LlmApproveApplyGateOutcome {
                requested: true,
                // Hash gate failures collapse to SkippedCallerNotApproved
                // ONLY when the strict preflight was bypassed (e.g. unit
                // tests). The preflight's structured error is the
                // production path; this fallback keeps the evaluator
                // pure even when it stands alone.
                status: LlmApproveApplyStatus::SkippedCallerNotApproved,
                applied_decision: Some(proposal.decision),
                proposal_hash_status: hash_status,
                computed_proposal_hash: computed_hash,
                supplied_proposal_hash: input.proposal_hash.clone(),
                caller_approved: input.caller_approved,
                safety_rule_results: rule_results,
            };
        }
    }

    // G3 — caller_approved double opt-in.
    if !input.caller_approved {
        return LlmApproveApplyGateOutcome {
            requested: true,
            status: LlmApproveApplyStatus::SkippedCallerNotApproved,
            applied_decision: Some(proposal.decision),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            safety_rule_results: rule_results,
        };
    }

    // G5 — decision must be Approved (invariant I1: never auto-reject;
    // we also never auto-anything-other-than-approve here).
    if !matches!(proposal.decision, ReviewDecision::Approved) {
        rule_results.push(format!(
            "rule:decision_not_approved (proposal.decision={}; gate only applies `approved`, never `needs_changes` per invariant I1)",
            proposal.decision.as_str(),
        ));
        return LlmApproveApplyGateOutcome {
            requested: true,
            status: LlmApproveApplyStatus::SkippedNonApprovedDecision,
            applied_decision: Some(proposal.decision),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            safety_rule_results: rule_results,
        };
    }
    rule_results.push("rule:decision_approved (proposal.decision=approved)".to_string());

    // G6 — confidence must be High.
    if !matches!(proposal.confidence, LlmAutoApproveProposalConfidence::High) {
        rule_results.push(format!(
            "rule:confidence_too_low (proposal.confidence={}; gate requires `high`)",
            proposal.confidence.as_str(),
        ));
        return LlmApproveApplyGateOutcome {
            requested: true,
            status: LlmApproveApplyStatus::SkippedConfidenceTooLow,
            applied_decision: Some(proposal.decision),
            proposal_hash_status: hash_status,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: input.proposal_hash.clone(),
            caller_approved: input.caller_approved,
            safety_rule_results: rule_results,
        };
    }
    rule_results.push("rule:confidence_high (proposal.confidence=high)".to_string());

    // All gates passed.
    rule_results.push(
        "rule:apply_gate_satisfied (G1..G6 all green; handler may run the existing approve transition)"
            .to_string(),
    );
    LlmApproveApplyGateOutcome {
        requested: true,
        status: LlmApproveApplyStatus::Applied,
        applied_decision: Some(proposal.decision),
        proposal_hash_status: hash_status,
        computed_proposal_hash: computed_hash,
        supplied_proposal_hash: input.proposal_hash.clone(),
        caller_approved: input.caller_approved,
        safety_rule_results: rule_results,
    }
}
