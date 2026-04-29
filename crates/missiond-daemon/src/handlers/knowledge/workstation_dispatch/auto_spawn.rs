use serde_json::{json, Value};

use super::descriptor::ParsedTaskContract;
use super::proposal::{
    proposal_json_kind, WorkstationProposal, WorkstationProposalBundle,
    WorkstationProposalConfidence, WorkstationProposalSafetyStatus, WorkstationProposalStatus,
};

// ── wave-22 / task 05 — autonomous workstation true spawn v1 ────────────
//
// Layered on top of the wave-21 / task 04 propose-only pass. The wave-21
// surface was deliberately SURFACE-only (every proposal carried
// `applied=false` and the bundle carried `auto_spawn=false`) so an
// operator could read the proposals and decide whether to re-issue with
// explicit args. Wave-22 / task 05 promotes that propose-only path to a
// CONDITIONAL real spawn under a new `auto_spawn=true` opt-in, gated by
// a multi-rule strict matrix that mirrors wave-22 / task 03's apply gate.
//
// Conservative invariants pinned at the call-site (NEVER violated):
//   * Default `auto_spawn=false` ⇒ byte-compatible with wave-21 / task 04.
//     No spawn, no preflight error, no response augmentation. Every
//     proposal STILL carries `applied=false` on the wire and the
//     wave-21 bundle STILL carries `auto_spawn=false` (the propose-only
//     surface is preserved verbatim — wave-22 surfaces auto-spawn on a
//     SEPARATE `workstation_auto_spawn_gate` block).
//   * Sonnet unavailable ⇒ gate refuses to spawn (`SkippedUnavailable`).
//     There is NEVER a fallback to `claude -p` / prompt mode (the gate
//     status text pins this invariant exactly like the wave-21 bundle).
//   * DAG mode rejects auto_spawn at preflight (mirrors the wave-21
//     `refuse_workstation_inference_in_dag_mode` rule — the gate is
//     single-node-execute-only in v1).
//   * The bundle must be `Suggested` AND every proposal must carry
//     `safety_status=safe` AND `confidence=high` for the gate to fire.
//     The wave-21 conservative whitelists (target ∈ {mission_execution |
//     mission_task_delegate | mission_flow_run}; dispatch_strategy ∈
//     {resident-lisp | fresh-code-alignment | agent-team | mixed}) are
//     enforced through the wave-21 safety classifier — proposals tagged
//     `unsupported_target` / `invalid_strategy` / `ambiguous_value` are
//     rejected here with a deterministic reason.
//   * `task_contract_path` MUST be supplied AND parse successfully into
//     a `ParsedTaskContract` with `:write-scope` non-empty. The contract
//     is the SSOT for what the spawned task is allowed to touch — no
//     contract ⇒ no spawn.
//   * `:must-not-touch` MUST NOT overlap with `:write-scope`. The spawn
//     refuses to dispatch a contract whose own forbidden scope already
//     intersects its write scope (defensive against malformed contracts).
//   * Caller MUST supply `caller_approved=true` (double opt-in mirroring
//     wave-22 / task 03 / 04). A single accidental flag flip cannot
//     trigger a real spawn.
//   * Caller MUST echo `workstation_proposal_hash` matching the
//     deterministic SHA-256 of the bundle. Hash mismatch / missing
//     fail-fast as a structured error BEFORE the spawn substrate runs.
//   * Caller MUST acknowledge `preflight_status_acceptable=true` —
//     this is the surface where the operator confirms hooks / preflight
//     state is acceptable (the daemon does not run hooks itself; the
//     gate just refuses to spawn without explicit confirmation).
//   * The spawn substrate is ALWAYS `mission_task_delegate` (wave-21
//     conservative target whitelist already restricts the candidates;
//     this layer additionally pins `mission_task_delegate` so the
//     spawn never silently takes a `mission_execution` /
//     `mission_flow_run` proposal). The wave-15 substrate
//     (`run_workstation_dispatch_with_contract`) handles the actual
//     dispatch — we NEVER shell out to `claude -p`, and the gate's
//     own status text pins this invariant verbatim.
//   * If any gate fails ⇒ structured failure (SafeDescriptor-style on
//     the `workstation_auto_spawn_gate` block). NO spawn happens.

