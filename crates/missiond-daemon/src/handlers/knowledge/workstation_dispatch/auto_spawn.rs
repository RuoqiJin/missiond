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

mod gate;
mod hash;
mod input;
mod outcome;

pub(crate) use self::gate::{enforce_auto_spawn_preflight, evaluate_workstation_auto_spawn_gate};
pub(crate) use self::hash::{compute_workstation_proposal_hash, WorkstationProposalHashStatus};
#[cfg(test)]
pub(crate) use self::input::AUTO_SPAWN_INVALID_PARAM;
pub(crate) use self::input::{
    parse_workstation_auto_spawn_input, WorkstationAutoSpawnInput,
    AUTO_SPAWN_MISSING_PROPOSAL_HASH, AUTO_SPAWN_PROPOSAL_HASH_MISMATCH,
};
pub(crate) use self::outcome::{WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnStatus};
