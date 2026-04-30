use super::*;

/// Wire status for the wave-22 / task 05 auto-spawn decision. Pinned as
/// a closed enum so dashboards can `grep` for stable strings without
/// inspecting the rest of the gate block. Each variant maps to exactly
/// one `auto_spawn_status` value on the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkstationAutoSpawnStatus {
    /// Caller did not opt in (`auto_spawn` absent / false). Gate block
    /// is omitted from the response so legacy callers stay byte-identical
    /// with wave-21 / task 04.
    NotRequested,
    /// All gates passed AND the spawn substrate ran successfully. The
    /// spawn-target / inner-result land on the response under the
    /// existing `workstation_dispatch_*` keys (the spawn re-uses the
    /// wave-15 `run_workstation_dispatch_with_contract` path).
    Spawned,
    /// Caller opted in but the bundle reported `Unavailable` (gateway
    /// not initialised / network failure). Gate refuses to synthesise
    /// a deterministic suggestion — there is NEVER a fallback to
    /// `claude -p` / prompt mode.
    SkippedUnavailable,
    /// Caller opted in but the bundle reported `NoSuggestions` /
    /// `PlanHintsPresent` / `NotInvoked`. No proposals to spawn against.
    SkippedNoProposals,
    /// Caller opted in but at least one proposal carried a non-`safe`
    /// `safety_status` (e.g. `ambiguous_value` / `unsupported_target` /
    /// `invalid_strategy`). The wave-21 whitelist is the SSOT here —
    /// the gate refuses to override the safety classifier.
    SkippedUnsafeProposal,
    /// Caller opted in but at least one proposal carried `confidence`
    /// other than `high`. The auto-spawn gate is deliberately stricter
    /// than the propose-only surface.
    SkippedConfidenceTooLow,
    /// Caller opted in but did not flip `caller_approved=true`. The
    /// double opt-in is required precisely so the gate cannot fire by
    /// a single accidental flag flip.
    SkippedCallerNotApproved,
    /// Caller opted in but did not supply `task_contract_path` (the
    /// contract is the SSOT for what the spawn is allowed to touch).
    SkippedMissingTaskContractPath,
    /// Caller opted in AND supplied `task_contract_path` but the file
    /// is missing / malformed / fails parse — the spawn refuses to
    /// proceed without a valid contract.
    SkippedMalformedTaskContract,
    /// Caller opted in but the contract's `:write-scope` is empty —
    /// a spawn against an empty write-scope contract has nothing to
    /// own; refuse defensively.
    SkippedEmptyWriteScope,
    /// Caller opted in but the contract's `:write-scope` overlaps with
    /// `:must-not-touch` — defensive refusal against a malformed
    /// contract that contradicts itself.
    SkippedForbiddenScopeOverlap,
    /// Caller opted in but did not acknowledge
    /// `preflight_status_acceptable=true`. The daemon does not run
    /// hooks itself; the gate refuses to spawn without explicit
    /// operator confirmation.
    SkippedPreflightUnacceptable,
    /// Caller opted in but the proposed `target` is not
    /// `mission_task_delegate`. The auto-spawn gate ALWAYS spawns
    /// through the wave-15 workstation substrate, which only wraps
    /// `mission_task_delegate`.
    SkippedUnsupportedTarget,
    /// All gates passed BUT the wave-15 substrate refused the dispatch
    /// (e.g. project root unresolved). The substrate's safe-descriptor
    /// reason flows through verbatim under
    /// `auto_spawn_substrate_reason` so the operator can fix and retry.
    SkippedSubstrateRefused,
    /// All gates passed BUT the wave-15 substrate's inner handler
    /// returned an error result. The inner payload flows through under
    /// `auto_spawn_inner_payload` so the operator can correlate.
    SkippedSubstrateInnerError,
}