/// Structured-error code returned when the caller flips `auto_spawn=true`
/// without echoing `workstation_proposal_hash`. Pinned as a constant so
/// dashboards can grep for the load-bearing failure reason without
/// inspecting the gate block. Mirrors the wave-22 / task 03 pattern
/// (`APPLY_GATE_MISSING_PROPOSAL_HASH`).
pub(crate) const AUTO_SPAWN_MISSING_PROPOSAL_HASH: &str = "AUTO_SPAWN_MISSING_PROPOSAL_HASH";

/// Structured-error code returned when the caller-supplied
/// `workstation_proposal_hash` does not match the bundle's deterministic
/// hash. The strongest "the proposals you saw are not the proposals we
/// have" signal — surfacing it BEFORE the spawn substrate runs is the
/// contract's hard requirement.
pub(crate) const AUTO_SPAWN_PROPOSAL_HASH_MISMATCH: &str = "AUTO_SPAWN_PROPOSAL_HASH_MISMATCH";

/// Structured-error code returned when the caller flips `auto_spawn=true`
/// but supplies a non-bool / non-string shape for the gate args. Caller
/// typos must fail fast so they can never silently degrade to skip.
/// Mirrors `APPLY_GATE_INVALID_PARAM` (wave-22 / task 03).
pub(crate) const AUTO_SPAWN_INVALID_PARAM: &str = "AUTO_SPAWN_INVALID_PARAM";

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

/// Wire status for the deterministic proposal-hash check on the
/// auto-spawn gate. Mirrors wave-22 / task 03's `ProposalHashStatus`
/// shape so dashboards see a uniform vocabulary across the two gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkstationProposalHashStatus {
    /// Caller did not supply `workstation_proposal_hash`. Under
    /// `auto_spawn=true` this collapses to a structured error
    /// (`AUTO_SPAWN_MISSING_PROPOSAL_HASH`) BEFORE the gate runs;
    /// surfaced here for completeness when the gate's preflight is
    /// bypassed (unit tests).
    NotSupplied,
    /// Caller-supplied hash matches the bundle's deterministic hash.
    Matches,
    /// Caller-supplied hash does NOT match. Surfaced as a structured
    /// error (`AUTO_SPAWN_PROPOSAL_HASH_MISMATCH`) BEFORE the gate runs.
    Mismatch,
    /// No bundle / no proposals available — hash check is moot.
    NoProposalAvailable,
}

impl WorkstationProposalHashStatus {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            WorkstationProposalHashStatus::NotSupplied => "not_supplied",
            WorkstationProposalHashStatus::Matches => "matches",
            WorkstationProposalHashStatus::Mismatch => "mismatch",
            WorkstationProposalHashStatus::NoProposalAvailable => "no_proposal_available",
        }
    }
}

/// Caller-supplied opt-in inputs for the wave-22 / task 05 auto-spawn
/// gate. Strict-shape: `auto_spawn` / `caller_approved` /
/// `preflight_status_acceptable` are bool-only (literal strings `"true"`
/// / `"false"` are rejected so a typo cannot silently flip the gate);
/// `workstation_proposal_hash` and `task_contract_path` are string-only.
#[derive(Debug, Clone, Default)]
pub(crate) struct WorkstationAutoSpawnInput {
    /// Caller opted into the gate (`auto_spawn=true`).
    pub auto_spawn: bool,
    /// Caller-supplied SHA-256 hash (32 hex chars) of the bundle they
    /// inspected. Required when `auto_spawn=true`.
    pub proposal_hash: Option<String>,
    /// Caller's second opt-in flag confirming human intent.
    /// Required-truthy when `auto_spawn=true`.
    pub caller_approved: bool,
    /// Caller-supplied `task_contract_path` (relative against the
    /// project root or absolute). Required when `auto_spawn=true`.
    pub task_contract_path: Option<String>,
    /// Caller's acknowledgement that hooks / preflight state is
    /// acceptable. Required-truthy when `auto_spawn=true`. The daemon
    /// does NOT run hooks itself — this is the explicit operator
    /// confirmation surface.
    pub preflight_status_acceptable: bool,
    /// True iff the caller explicitly supplied any of the gate fields
    /// (used to differentiate "caller opted out" from "caller never saw
    /// the knob" so the response stays byte-identical for the latter).
    pub explicit: bool,
}