impl WorkstationAutoSpawnStatus {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            WorkstationAutoSpawnStatus::NotRequested => "not_requested",
            WorkstationAutoSpawnStatus::Spawned => "spawned",
            WorkstationAutoSpawnStatus::SkippedUnavailable => "skipped_unavailable",
            WorkstationAutoSpawnStatus::SkippedNoProposals => "skipped_no_proposals",
            WorkstationAutoSpawnStatus::SkippedUnsafeProposal => "skipped_unsafe_proposal",
            WorkstationAutoSpawnStatus::SkippedConfidenceTooLow => "skipped_confidence_too_low",
            WorkstationAutoSpawnStatus::SkippedCallerNotApproved => "skipped_caller_not_approved",
            WorkstationAutoSpawnStatus::SkippedMissingTaskContractPath => {
                "skipped_missing_task_contract_path"
            }
            WorkstationAutoSpawnStatus::SkippedMalformedTaskContract => {
                "skipped_malformed_task_contract"
            }
            WorkstationAutoSpawnStatus::SkippedEmptyWriteScope => "skipped_empty_write_scope",
            WorkstationAutoSpawnStatus::SkippedForbiddenScopeOverlap => {
                "skipped_forbidden_scope_overlap"
            }
            WorkstationAutoSpawnStatus::SkippedPreflightUnacceptable => {
                "skipped_preflight_unacceptable"
            }
            WorkstationAutoSpawnStatus::SkippedUnsupportedTarget => "skipped_unsupported_target",
            WorkstationAutoSpawnStatus::SkippedSubstrateRefused => "skipped_substrate_refused",
            WorkstationAutoSpawnStatus::SkippedSubstrateInnerError => {
                "skipped_substrate_inner_error"
            }
        }
    }

    /// True iff the gate authorised the substrate dispatch AND the
    /// substrate ran successfully. Used by the response splicer to
    /// decide whether to attach the full inner payload.
    pub(crate) fn was_spawned(self) -> bool {
        matches!(self, WorkstationAutoSpawnStatus::Spawned)
    }
}

/// Pure outcome of [`evaluate_workstation_auto_spawn_gate`]. Side-effect
/// free — no DB, no bus, no LLM, no spawn. The handler consumes this
/// projection to decide whether to run the wave-15 substrate dispatch.
#[derive(Debug, Clone)]
pub(crate) struct WorkstationAutoSpawnGateOutcome {
    /// Whether the caller opted into the gate at all.
    pub requested: bool,
    /// Wire status — the load-bearing signal for observers.
    pub status: WorkstationAutoSpawnStatus,
    /// Spawn target the gate would have used (always
    /// `mission_task_delegate` when status=Spawned; carries the proposed
    /// target verbatim under SkippedUnsupportedTarget so the operator
    /// can see what was offered). `None` when no proposal exists.
    pub spawn_target: Option<String>,
    /// Validated `task_contract_path` (echoed for audit symmetry).
    pub task_contract_path: Option<String>,
    /// Result of the proposal-hash comparison.
    pub proposal_hash_status: WorkstationProposalHashStatus,
    /// Hash the gate computed from the bundle (always populated when a
    /// bundle exists; None when bundle is absent / unavailable).
    pub computed_proposal_hash: Option<String>,
    /// Caller-supplied hash (echoed for audit symmetry).
    pub supplied_proposal_hash: Option<String>,
    /// Whether the caller flipped `workstation_caller_approved=true`.
    pub caller_approved: bool,
    /// Whether the caller flipped `preflight_status_acceptable=true`.
    pub preflight_status_acceptable: bool,
    /// Flat list of `code:detail` strings explaining every gate's
    /// outcome. Always populated under non-NotRequested statuses.
    pub gate_results: Vec<String>,
    /// When the substrate dispatch ran (status=Spawned or status=
    /// SkippedSubstrateRefused / SkippedSubstrateInnerError), the
    /// substrate's safe-descriptor reason flows through here verbatim.
    pub substrate_reason: Option<String>,
}

impl WorkstationAutoSpawnGateOutcome {
    /// Build the wire shape consumed by the response payload. Always
    /// emits every field (with `null` for absent values) so observers
    /// can pivot on a stable shape regardless of which skip reason
    /// fired.
    pub(crate) fn to_response_json(&self) -> Value {
        json!({
            "requested": self.requested,
            "auto_spawn_status": self.status.as_wire(),
            "spawn_target": self.spawn_target.clone(),
            "task_contract_path": self.task_contract_path.clone(),
            "proposal_hash_status": self.proposal_hash_status.as_wire(),
            "computed_proposal_hash": self.computed_proposal_hash.clone(),
            "supplied_proposal_hash": self.supplied_proposal_hash.clone(),
            "caller_approved": self.caller_approved,
            "preflight_status_acceptable": self.preflight_status_acceptable,
            "gate_results": self.gate_results.clone(),
            "substrate_reason": self.substrate_reason.clone(),
        })
    }

    /// Construct a `NotRequested` outcome — used when the caller did
    /// not opt in. The gate block is omitted from the response in
    /// this case so wave-21 / task 04 byte-shape is preserved exactly.
    pub(crate) fn not_requested() -> Self {
        WorkstationAutoSpawnGateOutcome {
            requested: false,
            status: WorkstationAutoSpawnStatus::NotRequested,
            spawn_target: None,
            task_contract_path: None,
            proposal_hash_status: WorkstationProposalHashStatus::NotSupplied,
            computed_proposal_hash: None,
            supplied_proposal_hash: None,
            caller_approved: false,
            preflight_status_acceptable: false,
            gate_results: Vec::new(),
            substrate_reason: None,
        }
    }
}