/// Strict pre-flight validator for the wave-22 / task 05 auto-spawn
/// args. Rejects any non-bool / non-string shape so caller typos fail
/// fast with structured errors. Pure / no I/O. Mirrors
/// `parse_llm_approve_apply_gate_input` (wave-22 / task 03).
pub(crate) fn parse_workstation_auto_spawn_input(
    args: &Value,
) -> std::result::Result<WorkstationAutoSpawnInput, (String, String)> {
    let mut input = WorkstationAutoSpawnInput::default();

    let auto_spawn_v = args.get("auto_spawn");
    let hash_v = args.get("workstation_proposal_hash");
    let caller_v = args.get("workstation_caller_approved");
    let path_v = args.get("task_contract_path");
    let preflight_v = args.get("preflight_status_acceptable");
    input.explicit = auto_spawn_v.is_some()
        || hash_v.is_some()
        || caller_v.is_some()
        || path_v.is_some()
        || preflight_v.is_some();

    if let Some(v) = auto_spawn_v {
        if v.is_null() {
            // null behaves like absent
        } else if let Some(b) = v.as_bool() {
            input.auto_spawn = b;
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "auto_spawn must be a boolean (true|false); got {} \
                     — string `\"true\"` is REJECTED so a typo cannot silently flip the gate",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = hash_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(s) = v.as_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                input.proposal_hash = Some(trimmed.to_string());
            }
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "workstation_proposal_hash must be a string (SHA-256 hex truncated to 32 chars); \
                     got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = caller_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(b) = v.as_bool() {
            input.caller_approved = b;
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "workstation_caller_approved must be a boolean (true|false); got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = path_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(s) = v.as_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                input.task_contract_path = Some(trimmed.to_string());
            }
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "task_contract_path must be a string (relative against project root or absolute); \
                     got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = preflight_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(b) = v.as_bool() {
            input.preflight_status_acceptable = b;
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "preflight_status_acceptable must be a boolean (true|false); got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    Ok(input)
}

/// Pure deterministic SHA-256 hash over the LOAD-BEARING fields of a
/// workstation proposal bundle. Truncated to the leading 32 hex chars
/// (128 bits — way more than enough collision resistance for an
/// audit-trail correlator). Mirrors `compute_proposal_hash` (wave-22 /
/// task 03) for symmetry.
///
/// Inputs (canonical form):
///   * literal `"v1"` schema sentinel,
///   * bundle status wire,
///   * each proposal in its received order:
///       `field|value|confidence|safety_status`,
///     joined by `;`.
///
/// The hash is what the caller is expected to echo back via the
/// `workstation_proposal_hash` arg under `auto_spawn=true`. Caller can
/// derive it themselves from the `workstation_proposals` block — we
/// surface the same value under `workstation_proposal_hash` on the
/// `workstation_auto_spawn_gate` block so dashboards can
/// `assert hash == derive(...)` directly.
pub(crate) fn compute_workstation_proposal_hash(bundle: &WorkstationProposalBundle) -> String {
    use sha2::{Digest, Sha256};
    let proposals: Vec<String> = bundle
        .proposals
        .iter()
        .map(|p| {
            let value_str = p
                .value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| p.value.to_string());
            format!(
                "{}|{}|{}|{}",
                p.field,
                value_str,
                p.confidence.as_wire(),
                p.safety_status.as_wire(),
            )
        })
        .collect();
    let payload = format!("v1|{}|{}", bundle.status.as_wire(), proposals.join(";"));
    let mut h = Sha256::new();
    h.update(payload.as_bytes());
    let full = format!("{:x}", h.finalize());
    full.chars().take(32).collect()
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
