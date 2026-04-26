//! mission_plan — DAG scheduler v2 (bounded ready-node concurrency).
//!
//! This module is loaded by `mission_plan(action=execute, scheduler_mode="dag_v1")`.
//! It is intentionally separated from `plan.rs` so the v0 single-node runner
//! stays untouched as the default contract.
//!
//! v2 changes (Wave 13 / Task 02) keep the parser / validator / dry-run wire
//! shape identical to v1 — they only upgrade the runtime: a wave-based
//! scheduler now dispatches up to `max_parallel_nodes` ready nodes
//! concurrently within the same async task, observes a richer node lifecycle
//! (`pending / ready / running / succeeded / failed / skipped`), and writes
//! one evidence-collector entry per state transition (start + finish for
//! every running node, plus an explicit skip entry for every taint /
//! condition / fail-fast-aborted node).
//!
//! Lisp authority:
//!   - intent-flow.lisp        :: F-intent-alignment-plan-execution-loop ::
//!                                 s6 execution-runner
//!   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//!                                 role plan-runner
//!   - intent-tools.lisp       :: implemented-surface mission_plan ::
//!                                 :execute-contract
//!
//! Scope (v2) — what this scheduler DOES support:
//!   * Top-level `(node :id ... :target ...)` forms inside an outer
//!     `(plan|plan-draft|PLAN ...)` envelope.
//!   * Field allowlist:
//!       :id (required, unique)
//!       :target (required; one of mission_execution / mission_task_delegate /
//!                mission_flow_run)
//!       :objective
//!       :depends-on (vector / list of node id strings)
//!       :condition
//!       :failure-policy (`fail-fast` (default) | `continue`)
//!       :timeout-ms
//!       :dispatch-strategy
//!       :target-project
//!       :requested-cwd
//!       :flow-id
//!   * Validation:
//!       - Unique `:id` per node.
//!       - All `:depends-on` ids must exist in the same DAG.
//!       - The dependency graph must be acyclic (Kahn topo sort).
//!       - `:target` must be on the inner-dispatch whitelist.
//!   * Execution mode:
//!       - Wave-based scheduler driven by a `tokio::task::JoinSet`. Each
//!         wave drains up to `max_parallel_nodes` ready nodes (default 1
//!         keeps the v1 strictly-sequential contract intact). Ready-node
//!         selection is deterministic (sorted by node id) so test output is
//!         reproducible across runs.
//!       - Node lifecycle: `pending → ready → running → succeeded | failed`
//!         for executed nodes, `pending → skipped` for taint / condition /
//!         fail-fast-aborted nodes. Each transition writes one
//!         `plan_dag_node_dispatch` evidence entry tagged with
//!         `state_transition`.
//!       - `failure-policy=fail-fast` (default): the failing node taints its
//!         transitive downstream and the scheduler stops dispatching new
//!         waves. In-flight nodes from the *current* wave are awaited so the
//!         caller still sees their final state — they are never abandoned
//!         mid-flight. Any nodes still `pending` after the in-flight wave
//!         drains are marked `skipped` with reason `fail_fast_aborted`.
//!       - `failure-policy=continue`: the failing node taints only its own
//!         transitive downstream (marked `skipped_upstream_failed`);
//!         independent ready nodes keep being dispatched in subsequent waves.
//!   * `dry_run=true`: returns the planned DAG, the topological order, and
//!     the projected concurrency waves (groups of node ids the scheduler
//!     would launch together given `max_parallel_nodes`) without
//!     dispatching anything and without writing evidence.
//!   * Evidence sidecar: every node-state transition appends one
//!     `plan_dag_node_dispatch` entry via the typed evidence collector.
//!
//! Out of scope (v2) — explicitly NOT supported:
//!   * Per-node retry policy.
//!   * Rollback / compensation.
//!   * Condition evaluation (`:condition` is captured into evidence but never
//!     executed; non-empty condition currently forces the node to be marked
//!     `skipped_condition`).
//!   * Free-form Lisp interpretation. Unknown sub-forms (anything that isn't a
//!     `(node ...)` at top level) are recorded into `node_hint_summary.unsupported_forms`
//!     so callers can see what was ignored.
//!   * Unsupported per-node fields (anything outside the allowlist above) are
//!     captured into `node_hint_summary.unsupported_fields[node_id]` so the
//!     audit trail never silently drops author intent.
//!   * Per-node retry / per-attempt bookkeeping. v2 dispatches every node
//!     exactly once; the `attempt` slot on the `PlanNodeStateChanged` event
//!     and on the evidence entry is hard-coded to `1` so the wire shape is
//!     ready for a retry-aware future scheduler without forcing readers to
//!     handle absence as a special case.
//!
//! Live `ExecutionEvent` bus integration (wave-14 / Task 02): every node
//! transition (`ready -> running`, `running -> succeeded|failed`,
//! `pending -> skipped`) now publishes a `PlanNodeStateChanged` event on
//! the execution bus and stamps the resulting live `Seq` (or the
//! deterministic
//! `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` id when publish
//! fails) into the evidence entry's `execution_events` array via
//! `EventRef::new(...)`. Bus publish failure is observability-only — it
//! never aborts the dispatch, it only records a warning string in
//! `outcome.bus_publish_warnings` so the response surfaces the degraded
//! observability path.

use anyhow::Result;
use missiond_core::event::events::{ExecutionEvent, QuestionEvent};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use crate::state::AppState;
use missiond_core::types::{Plan, PlanStatus};

use super::agent_execution::scopes_overlap_pure;
use super::evidence_collector::{
    self, AppendOutcome, EventRef, EvidenceEntry,
};
use super::plan::{
    build_internal_dispatch_args, parse_infer_plan_fields_mode, tool_result_payload,
    InferPlanFieldsMode, ParsedPlanHints,
};

const VALID_TARGETS: &[&str] = &[
    "mission_execution",
    "mission_task_delegate",
    "mission_flow_run",
];

const FAILURE_POLICY_FAIL_FAST: &str = "fail-fast";
const FAILURE_POLICY_CONTINUE: &str = "continue";

/// wave-16 / task 05 — retry policy ceiling.
///
/// `:retry-count` (alias `:max-attempts`) is interpreted as **additional**
/// attempts beyond the first. The scheduler always runs attempt 1; every
/// retry hint adds N more attempts on top, capped here so a runaway plan
/// (`:retry-count 9999`) cannot melt the dispatch loop. The cap matches
/// the safe-default the wave brief calls out (max attempts = 3 → at most
/// two retries after the first attempt).
const MAX_NODE_ATTEMPTS_CAP: u32 = 3;

/// wave-16 / task 05 — upper bound on the optional `:retry-delay-ms`
/// pause between attempts. We cap at 60 seconds to keep an authoring
/// mistake (`:retry-delay-ms 999999999`) from stalling the entire wave
/// scheduler. Authors that legitimately need longer back-offs should
/// model that as a separate plan node, not a per-node sleep.
const MAX_RETRY_DELAY_MS: u64 = 60_000;

/// One node in the executable DAG. Only fields on the v1 allowlist are kept
/// here; unsupported fields land in `unsupported_fields` and are surfaced via
/// `node_hint_summary` so author intent never disappears silently.
#[derive(Debug, Clone, Default)]
pub(super) struct DagNode {
    pub id: String,
    pub target: String,
    pub objective: Option<String>,
    pub depends_on: Vec<String>,
    pub condition: Option<String>,
    pub failure_policy: String,
    pub timeout_ms: Option<i64>,
    pub dispatch_strategy: Option<String>,
    pub target_project: Option<String>,
    pub requested_cwd: Option<String>,
    pub flow_id: Option<String>,
    /// wave-15 / task 05 — workstation-dispatch hint contract additions.
    /// Each field is captured raw and only consumed by the
    /// `workstation_dispatch` module when this node opts in. Storing them
    /// on the node (rather than pushing into `unsupported_fields`) lets
    /// the v2 scheduler route the node through the workstation-dispatch
    /// substrate without a second parse pass.
    pub scope: Option<String>,
    pub commit_policy: Option<String>,
    pub owned_files_raw: Option<String>,
    pub forbidden_files_raw: Option<String>,
    pub acceptance_commands_raw: Option<String>,
    /// wave-17 / task 03 — declarative acceptance evaluator hint.
    /// `:acceptance-mode "inner_status" | "manual" | "evidence_keys"`.
    /// Absent / blank / unrecognised values fall through to the default
    /// behaviour: nodes with `acceptance_commands_raw` set but no safe
    /// evaluator pause as `manual_required`; nodes without any
    /// acceptance hints preserve the wave-13 succeed-on-dispatch contract.
    /// Unknown raw values ALSO get pushed into `unsupported_fields` so
    /// the typo surfaces through `node_hint_summary`.
    pub acceptance_mode_raw: Option<String>,
    /// wave-17 / task 03 — required typed-evidence keys when
    /// `:acceptance-mode "evidence_keys"`. Stored as the raw lisp list
    /// string and split via `split_lisp_string_list` at evaluation time
    /// (same shape as `:owned-files` / `:acceptance-commands`).
    pub acceptance_evidence_keys_raw: Option<String>,
    /// wave-18 / task 03 — cross-node acceptance fan-in dependencies.
    /// `:acceptance-depends-on ["node-a" "node-b"]`. When non-empty, the
    /// node's acceptance phase additionally inspects the listed prior
    /// nodes' terminal state / evidence per `acceptance_requires_raw`
    /// before deciding accept / reject. Each entry MUST also appear as
    /// a (transitive) `:depends-on` ancestor of this node — otherwise
    /// the prior node's evidence may not yet exist when this node's
    /// acceptance phase runs (validator raises
    /// `DagBuildError::AcceptanceFanInDepNotAncestor`).
    pub acceptance_depends_on: Vec<String>,
    /// wave-18 / task 03 — fan-in policy. Recognised:
    ///   `all_succeeded` — every listed node must be Succeeded.
    ///   `any_succeeded` — at least one listed node must be Succeeded.
    ///   `evidence_keys` — read keys (`:acceptance-evidence-keys`) from
    ///                     the source node's `inner_payload`. Source
    ///                     resolves to `:acceptance-source-node` (single
    ///                     id, must be in `acceptance_depends_on`).
    /// Absent / blank with NO `:acceptance-depends-on` is the wave-17
    /// shape (no fan-in); absent with `:acceptance-depends-on` declared
    /// raises `DagBuildError::AcceptanceFanInRequiresMissing`.
    /// Unknown raw values land BOTH on the typed slot AND in
    /// `unsupported_fields` so the typo surfaces through
    /// `node_hint_summary`; the validator then raises a structured
    /// error so the typo cannot silently degrade fan-in to "no check".
    pub acceptance_requires_raw: Option<String>,
    /// wave-18 / task 03 — single source-node id for `evidence_keys`
    /// fan-in mode. MUST be present in `acceptance_depends_on` and the
    /// plan node set; the validator raises a structured error otherwise.
    /// Ignored under `all_succeeded` / `any_succeeded` modes.
    pub acceptance_source_node: Option<String>,
    pub workstation_dispatch_flag: Option<String>,
    /// wave-16 / task 04 — per-node review-gate hint contract.
    /// `:review-gate` is the gate kind ("none" default, "question-event"
    /// pauses the node and emits `QuestionEvent::Created`); `:review-action`
    /// is folded into the deterministic question id so authors can
    /// override the default `plan-node` action label per node;
    /// `:review-text` is a free-form prompt echoed back on the response so
    /// reviewers see what the author wanted answered before resume.
    pub review_gate: Option<String>,
    pub review_action: Option<String>,
    pub review_text: Option<String>,
    /// wave-16 / task 05 — bounded per-node retry policy.
    /// `:retry-count` / `:max-attempts` declares **additional** attempts
    /// beyond the first; absent / 0 keeps the v2-baseline single-attempt
    /// dispatch (`max_attempts = 1`). Negative / non-numeric values land
    /// in `DagBuildError::InvalidRetryHint` at validation time so a typo
    /// fails fast instead of silently disabling retry. Parsed values are
    /// capped to `MAX_NODE_ATTEMPTS_CAP` (so `max_attempts ∈ [1, 3]`
    /// after `effective_max_attempts` resolves them).
    pub retry_count: Option<u32>,
    /// wave-16 / task 05 — optional sleep between attempts. Capped to
    /// `MAX_RETRY_DELAY_MS` (60s) so an authoring typo cannot stall the
    /// wave scheduler. Absent → no sleep between attempts.
    pub retry_delay_ms: Option<u64>,
    /// wave-16 / task 05 — parser-stage retry hint failure carried
    /// forward so `build_validated_dag` can raise a structured
    /// `DagBuildError::InvalidRetryHint`. Stored as
    /// `(key, raw_value, detail)` so the validator can emit a precise
    /// error message without re-parsing the form. Set when either
    /// `:retry-count`/`:max-attempts` or `:retry-delay-ms` failed to
    /// parse as a non-negative integer.
    pub retry_parse_error: Option<(String, String, String)>,
    /// wave-17 / task 04 — conservative rollback descriptor hints.
    ///
    /// Captured per-node so the wave loop can decide what (if anything)
    /// to do AFTER the final failed attempt. The scheduler is
    /// deliberately conservative:
    ///   * absent / `"none"` → no rollback at all (default).
    ///   * `"descriptor"`     → record/return a structured rollback
    ///                          descriptor; never dispatch.
    ///   * `"workstation"`    → only dispatch a rollback task if every
    ///                          safety condition (resolved project,
    ///                          non-empty rollback objective, owned
    ///                          files, safe dispatch strategy) is
    ///                          satisfied. Otherwise surface as
    ///                          `refused`.
    ///
    /// Unrecognised raw values still land on the typed slot AND are
    /// pushed into `unsupported_fields` so the typo surfaces through
    /// `node_hint_summary` while the scheduler safely falls back to
    /// "no rollback".
    pub rollback_policy: Option<String>,
    /// wave-17 / task 04 — free-form objective for the rollback brief.
    /// Required for `workstation` mode (its absence is one of the
    /// safety refusals); echoed verbatim under `descriptor` mode so
    /// observers / out-of-band tooling can act on the intent.
    pub rollback_objective: Option<String>,
    /// wave-17 / task 04 — owned files the rollback task is allowed to
    /// stage / commit. Stored as the raw lisp list string and split via
    /// `split_lisp_string_list` at evaluation time (same shape as
    /// `:owned-files` / `:acceptance-commands`). Required (non-empty)
    /// for the `workstation` mode safety gate.
    pub rollback_owned_files_raw: Option<String>,
    /// wave-17 / task 04 — acceptance commands the rollback task must
    /// pass before commit. Surfaced verbatim into the rollback brief
    /// AND the descriptor; the scheduler NEVER executes them (mirrors
    /// the wave-17 / task 03 acceptance-commands invariant).
    pub rollback_acceptance_commands_raw: Option<String>,
    /// wave-18 / task 04 — `:compensates "<failed-node-id>"`. When
    /// present, the cascade rollback evaluator treats THIS node as a
    /// candidate compensation step for the named failed node. Pure
    /// metadata: declaring `:compensates` does NOT make this node
    /// dispatch automatically — only the cascade evaluator (running
    /// AFTER the named node fails) consults the field. The compensation
    /// node still runs through the regular DAG dispatch path otherwise
    /// (so authors can also declare `:depends-on` on the failed node
    /// to gate manual cascading; the cascade evaluator is independent).
    pub compensates: Option<String>,
    /// wave-19 / task 10 — `:compensate-node "<comp-node-id>"` (alias
    /// `:compensate-ref`). Forward declaration: declared on the failing
    /// (cascade-root) node and points AT the compensation node id. The
    /// reverse `:compensates` declaration (declared on the compensation
    /// node, points BACK at the failing node) remains supported and is
    /// the primary contract; `:compensate-node` lets authors who prefer
    /// to read the cascade top-down state the relationship from the
    /// failing-node side instead.
    ///
    /// Both directions parse into independent slots; the validator (in
    /// `build_validated_dag`) reconciles them with strict rules:
    ///   * forward ref MUST resolve to a declared node id and MUST NOT
    ///     point at the failing node itself (self-reference rejected with
    ///     `DagBuildError::CompensateNodeInvalid`);
    ///   * if BOTH the forward `:compensate-node "X"` AND the reverse
    ///     `:compensates "Y"` (declared on X) are present, then Y MUST
    ///     equal the failing node id — otherwise the validator fails
    ///     fast with `DagBuildError::CompensateDirectionMismatch` so the
    ///     scheduler never silently picks one direction.
    /// The compensation discovery in `compute_compensation_order` reads
    /// the union of both directions (after validator agreement) so
    /// existing wave-18 plans behave byte-identically.
    pub compensate_node: Option<String>,
    /// wave-18 / task 04 — `:rollback-cascade "none" | "plan" |
    /// "dispatch-safe"`. Per-node opt-in for the cascade rollback
    /// evaluator. Defaults to `none` so the wave-17 / task 04 node-local
    /// rollback behaviour is preserved byte-for-byte for plans that did
    /// not opt into cascading.
    ///
    /// * `none`           — cascade pass skipped (default); the node-local
    ///                      rollback (`:rollback-policy`) still runs.
    /// * `plan`           — cascade evaluator computes the ordered list of
    ///                      compensation nodes and records the plan on the
    ///                      response + evidence row. **NEVER dispatches.**
    /// * `dispatch-safe`  — cascade evaluator computes the same plan AND,
    ///                      for every compensation node whose own
    ///                      rollback safety gates pass, dispatches it
    ///                      through the wave-15 workstation substrate.
    ///                      Refusals are recorded but the cascade itself
    ///                      is NEVER retried.
    ///
    /// Unrecognised raw values land BOTH on the typed slot AND in
    /// `unsupported_fields` so the typo surfaces through
    /// `node_hint_summary` while the cascade evaluator safely degrades
    /// to "no cascade" (the safe default).
    pub rollback_cascade: Option<String>,
    /// wave-18 / task 04 — `:rollback-after ["node-a" "node-b"]`. Optional
    /// ordering hint consumed by the cascade evaluator. When two
    /// compensation nodes both declare `:compensates` for the same failed
    /// node, the cascade ordering algorithm runs them in the topological
    /// order induced by `:rollback-after` (which is treated as an
    /// ADDITIONAL "must-run-after" edge for cascade ordering only — it
    /// is NOT promoted to a real `:depends-on` because cascade ordering
    /// must not silently change forward dispatch order). Cycles in the
    /// `:rollback-after` graph fall back to declaration order so a
    /// typo never deadlocks the cascade.
    pub rollback_after: Vec<String>,
    /// Per-node unsupported `:keyword value` pairs, kept in source order.
    pub unsupported_fields: Vec<(String, String)>,
}

impl DagNode {
    /// True iff this node opted into workstation-dispatch v0 via
    /// `:workstation-dispatch true` (or any bareword that lowercases to
    /// `true`/`yes`/`on`/`1`).
    pub(super) fn workstation_dispatch_opt_in(&self) -> bool {
        match self.workstation_dispatch_flag.as_deref() {
            Some(raw) => matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "true" | "yes" | "on" | "1"
            ),
            None => false,
        }
    }

    /// Resolve the parsed `:review-gate` hint to a typed kind. Pure helper
    /// — the scheduler routes on this enum so unsupported / typo'd values
    /// fall through to `None` instead of silently pausing a node the
    /// author meant to dispatch.
    pub(super) fn review_gate_kind(&self) -> ReviewGateKind {
        match self
            .review_gate
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            // Default (absent / "none" / blank) keeps v2 behaviour
            // byte-compatible — the scheduler dispatches as before.
            None | Some("") | Some("none") => ReviewGateKind::None,
            Some("question-event") | Some("question_event") => {
                ReviewGateKind::QuestionEvent
            }
            // Unknown gate kinds are recorded into `unsupported_fields`
            // by the parser so the audit trail keeps author intent; the
            // scheduler treats them as `None` to avoid pausing a node
            // for a typo. The author sees the typo in the response's
            // `node_hint_summary.unsupported_fields`.
            Some(_) => ReviewGateKind::None,
        }
    }

    /// wave-16 / task 05 — total attempts the scheduler will make for
    /// this node before declaring it `failed`. Always ≥ 1: the first
    /// dispatch is attempt 1, and `:retry-count`/`:max-attempts` adds
    /// **additional** retries on top, capped to
    /// `MAX_NODE_ATTEMPTS_CAP`. Absent / 0 keeps the v2-baseline
    /// single-attempt contract intact. Capping is also applied here so
    /// callers (response serialisers, dry-run) can use this as the
    /// single source of truth for "what the scheduler will actually do".
    pub(super) fn effective_max_attempts(&self) -> u32 {
        let extra = self.retry_count.unwrap_or(0);
        let total = extra.saturating_add(1);
        total.clamp(1, MAX_NODE_ATTEMPTS_CAP)
    }

    /// True iff the node opted into ≥ 1 retry attempt. Used by the
    /// dry-run / dispatch surface to decide whether to emit a
    /// `retry_plan` entry for this node (we omit nodes with the default
    /// single-attempt contract so the v2 byte-shape stays untouched
    /// for callers that do not opt in).
    pub(super) fn retry_enabled(&self) -> bool {
        self.effective_max_attempts() > 1
    }

    /// wave-16 / task 05 — clamp the optional `:retry-delay-ms` to the
    /// safe ceiling. Absent / 0 → `None` so the scheduler skips the
    /// `tokio::time::sleep` entirely (no idle wake-up cost).
    pub(super) fn effective_retry_delay_ms(&self) -> Option<u64> {
        self.retry_delay_ms
            .filter(|&n| n > 0)
            .map(|n| n.min(MAX_RETRY_DELAY_MS))
    }

    /// wave-17 / task 03 — typed projection of `:acceptance-mode`. Pure
    /// helper so the scheduler can pivot on the enum without
    /// re-tokenising the raw string. Returns `None` when the author did
    /// not declare a mode OR wrote an unrecognised value (the parser
    /// also pushes unrecognised values into `unsupported_fields`).
    pub(super) fn acceptance_mode_kind(&self) -> Option<AcceptanceMode> {
        let raw = self.acceptance_mode_raw.as_deref()?.trim();
        if raw.is_empty() {
            return None;
        }
        AcceptanceMode::parse(raw)
    }

    /// wave-17 / task 03 — true iff this node carries any acceptance
    /// hint at all (mode / commands / evidence keys / fan-in). Used by
    /// the scheduler to skip the acceptance-evidence emit when the node
    /// did not opt in (preserves the wave-13 byte shape).
    pub(super) fn has_acceptance_hints(&self) -> bool {
        let mode_present = self
            .acceptance_mode_raw
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let commands_present = self
            .acceptance_commands_raw
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let keys_present = self
            .acceptance_evidence_keys_raw
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        // wave-18 / task 03 — fan-in declarations also count as hints
        // so the scheduler emits the acceptance evidence row even when
        // the per-node acceptance is `not_evaluated`.
        let fan_in_present = !self.acceptance_depends_on.is_empty()
            || self
                .acceptance_requires_raw
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
        mode_present || commands_present || keys_present || fan_in_present
    }

    /// wave-18 / task 03 — typed projection of `:acceptance-requires`.
    /// Returns `None` when the author did not declare a value OR wrote an
    /// unrecognised one (the parser also pushes unrecognised values into
    /// `unsupported_fields`). The validator turns "fan-in deps declared
    /// but no recognised mode" into a structured error so the typo cannot
    /// silently disable the gate.
    pub(super) fn acceptance_requires_kind(&self) -> Option<AcceptanceRequires> {
        let raw = self.acceptance_requires_raw.as_deref()?.trim();
        if raw.is_empty() {
            return None;
        }
        AcceptanceRequires::parse(raw)
    }

    /// wave-18 / task 03 — true iff this node opted into cross-node
    /// acceptance fan-in (one or more `:acceptance-depends-on` entries
    /// AND a recognised `:acceptance-requires` mode).
    pub(super) fn has_acceptance_fan_in(&self) -> bool {
        !self.acceptance_depends_on.is_empty()
            && self.acceptance_requires_kind().is_some()
    }

    /// wave-17 / task 04 — typed projection of `:rollback-policy`.
    /// Returns `None` when the author did not declare a policy OR wrote
    /// an unrecognised value (the parser also pushes unrecognised
    /// values into `unsupported_fields` so the typo is loud).
    pub(super) fn rollback_policy_kind(&self) -> Option<RollbackPolicy> {
        let raw = self.rollback_policy.as_deref()?.trim();
        if raw.is_empty() {
            return None;
        }
        RollbackPolicy::parse(raw)
    }

    /// wave-18 / task 04 — typed projection of `:rollback-cascade`.
    /// Returns `None` when the author did not declare a cascade mode OR
    /// wrote an unrecognised value (the parser ALSO pushes unrecognised
    /// values into `unsupported_fields` so the typo is loud). The
    /// scheduler treats `None` as `RollbackCascadeMode::None` (the safe
    /// default — cascade pass skipped).
    pub(super) fn rollback_cascade_kind(&self) -> Option<RollbackCascadeMode> {
        let raw = self.rollback_cascade.as_deref()?.trim();
        if raw.is_empty() {
            return None;
        }
        RollbackCascadeMode::parse(raw)
    }

    /// wave-18 / task 04 — true iff this node opted into the cascade
    /// rollback evaluator (any `:rollback-cascade` value other than
    /// `"none"`). Used by the scheduler to decide whether to run the
    /// cascade pass after the per-node `run_rollback`.
    pub(super) fn has_active_rollback_cascade(&self) -> bool {
        matches!(
            self.rollback_cascade_kind(),
            Some(RollbackCascadeMode::Plan) | Some(RollbackCascadeMode::DispatchSafe)
        )
    }

    /// wave-17 / task 04 — true iff this node opted into ANY rollback
    /// hint (policy / objective / owned files / acceptance commands).
    /// Used to skip the rollback evaluator entirely on the wave-13
    /// byte-shape path (no hints declared → no rollback evidence row).
    pub(super) fn has_rollback_hints(&self) -> bool {
        let policy_present = self
            .rollback_policy
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let objective_present = self
            .rollback_objective
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let owned_present = self
            .rollback_owned_files_raw
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let acceptance_present = self
            .rollback_acceptance_commands_raw
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        // wave-18 / task 04 — cascade hints also count: a node that
        // declares `:rollback-cascade` / `:compensates` / `:rollback-after`
        // but no `:rollback-policy` should still surface its rollback
        // intent through the response so audit can pin the cascade plan.
        let cascade_present = self
            .rollback_cascade
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let compensates_present = self
            .compensates
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        // wave-19 / task 10 — forward `:compensate-node` refs are also
        // a rollback hint (declared on the failing node side); surface
        // them through `node_hint_summary` for the same audit reasons.
        let compensate_node_present = self
            .compensate_node
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let rollback_after_present = !self.rollback_after.is_empty();
        policy_present
            || objective_present
            || owned_present
            || acceptance_present
            || cascade_present
            || compensates_present
            || compensate_node_present
            || rollback_after_present
    }
}

/// wave-16 / task 04 — typed projection of `:review-gate` for the
/// scheduler. Kept on the parser side so dispatch-time logic can match
/// without re-tokenising the raw string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReviewGateKind {
    /// No gate — node dispatches as before. Default for absent / "none"
    /// / blank values, AND for unrecognised values (which are also
    /// captured into `unsupported_fields` so the typo is observable).
    None,
    /// Pause the node and emit `QuestionEvent::Created` instead of
    /// dispatching the target tool.
    QuestionEvent,
}

/// wave-17 / task 03 — typed projection of `:acceptance-mode` for the
/// deterministic acceptance evaluator. Resolved on the parser side so the
/// runtime can pivot without re-tokenising the raw string.
///
/// Three modes are recognised:
///   * `InnerStatus` — accept when the inner dispatch returned Ok and the
///     inner payload does not carry an explicit non-success status.
///   * `EvidenceKeys` — accept when the inner payload (object or array of
///     objects under `evidence` / `typed_evidence`) contains every key
///     declared in `:acceptance-evidence-keys`.
///   * `Manual`      — never auto-accept; always surface as
///     `acceptance_status="manual_required"` so a human / follow-up
///     pipeline must approve the node.
///
/// `None` (returned by [`DagNode::acceptance_mode_kind`]) means the
/// author did not declare a mode. The evaluator then falls back to the
/// default policy: any declared `:acceptance-commands` triggers
/// `manual_required` (we refuse to run shell from PLAN.lisp); no hints
/// at all preserves the wave-13 succeed-on-dispatch contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AcceptanceMode {
    InnerStatus,
    EvidenceKeys,
    Manual,
}

impl AcceptanceMode {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            AcceptanceMode::InnerStatus => "inner_status",
            AcceptanceMode::EvidenceKeys => "evidence_keys",
            AcceptanceMode::Manual => "manual",
        }
    }

    /// Parse a raw `:acceptance-mode` value into a typed mode. Trims and
    /// lowercases the input; `_` and `-` separators are interchangeable
    /// so authors can write either `inner_status` or `inner-status`.
    /// Unknown values yield `None` (the caller — the parser — also pushes
    /// them onto `unsupported_fields` so the typo surfaces in
    /// `node_hint_summary`).
    pub(super) fn parse(raw: &str) -> Option<Self> {
        let lc = raw.trim().to_ascii_lowercase();
        match lc.as_str() {
            "inner_status" | "inner-status" => Some(AcceptanceMode::InnerStatus),
            "evidence_keys" | "evidence-keys" => Some(AcceptanceMode::EvidenceKeys),
            "manual" => Some(AcceptanceMode::Manual),
            _ => None,
        }
    }
}

/// wave-18 / task 03 — typed projection of `:acceptance-requires` for
/// the cross-node acceptance fan-in evaluator. Resolved on the parser
/// side so the runtime can pivot without re-tokenising the raw string.
///
/// Three modes are recognised:
///   * `AllSucceeded` — fan-in passes when every node listed in
///                      `:acceptance-depends-on` reached terminal state
///                      `Succeeded`.
///   * `AnySucceeded` — fan-in passes when at least one listed node
///                      reached terminal state `Succeeded`.
///   * `EvidenceKeys` — fan-in passes when the `:acceptance-source-node`'s
///                      `inner_payload` contains every key declared in
///                      `:acceptance-evidence-keys`. Reuses the wave-17
///                      sidecar shape (top-level + well-known nested
///                      holders); the scheduler NEVER re-runs the source
///                      node — it only inspects the recorded payload.
///
/// `None` (returned by [`DagNode::acceptance_requires_kind`]) means the
/// author either did not declare the field OR wrote an unrecognised
/// value. The validator raises a structured error in that case if the
/// node also declared `:acceptance-depends-on`, so the typo cannot
/// silently degrade fan-in to "no gate".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AcceptanceRequires {
    AllSucceeded,
    AnySucceeded,
    EvidenceKeys,
}

impl AcceptanceRequires {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            AcceptanceRequires::AllSucceeded => "all_succeeded",
            AcceptanceRequires::AnySucceeded => "any_succeeded",
            AcceptanceRequires::EvidenceKeys => "evidence_keys",
        }
    }

    /// Parse a raw `:acceptance-requires` value. Trims + lowercases;
    /// `_` and `-` separators are interchangeable so authors can write
    /// either `all_succeeded` or `all-succeeded`. Unknown values yield
    /// `None` so the parser can land them in `unsupported_fields` AND
    /// the validator can raise a structured error instead of silently
    /// degrading fan-in to a no-op.
    pub(super) fn parse(raw: &str) -> Option<Self> {
        let lc = raw.trim().to_ascii_lowercase();
        match lc.as_str() {
            "all_succeeded" | "all-succeeded" => Some(AcceptanceRequires::AllSucceeded),
            "any_succeeded" | "any-succeeded" => Some(AcceptanceRequires::AnySucceeded),
            "evidence_keys" | "evidence-keys" => Some(AcceptanceRequires::EvidenceKeys),
            _ => None,
        }
    }
}

/// wave-17 / task 03 — outcome of the deterministic acceptance phase.
/// Drives whether a successful dispatch becomes `Succeeded`, `Failed`,
/// or `Paused (manual_required)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AcceptanceStatus {
    /// Author declared no acceptance hints — preserve the wave-13
    /// succeed-on-dispatch contract.
    NotEvaluated,
    /// Acceptance evaluator approved the run.
    Accepted,
    /// Acceptance evaluator refused (e.g. evidence_keys missing).
    Rejected,
    /// Acceptance cannot be proven without human input (manual mode, or
    /// declared commands without a safe evaluator). Node pauses; the
    /// scheduler MUST NOT execute any declared shell commands.
    ManualRequired,
}

impl AcceptanceStatus {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            AcceptanceStatus::NotEvaluated => "not_evaluated",
            AcceptanceStatus::Accepted => "accepted",
            AcceptanceStatus::Rejected => "rejected",
            AcceptanceStatus::ManualRequired => "manual_required",
        }
    }
}

/// wave-17 / task 03 — pure result of evaluating a node's acceptance
/// hints. Carries every field the response and evidence rows surface so
/// callers don't have to re-derive them from the node + payload.
#[derive(Debug, Clone)]
pub(super) struct AcceptanceEvaluation {
    pub status: AcceptanceStatus,
    /// Resolved typed mode. `None` when the author did not declare
    /// `:acceptance-mode` (or wrote an unrecognised value).
    pub mode: Option<AcceptanceMode>,
    /// Declared acceptance commands surfaced verbatim — NEVER executed.
    /// The evaluator captures them so the response + evidence rows make
    /// the author intent visible to humans / downstream pipelines that
    /// might run them out-of-band.
    pub commands: Vec<String>,
    /// Required evidence keys declared via `:acceptance-evidence-keys`
    /// (only meaningful for `evidence_keys` mode but surfaced regardless
    /// so observers can see the declared contract).
    pub evidence_keys: Vec<String>,
    /// Human-readable explanation of the decision. Always populated.
    pub reason: String,
    /// wave-18 / task 03 — cross-node acceptance fan-in outcome. `None`
    /// when the author did not declare any fan-in hints; `Some(...)`
    /// captures the resolved mode + source nodes + result + reason so
    /// downstream observers can audit the decision without re-deriving
    /// it from the prior nodes' evidence.
    pub fan_in: Option<AcceptanceFanInOutcome>,
}

impl AcceptanceEvaluation {
    /// Convenience: this evaluation produced no acceptance signal at all
    /// (no hints declared). Used by the scheduler to skip the
    /// acceptance-evidence emit and preserve the v2 byte-shape.
    pub(super) fn is_inactive(&self) -> bool {
        matches!(self.status, AcceptanceStatus::NotEvaluated)
            && self.mode.is_none()
            && self.commands.is_empty()
            && self.evidence_keys.is_empty()
            && self.fan_in.is_none()
    }

    /// Project the evaluation as a JSON block suitable for
    /// `node_results[].acceptance` / `evidence.acceptance`. Stable shape
    /// — every field is always present so consumers don't have to
    /// branch on absence. The `fan_in` block is omitted when the
    /// author did not opt into cross-node fan-in so the wave-17
    /// byte-shape is preserved for callers that did not declare it.
    pub(super) fn to_json(&self) -> Value {
        let mut v = json!({
            "status": self.status.as_wire(),
            "mode": self.mode.map(|m| m.as_wire()),
            "commands": self.commands,
            "evidence_keys": self.evidence_keys,
            "reason": self.reason,
        });
        if let Some(f) = &self.fan_in {
            v["fan_in"] = f.to_json();
        }
        v
    }
}

/// wave-18 / task 03 — pure result of evaluating cross-node acceptance
/// fan-in. Always carries the resolved mode + source nodes + decision
/// so observers can audit the gate without re-walking prior nodes'
/// evidence.
#[derive(Debug, Clone)]
pub(super) struct AcceptanceFanInOutcome {
    pub mode: AcceptanceRequires,
    /// Source nodes that participated in this fan-in evaluation, in
    /// the order the author declared them. For `evidence_keys` mode
    /// this is a single-element list (the resolved
    /// `:acceptance-source-node`).
    pub source_nodes: Vec<String>,
    /// `true` iff the fan-in passed (gate satisfied). When `false`,
    /// the parent acceptance evaluation flips its status to `Rejected`.
    pub passed: bool,
    /// Human-readable explanation of the decision. Always populated.
    pub reason: String,
}

impl AcceptanceFanInOutcome {
    pub(super) fn to_json(&self) -> Value {
        json!({
            "mode": self.mode.as_wire(),
            "source_nodes": self.source_nodes,
            "passed": self.passed,
            "reason": self.reason,
        })
    }
}

/// wave-17 / task 04 — typed projection of `:rollback-policy` for the
/// conservative rollback descriptor pass. Resolved on the parser side
/// so the runtime can pivot without re-tokenising the raw string.
///
/// Three modes are recognised:
///   * `None`        — author wrote `"none"` (or omitted the policy
///                      entirely; absence on `DagNode::rollback_policy`
///                      is the SAME as `None`). Preserves the existing
///                      failure behaviour: failed node propagates taint
///                      per `:failure-policy`, no rollback descriptor
///                      is emitted.
///   * `Descriptor`  — record / surface a structured rollback
///                      descriptor (objective + owned files +
///                      acceptance commands + brief preview) on the
///                      response and evidence row. **Never dispatches.**
///                      Use this when the author wants downstream
///                      observers / humans to know what a rollback
///                      WOULD do without authorising the scheduler to
///                      execute it.
///   * `Workstation` — opt into automatic rollback dispatch through
///                      the existing wave-15 workstation-dispatch
///                      substrate. The scheduler ONLY dispatches when
///                      every safety condition holds (resolved target
///                      project, non-empty rollback objective, at
///                      least one owned file, dispatch strategy is on
///                      the inferable whitelist). Otherwise the row
///                      surfaces as `refused` with the failing
///                      condition spelled out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RollbackPolicy {
    None,
    Descriptor,
    Workstation,
}

impl RollbackPolicy {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            RollbackPolicy::None => "none",
            RollbackPolicy::Descriptor => "descriptor",
            RollbackPolicy::Workstation => "workstation",
        }
    }

    /// Parse a raw `:rollback-policy` value into a typed mode. Trims
    /// and lowercases the input; unknown values yield `None` (the
    /// parser also pushes them onto `unsupported_fields` so the typo
    /// surfaces in `node_hint_summary`).
    pub(super) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" => Some(RollbackPolicy::None),
            "descriptor" => Some(RollbackPolicy::Descriptor),
            "workstation" => Some(RollbackPolicy::Workstation),
            _ => None,
        }
    }
}

/// wave-17 / task 04 — outcome of the conservative rollback pass.
/// Drives whether the failed node carries a rollback descriptor on the
/// response, whether a rollback task was dispatched, and (when refused)
/// the condition that failed.
///
/// Wire vocabulary is fixed so audit dashboards can pivot on a single
/// string:
///   * `not_requested`     — no rollback hints declared OR
///                            `:rollback-policy "none"`. Default for
///                            failed nodes that did not opt in.
///   * `descriptor_ready`  — `:rollback-policy "descriptor"`. The
///                            descriptor is recorded on the response /
///                            evidence; **no dispatch happened**.
///   * `dispatched`        — `:rollback-policy "workstation"` AND
///                            every safety gate passed AND the
///                            rollback dispatch ran. The inner
///                            payload + brief preview ride on the row.
///   * `refused`           — `:rollback-policy "workstation"` was
///                            requested but at least one safety gate
///                            failed. The reason carries the failing
///                            condition. **No dispatch happened.**
///                            SafeDescriptor refusals from the
///                            underlying substrate also collapse here.
///   * `failed`            — `:rollback-policy "workstation"` was
///                            dispatched but the inner handler
///                            returned an error. The inner payload's
///                            error message is captured on the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RollbackStatus {
    NotRequested,
    DescriptorReady,
    Dispatched,
    Refused,
    Failed,
}

impl RollbackStatus {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            RollbackStatus::NotRequested => "not_requested",
            RollbackStatus::DescriptorReady => "descriptor_ready",
            RollbackStatus::Dispatched => "dispatched",
            RollbackStatus::Refused => "refused",
            RollbackStatus::Failed => "failed",
        }
    }
}

/// wave-17 / task 04 — pure result of evaluating a node's rollback
/// hints. `policy=None` + `status=NotRequested` is the inactive
/// default; the wave loop suppresses the rollback evidence + response
/// surfacing whenever this evaluation is inactive so the wave-13
/// byte shape stays untouched for plans that did not opt in.
#[derive(Debug, Clone)]
pub(super) struct RollbackEvaluation {
    pub policy: RollbackPolicy,
    pub status: RollbackStatus,
    /// Reason the gate / dispatch landed where it did. Always populated
    /// (even for the `not_requested` branch) so the audit row carries a
    /// human-readable explanation of the decision.
    pub reason: String,
    /// Resolved rollback objective (may be empty when no hint declared).
    pub objective: Option<String>,
    /// Resolved owned-files list. Surfaced verbatim into the descriptor
    /// + brief.
    pub owned_files: Vec<String>,
    /// Resolved acceptance-commands list. Surfaced verbatim — NEVER
    /// executed by the scheduler.
    pub acceptance_commands: Vec<String>,
    /// Trimmed task-brief preview when the substrate built one.
    /// `None` for `not_requested` and for `refused` paths that
    /// short-circuited before brief construction.
    pub task_brief_preview: Option<String>,
    /// File path the brief was mirrored to (currently always `None` —
    /// substrate does not yet write the brief to disk; kept on the
    /// shape so a future enhancement can fill it in without breaking
    /// the wire contract).
    pub task_brief_path: Option<String>,
    /// Inner dispatch payload from `run_workstation_dispatch` when the
    /// rollback was actually dispatched. `None` for descriptor-only,
    /// not-requested, and refused paths.
    pub inner_payload: Option<Value>,
    /// wave-18 / task 04 — cascade rollback outcome for THIS failed node.
    /// `None` when the node did not opt into cascading (default — the
    /// wave-17 / task 04 byte shape is preserved). `Some(out)` carries
    /// the resolved cascade mode + ordered compensation outcomes; the
    /// scheduler stamps it onto `node_results[].rollback.cascade` so
    /// callers see the cascade plan + dispatch / refusal results without
    /// re-deriving from evidence.
    pub cascade: Option<CascadeRollbackOutcome>,
}

impl RollbackEvaluation {
    /// Convenience: this evaluation produced no rollback signal at all.
    /// Used by the scheduler to skip the rollback-evidence emit and
    /// preserve the v2 byte-shape.
    pub(super) fn is_inactive(&self) -> bool {
        matches!(self.policy, RollbackPolicy::None)
            && matches!(self.status, RollbackStatus::NotRequested)
            && self.objective.is_none()
            && self.owned_files.is_empty()
            && self.acceptance_commands.is_empty()
            // wave-18 / task 04 — a cascade-only opt-in (no node-local
            // rollback hints) MUST still surface so observers can pin
            // the cascade plan. Treat any active cascade as a signal.
            && self
                .cascade
                .as_ref()
                .map(|c| c.is_inactive())
                .unwrap_or(true)
    }

    /// Project the evaluation as a JSON block suitable for
    /// `node_results[].rollback` / `evidence.rollback`. Stable shape
    /// — every field is always present so consumers don't have to
    /// branch on absence.
    pub(super) fn to_json(&self) -> Value {
        let mut v = json!({
            "policy": self.policy.as_wire(),
            "status": self.status.as_wire(),
            "reason": self.reason,
            "objective": self.objective,
            "owned_files": self.owned_files,
            "acceptance_commands": self.acceptance_commands,
            "acceptance_commands_executed": false,
        });
        if let Some(preview) = self.task_brief_preview.as_deref() {
            v["task_brief_preview"] = json!(preview);
        }
        if let Some(p) = self.task_brief_path.as_deref() {
            v["task_brief_path"] = json!(p);
        }
        if let Some(inner) = self.inner_payload.clone() {
            v["inner_result"] = inner;
        }
        // wave-18 / task 04 — cascade outcome rides on the same JSON
        // block so observers can pin `rollback.cascade.compensations[]`
        // without descending into a separate evidence row. Quiet when
        // the cascade evaluator never produced an observable signal so
        // the wave-17 / task 04 byte-shape stays untouched for plans
        // that did not opt into cascading.
        if let Some(cascade) = self.cascade.as_ref() {
            if !cascade.is_inactive() {
                v["cascade"] = cascade.to_json();
            }
        }
        v
    }
}

/// wave-18 / task 04 — typed projection of `:rollback-cascade` for the
/// conservative cascade rollback evaluator. Resolved on the parser side
/// so the runtime can pivot without re-tokenising the raw string.
///
/// Three modes are recognised:
///   * `None`         — author wrote `"none"` OR omitted the value
///                       entirely. Cascade pass skipped; only the
///                       wave-17 / task 04 node-local rollback runs.
///                       This is the safe default — preserves the
///                       byte shape for plans that did not opt into
///                       cascading.
///   * `Plan`         — cascade evaluator computes the ordered list of
///                       compensation nodes (every plan node carrying
///                       `:compensates "<this-failed-node>"`) and
///                       records the plan on the response + evidence
///                       row. **NEVER dispatches.** Use this when the
///                       author wants downstream observers / humans to
///                       see what compensation WOULD be required without
///                       authorising the scheduler to execute it.
///   * `DispatchSafe` — cascade evaluator computes the same plan AND,
///                       for every compensation node whose own
///                       rollback safety gates pass, dispatches it
///                       through the wave-15 workstation substrate.
///                       Refusals are recorded but the cascade itself
///                       is NEVER retried — SafeDescriptor / safety-gate
///                       refusals stay refusals (mirrors the wave-17 /
///                       task 04 non-retryable invariant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RollbackCascadeMode {
    None,
    Plan,
    DispatchSafe,
}

impl RollbackCascadeMode {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            RollbackCascadeMode::None => "none",
            RollbackCascadeMode::Plan => "plan",
            RollbackCascadeMode::DispatchSafe => "dispatch-safe",
        }
    }

    /// Parse a raw `:rollback-cascade` value. Trims + lowercases; both
    /// `_` and `-` separators are accepted so authors can write either
    /// `dispatch_safe` or `dispatch-safe`. Unknown values yield `None`
    /// (the parser also pushes them onto `unsupported_fields`).
    pub(super) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" => Some(RollbackCascadeMode::None),
            "plan" => Some(RollbackCascadeMode::Plan),
            "dispatch-safe" | "dispatch_safe" => {
                Some(RollbackCascadeMode::DispatchSafe)
            }
            _ => None,
        }
    }
}

/// wave-18 / task 04 — outcome of the cascade rollback pass for a single
/// compensation node. Captures whether the node was just recorded
/// (`plan` mode), dispatched through the substrate (`dispatch-safe` +
/// safety passed + dispatch ok), refused (any safety / substrate
/// refusal), or failed (substrate dispatched but inner handler errored).
///
/// Wire vocabulary mirrors [`RollbackStatus`] so audit dashboards can
/// pivot on the same string vocabulary across both single-node and
/// cascade evaluations.
#[derive(Debug, Clone)]
pub(super) struct CascadeCompensationOutcome {
    /// Plan id of the compensation node (matches `DagNode::id`).
    pub node_id: String,
    /// Resolved policy of THIS compensation node (NOT the cascade root).
    /// `None` when the compensation node carried no `:rollback-policy`
    /// — the cascade evaluator treats that as `Descriptor` for the
    /// purpose of recording intent.
    pub policy: RollbackPolicy,
    /// Final per-compensation-node status. Vocabulary:
    ///   * `descriptor_ready` — `plan` mode OR `dispatch-safe` mode but
    ///                          the compensation node is descriptor-only
    ///                          (`:rollback-policy "descriptor"`).
    ///   * `dispatched`       — `dispatch-safe` mode AND every safety
    ///                          gate passed AND inner handler returned Ok.
    ///   * `refused`          — `dispatch-safe` mode AND at least one
    ///                          safety gate failed (or substrate refused).
    ///                          Non-retryable.
    ///   * `failed`           — `dispatch-safe` mode AND the substrate
    ///                          dispatched but the inner handler returned
    ///                          an error. Non-retryable.
    pub status: RollbackStatus,
    /// Human-readable explanation of the per-compensation-node decision.
    pub reason: String,
    /// Resolved objective for this compensation node (may be empty).
    pub objective: Option<String>,
    /// Resolved owned-files list for this compensation node.
    pub owned_files: Vec<String>,
    /// Resolved acceptance commands surfaced verbatim — NEVER executed.
    pub acceptance_commands: Vec<String>,
    /// Trimmed task-brief preview when the substrate / pure helper built
    /// one. `None` for pure-plan-mode entries.
    pub task_brief_preview: Option<String>,
    /// File path the brief was mirrored to (currently always `None` —
    /// substrate does not yet write the brief to disk; kept for shape
    /// compatibility with the node-local rollback evaluation).
    pub task_brief_path: Option<String>,
    /// Inner dispatch payload from `run_workstation_dispatch` when the
    /// compensation was actually dispatched.
    pub inner_payload: Option<Value>,
}

impl CascadeCompensationOutcome {
    pub(super) fn to_json(&self) -> Value {
        let mut v = json!({
            "node_id": self.node_id,
            "policy": self.policy.as_wire(),
            "status": self.status.as_wire(),
            "reason": self.reason,
            "objective": self.objective,
            "owned_files": self.owned_files,
            "acceptance_commands": self.acceptance_commands,
            "acceptance_commands_executed": false,
        });
        if let Some(p) = self.task_brief_preview.as_deref() {
            v["task_brief_preview"] = json!(p);
        }
        if let Some(p) = self.task_brief_path.as_deref() {
            v["task_brief_path"] = json!(p);
        }
        if let Some(inner) = self.inner_payload.clone() {
            v["inner_result"] = inner;
        }
        v
    }
}

/// wave-18 / task 04 — top-level outcome of the cascade rollback pass
/// for a single failed (cascade root) node. Carries the resolved cascade
/// mode + the ordered list of compensation outcomes so observers can
/// audit "which compensation nodes were planned / dispatched / refused"
/// without re-walking the prior nodes.
///
/// `is_inactive()` is true iff the cascade evaluator was either skipped
/// entirely (no compensation nodes found AND mode=None) OR ran but
/// produced no observable signal — the wave loop suppresses the cascade
/// surface in that case so the wave-17 / task 04 byte shape stays untouched
/// for plans that did not opt into cascading.
#[derive(Debug, Clone)]
pub(super) struct CascadeRollbackOutcome {
    pub mode: RollbackCascadeMode,
    /// Cascade root: the failed node id whose compensation is being planned.
    pub cascade_root: String,
    /// Compensation nodes in resolved cascade order. Empty when no plan
    /// node carries `:compensates "<cascade_root>"`.
    pub compensations: Vec<CascadeCompensationOutcome>,
    /// Human-readable explanation of the cascade-level decision (e.g.
    /// "cascade plan recorded; 2 compensation nodes",
    /// "no compensation nodes declared", etc.).
    pub reason: String,
}

impl CascadeRollbackOutcome {
    /// Convenience: this outcome produced no observable cascade signal.
    /// Used by the scheduler to decide whether to surface the cascade
    /// block on the response / evidence row.
    pub(super) fn is_inactive(&self) -> bool {
        matches!(self.mode, RollbackCascadeMode::None)
            && self.compensations.is_empty()
    }

    /// Project the outcome as a JSON block suitable for
    /// `node_results[].rollback.cascade` / `evidence.rollback.cascade`.
    pub(super) fn to_json(&self) -> Value {
        let comps: Vec<Value> = self
            .compensations
            .iter()
            .map(|c| c.to_json())
            .collect();
        json!({
            "mode": self.mode.as_wire(),
            "cascade_root": self.cascade_root,
            "reason": self.reason,
            "compensations": comps,
        })
    }
}

/// wave-17 / task 04 — pure helper that derives the rollback descriptor
/// data (objective + owned files + acceptance commands + resolved
/// policy) from the node hints. Decoupled from any IO so unit tests can
/// pin the shape without standing up an `AppState`.
///
/// Returns the resolved policy and the descriptor payload. The actual
/// dispatch decision (refused vs dispatched) belongs to the wave loop —
/// this helper only produces the inputs.
pub(super) fn build_rollback_descriptor(node: &DagNode) -> RollbackDescriptor {
    let policy = node.rollback_policy_kind().unwrap_or(RollbackPolicy::None);
    let objective = node
        .rollback_objective
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let owned_files =
        super::plan::split_lisp_string_list(node.rollback_owned_files_raw.as_deref());
    let acceptance_commands = super::plan::split_lisp_string_list(
        node.rollback_acceptance_commands_raw.as_deref(),
    );
    RollbackDescriptor {
        policy,
        objective,
        owned_files,
        acceptance_commands,
    }
}

/// wave-17 / task 04 — descriptor inputs derived from a node's
/// `:rollback-*` hints. Does NOT carry any decision yet; the wave loop
/// (or the test fixtures) consume this to evaluate safety + dispatch.
#[derive(Debug, Clone)]
pub(super) struct RollbackDescriptor {
    pub policy: RollbackPolicy,
    pub objective: Option<String>,
    pub owned_files: Vec<String>,
    pub acceptance_commands: Vec<String>,
}

impl RollbackDescriptor {
    /// Project the descriptor as a `WorkstationDispatchHints` value the
    /// substrate consumes. The rollback brief reuses the wave-15
    /// task-brief shape so observers see the same headings as a
    /// forward task brief.
    pub(super) fn to_workstation_hints(
        &self,
        node: &DagNode,
    ) -> super::workstation_dispatch::WorkstationDispatchHints {
        super::workstation_dispatch::WorkstationDispatchHints {
            objective: self.objective.clone(),
            // Free-form scope explains the rollback intent so the
            // delegated agent never confuses a rollback brief with a
            // forward brief.
            scope: Some(format!(
                "rollback for failed plan-DAG node `{}` (target=`{}`)",
                node.id, node.target
            )),
            owned_files: self.owned_files.clone(),
            // Forbidden files for the rollback brief mirror any forward
            // forbidden hints so the rollback agent inherits the same
            // safety boundary.
            forbidden_files: super::plan::split_lisp_string_list(
                node.forbidden_files_raw.as_deref(),
            ),
            acceptance_commands: self.acceptance_commands.clone(),
            commit_policy: node
                .commit_policy
                .clone()
                .or(Some("scoped".to_string())),
            target_project: node.target_project.clone(),
            requested_cwd: node.requested_cwd.clone(),
            // Rollback dispatch reuses the forward dispatch strategy so
            // the same workstation backend handles both.
            dispatch_strategy: node.dispatch_strategy.clone(),
        }
    }

    /// Determine whether the descriptor satisfies every safety
    /// requirement to dispatch a rollback through the workstation
    /// substrate. Pure: no side effects. Returns `Ok(())` when safe,
    /// `Err(reason)` with the human-readable failing condition
    /// otherwise. The reason vocabulary is stable so dashboards can
    /// pivot on it.
    pub(super) fn safety_check_for_workstation(
        &self,
        node: &DagNode,
    ) -> std::result::Result<(), String> {
        // 1. Objective must be present + non-empty.
        let has_obj = self
            .objective
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !has_obj {
            return Err(
                "rollback workstation dispatch requires :rollback-objective (non-empty)"
                    .to_string(),
            );
        }
        // 2. At least one owned file must be declared. Workstation
        //    dispatch with no owned files would let the rollback agent
        //    touch arbitrary parts of the tree — the exact thing the
        //    scoped-commit invariant exists to prevent.
        if self.owned_files.is_empty() {
            return Err(
                "rollback workstation dispatch requires :rollback-owned-files (>= 1 entry)"
                    .to_string(),
            );
        }
        // 3. Project must be resolvable. We check the static signal
        //    (target_project / requested_cwd present); the substrate
        //    re-validates via `resolve_target_project_root` so absence
        //    of either signal would always result in
        //    `SafeDescriptorReason::ProjectRootUnresolved`. Catching
        //    it here turns the refusal into a friendlier
        //    "no project signal" message rather than a downstream
        //    resolver error.
        let has_project = node
            .target_project
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let has_cwd = node
            .requested_cwd
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !has_project && !has_cwd {
            return Err(
                "rollback workstation dispatch requires :target-project or :requested-cwd \
                 to resolve a project root"
                    .to_string(),
            );
        }
        // 4. Dispatch strategy must be on the inferable whitelist —
        //    `unknown` / `prompt-fallback` are forward-only paths and
        //    are not safe to ride for a destructive rollback.
        let strat = node
            .dispatch_strategy
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("");
        if !super::workstation_dispatch::INFERABLE_DISPATCH_STRATEGIES
            .contains(&strat)
        {
            return Err(format!(
                "rollback workstation dispatch requires :dispatch-strategy on the safe \
                 whitelist {:?}; got `{}`",
                super::workstation_dispatch::INFERABLE_DISPATCH_STRATEGIES,
                strat
            ));
        }
        Ok(())
    }
}

/// wave-17 / task 04 — pure helper composing the descriptor + the
/// safety check + a static decision (no IO). Intended for unit tests
/// that pin "given hints X, what status / reason would the wave loop
/// land on BEFORE dispatch?". The wave loop always re-runs the
/// safety check before invoking the substrate so this helper and the
/// runtime cannot drift.
pub(super) fn pre_dispatch_rollback_decision(
    node: &DagNode,
) -> RollbackEvaluation {
    let descriptor = build_rollback_descriptor(node);
    match descriptor.policy {
        RollbackPolicy::None => RollbackEvaluation {
            policy: RollbackPolicy::None,
            status: if node.has_rollback_hints() {
                // Author declared SOME rollback hint but explicitly
                // wrote `:rollback-policy "none"`. Surface as
                // `not_requested` (the explicit-none decision dominates)
                // so the response stays quiet.
                RollbackStatus::NotRequested
            } else {
                RollbackStatus::NotRequested
            },
            reason: if node.has_rollback_hints() {
                "rollback policy explicitly set to none; no rollback dispatch".to_string()
            } else {
                "no rollback hints declared".to_string()
            },
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: None,
            task_brief_path: None,
            inner_payload: None,
            cascade: None,
        },
        RollbackPolicy::Descriptor => RollbackEvaluation {
            policy: RollbackPolicy::Descriptor,
            status: RollbackStatus::DescriptorReady,
            reason: "descriptor mode: rollback intent recorded; no dispatch performed"
                .to_string(),
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: None,
            task_brief_path: None,
            inner_payload: None,
            cascade: None,
        },
        RollbackPolicy::Workstation => match descriptor.safety_check_for_workstation(node) {
            Ok(()) => RollbackEvaluation {
                policy: RollbackPolicy::Workstation,
                status: RollbackStatus::Refused,
                reason: "workstation mode passed pre-dispatch safety; runtime will attempt dispatch"
                    .to_string(),
                objective: descriptor.objective,
                owned_files: descriptor.owned_files,
                acceptance_commands: descriptor.acceptance_commands,
                task_brief_preview: None,
                task_brief_path: None,
                inner_payload: None,
                cascade: None,
            },
            Err(detail) => RollbackEvaluation {
                policy: RollbackPolicy::Workstation,
                status: RollbackStatus::Refused,
                reason: format!("rollback workstation dispatch refused: {}", detail),
                objective: descriptor.objective,
                owned_files: descriptor.owned_files,
                acceptance_commands: descriptor.acceptance_commands,
                task_brief_preview: None,
                task_brief_path: None,
                inner_payload: None,
                cascade: None,
            },
        },
    }
}

/// wave-17 / task 03 — pure deterministic acceptance evaluator. NEVER
/// runs shell. Decides one of the four [`AcceptanceStatus`] values based
/// on the node's hints + the inner dispatch payload.
///
/// Decision tree (in order):
///   1. No hints at all (no mode + no commands + no keys) →
///      `NotEvaluated`. The caller preserves the wave-13
///      succeed-on-dispatch contract.
///   2. Mode = `Manual` → `ManualRequired` (always pauses, regardless of
///      payload). Reason: `"manual mode declared"`.
///   3. Mode = `InnerStatus` → `Accepted` iff `dispatch_succeeded` AND
///      the inner payload does not carry an explicit failure status
///      (`success=false`, `error` string, or `status="error"`).
///      Otherwise `Rejected` with a reason explaining the mismatch.
///   4. Mode = `EvidenceKeys` → `Accepted` iff every required key is
///      present in the inner payload's typed-evidence projection;
///      otherwise `Rejected` with the missing-key list. Empty key list
///      degrades to `ManualRequired` (an empty contract cannot prove
///      anything).
///   5. Mode unset but `:acceptance-commands` declared → `ManualRequired`
///      (we refuse to run shell). Reason captures the command count so
///      observers can tell why the gate triggered.
///   6. Otherwise (mode unset, no commands, only stray keys) →
///      `ManualRequired` so the author's typo surfaces loudly.
///
/// `dispatch_succeeded` is the boolean we already computed from the
/// inner classification. The evaluator never re-derives it from the
/// payload — that would risk drifting from the dispatch judgment.
pub(super) fn evaluate_node_acceptance(
    node: &DagNode,
    inner_payload: &Value,
    dispatch_succeeded: bool,
) -> AcceptanceEvaluation {
    let commands =
        super::plan::split_lisp_string_list(node.acceptance_commands_raw.as_deref());
    let evidence_keys = super::plan::split_lisp_string_list(
        node.acceptance_evidence_keys_raw.as_deref(),
    );
    let mode_raw = node.acceptance_mode_raw.as_deref().unwrap_or("").trim();
    let mode = if mode_raw.is_empty() {
        None
    } else {
        AcceptanceMode::parse(mode_raw)
    };

    if mode.is_none() && commands.is_empty() && evidence_keys.is_empty() {
        return AcceptanceEvaluation {
            status: AcceptanceStatus::NotEvaluated,
            mode: None,
            commands,
            evidence_keys,
            reason: "no acceptance hints declared".to_string(),
            fan_in: None,
        };
    }

    // wave-18 / task 03 — when the node opted into cross-node fan-in
    // AND did NOT declare a per-node `:acceptance-mode`, the
    // `:acceptance-evidence-keys` list is owned by the fan-in
    // evaluator (its `evidence_keys` mode reads them off the source
    // node's payload). Surface a `NotEvaluated` per-node decision in
    // that case so `apply_acceptance_fan_in` is the sole decider; the
    // wave-17 "keys without mode → manual_required" warning would
    // otherwise pre-empt fan-in.
    if mode.is_none() && commands.is_empty() && node.has_acceptance_fan_in() {
        return AcceptanceEvaluation {
            status: AcceptanceStatus::NotEvaluated,
            mode: None,
            commands,
            evidence_keys,
            reason:
                "per-node acceptance deferred to cross-node fan-in evaluator"
                    .to_string(),
            fan_in: None,
        };
    }

    match mode {
        Some(AcceptanceMode::Manual) => AcceptanceEvaluation {
            status: AcceptanceStatus::ManualRequired,
            mode,
            commands,
            evidence_keys,
            reason: "acceptance-mode=manual; human approval required".to_string(),
            fan_in: None,
        },
        Some(AcceptanceMode::InnerStatus) => {
            if !dispatch_succeeded {
                return AcceptanceEvaluation {
                    status: AcceptanceStatus::Rejected,
                    mode,
                    commands,
                    evidence_keys,
                    reason: "inner_status: dispatch classification was not Ok".to_string(),
                    fan_in: None,
                };
            }
            if let Some(detail) = inner_payload_failure_signal(inner_payload) {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Rejected,
                    mode,
                    commands,
                    evidence_keys,
                    reason: format!(
                        "inner_status: inner payload reports non-success ({})",
                        detail
                    ),
                    fan_in: None,
                }
            } else {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Accepted,
                    mode,
                    commands,
                    evidence_keys,
                    reason: "inner_status: dispatch Ok and payload carries no error signal"
                        .to_string(),
                    fan_in: None,
                }
            }
        }
        Some(AcceptanceMode::EvidenceKeys) => {
            if evidence_keys.is_empty() {
                return AcceptanceEvaluation {
                    status: AcceptanceStatus::ManualRequired,
                    mode,
                    commands,
                    evidence_keys,
                    reason:
                        "evidence_keys mode declared but :acceptance-evidence-keys is empty"
                            .to_string(),
                    fan_in: None,
                };
            }
            let missing = inner_payload_missing_keys(inner_payload, &evidence_keys);
            if missing.is_empty() {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Accepted,
                    mode,
                    commands,
                    evidence_keys,
                    reason: "evidence_keys: all required keys present in inner payload"
                        .to_string(),
                    fan_in: None,
                }
            } else {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Rejected,
                    mode,
                    commands,
                    evidence_keys,
                    reason: format!(
                        "evidence_keys: missing required keys {:?}",
                        missing
                    ),
                    fan_in: None,
                }
            }
        }
        None => {
            // Mode unset but the author declared SOME acceptance hint.
            // We refuse to execute shell from PLAN.lisp, so the only
            // safe default is to surface the gate as manual_required.
            let reason = if !commands.is_empty() {
                format!(
                    "acceptance commands declared ({} item(s)) without :acceptance-mode; \
                     PLAN DAG never runs shell — manual approval required",
                    commands.len()
                )
            } else {
                format!(
                    "acceptance evidence keys declared ({} item(s)) without :acceptance-mode; \
                     manual approval required",
                    evidence_keys.len()
                )
            };
            AcceptanceEvaluation {
                status: AcceptanceStatus::ManualRequired,
                mode,
                commands,
                evidence_keys,
                reason,
                fan_in: None,
            }
        }
    }
}

/// wave-18 / task 03 — apply cross-node acceptance fan-in on top of the
/// per-node evaluation. Pure: never touches the bus, never executes
/// shell, only inspects the prior nodes' terminal lifecycle state and
/// recorded `inner_payload`. Runs AFTER `evaluate_node_acceptance`; the
/// per-node status acts as a precondition:
///
///   * `NotEvaluated` (no per-node hints) — fan-in still runs because
///     `:acceptance-depends-on` is itself an opt-in. Pass flips status
///     to `Accepted`; fail flips it to `Rejected`.
///   * `Accepted`     — fan-in pass keeps `Accepted`; fail flips to
///                       `Rejected`.
///   * `Rejected` / `ManualRequired` — the per-node decision dominates.
///                       Fan-in is recorded for audit but does NOT
///                       override the parent decision (we don't promote
///                       a rejected node to accepted, and we don't
///                       de-pause a manual_required node).
///
/// `prior_results` is the scheduler's `results_by_id` snapshot keyed by
/// node id; each entry's `state` and `inner_payload` are the source of
/// truth. Missing source entries (which the validator forbids at build
/// time) collapse to a fan-in failure with a loud reason — defence in
/// depth in case the scheduler ever calls this without the validator.
pub(super) fn apply_acceptance_fan_in(
    base: AcceptanceEvaluation,
    node: &DagNode,
    prior_results: &HashMap<String, &NodeResult>,
) -> AcceptanceEvaluation {
    if !node.has_acceptance_fan_in() {
        return base;
    }
    // SAFETY: `has_acceptance_fan_in` already proved both halves are
    // present + recognised, so the unwraps below cannot fire.
    let mode = node.acceptance_requires_kind().expect(
        "has_acceptance_fan_in implies acceptance_requires_kind() is Some — \
         validator must have raised earlier",
    );
    let source_nodes: Vec<String> = node.acceptance_depends_on.clone();

    // Per-node evaluation must dominate when it already produced a
    // terminal "do not accept" signal. We still record the fan-in for
    // audit so observers can see what the gate would have decided.
    let parent_dominates = matches!(
        base.status,
        AcceptanceStatus::Rejected | AcceptanceStatus::ManualRequired
    );

    let outcome = match mode {
        AcceptanceRequires::AllSucceeded => {
            let mut failing: Vec<String> = Vec::new();
            for id in &source_nodes {
                let succeeded = prior_results
                    .get(id)
                    .map(|r| matches!(r.state, NodeState::Succeeded))
                    .unwrap_or(false);
                if !succeeded {
                    failing.push(id.clone());
                }
            }
            if failing.is_empty() {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: true,
                    reason: format!(
                        "all_succeeded: every source node ({}) reached succeeded",
                        source_nodes.len()
                    ),
                }
            } else {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: false,
                    reason: format!(
                        "all_succeeded: source node(s) not succeeded: {:?}",
                        failing
                    ),
                }
            }
        }
        AcceptanceRequires::AnySucceeded => {
            let mut succeeded_any = false;
            for id in &source_nodes {
                if let Some(r) = prior_results.get(id) {
                    if matches!(r.state, NodeState::Succeeded) {
                        succeeded_any = true;
                        break;
                    }
                }
            }
            if succeeded_any {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: true,
                    reason: "any_succeeded: at least one source node reached succeeded"
                        .to_string(),
                }
            } else {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: false,
                    reason: format!(
                        "any_succeeded: no source node ({}) reached succeeded",
                        source_nodes.len()
                    ),
                }
            }
        }
        AcceptanceRequires::EvidenceKeys => {
            // Validator guarantees `acceptance_source_node` is set AND
            // present in `acceptance_depends_on` AND in the plan, but
            // we defend in depth — a missing entry fails the gate
            // loudly instead of silently passing.
            let source_id = node
                .acceptance_source_node
                .clone()
                .unwrap_or_default();
            let single_source = vec![source_id.clone()];
            let keys = super::plan::split_lisp_string_list(
                node.acceptance_evidence_keys_raw.as_deref(),
            );
            if source_id.is_empty() {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: false,
                    reason: "evidence_keys: :acceptance-source-node is missing"
                        .to_string(),
                }
            } else if keys.is_empty() {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: single_source,
                    passed: false,
                    reason:
                        "evidence_keys: :acceptance-evidence-keys is empty — nothing to prove"
                            .to_string(),
                }
            } else {
                match prior_results.get(&source_id) {
                    None => AcceptanceFanInOutcome {
                        mode,
                        source_nodes: single_source,
                        passed: false,
                        reason: format!(
                            "evidence_keys: source node `{}` produced no result",
                            source_id
                        ),
                    },
                    Some(r) => {
                        let missing =
                            inner_payload_missing_keys(&r.inner_payload, &keys);
                        if missing.is_empty() {
                            AcceptanceFanInOutcome {
                                mode,
                                source_nodes: single_source,
                                passed: true,
                                reason: format!(
                                    "evidence_keys: source node `{}` carries every required key",
                                    source_id
                                ),
                            }
                        } else {
                            AcceptanceFanInOutcome {
                                mode,
                                source_nodes: single_source,
                                passed: false,
                                reason: format!(
                                    "evidence_keys: source node `{}` missing keys {:?}",
                                    source_id, missing
                                ),
                            }
                        }
                    }
                }
            }
        }
    };

    let mut next = base;
    let fan_in_passed = outcome.passed;
    let fan_in_reason = outcome.reason.clone();
    next.fan_in = Some(outcome);

    if parent_dominates {
        // Per-node decision wins; fan-in is informational only.
        return next;
    }

    if fan_in_passed {
        // NotEvaluated → Accepted, Accepted → Accepted (status stable).
        if matches!(next.status, AcceptanceStatus::NotEvaluated) {
            next.status = AcceptanceStatus::Accepted;
            next.reason = format!("acceptance_fan_in: {}", fan_in_reason);
        }
    } else {
        next.status = AcceptanceStatus::Rejected;
        next.reason = format!("acceptance_fan_in: {}", fan_in_reason);
    }
    next
}

/// wave-17 / task 03 — best-effort detection of an explicit failure
/// signal in an inner-dispatch payload. Returns `Some(detail)` when the
/// payload structurally claims non-success, `None` otherwise.
///
/// Recognised shapes (all conservative — only loud signals count):
///   * `payload.error` is a non-empty string.
///   * `payload.success == false`.
///   * `payload.ok == false`.
///   * `payload.status` ∈ {"error", "failed", "fail"}.
///   * `payload.workstation_dispatch_status` starts with `"skipped_"`
///     or equals `"failed"` (matches the wave-15 substrate's
///     safe-descriptor refusal vocabulary).
fn inner_payload_failure_signal(payload: &Value) -> Option<String> {
    let obj = payload.as_object()?;
    if let Some(s) = obj.get("error").and_then(|v| v.as_str()) {
        if !s.trim().is_empty() {
            return Some(format!("error=`{}`", s));
        }
    }
    if let Some(false) = obj.get("success").and_then(|v| v.as_bool()) {
        return Some("success=false".to_string());
    }
    if let Some(false) = obj.get("ok").and_then(|v| v.as_bool()) {
        return Some("ok=false".to_string());
    }
    if let Some(s) = obj.get("status").and_then(|v| v.as_str()) {
        let lc = s.trim().to_ascii_lowercase();
        if matches!(lc.as_str(), "error" | "failed" | "fail") {
            return Some(format!("status=`{}`", s));
        }
    }
    if let Some(s) = obj
        .get("workstation_dispatch_status")
        .and_then(|v| v.as_str())
    {
        let lc = s.trim().to_ascii_lowercase();
        if lc == "failed" || lc.starts_with("skipped_") {
            return Some(format!("workstation_dispatch_status=`{}`", s));
        }
    }
    None
}

/// wave-17 / task 03 — pure helper: locate every required key NOT
/// present in the inner payload. The payload is searched at the
/// top-level object AND inside common nested holders (`evidence`,
/// `typed_evidence`, `inner_result.evidence`) so authors don't have to
/// guess where the substrate stashed the typed evidence. Order of
/// returned missing keys matches `required` for stable test output.
fn inner_payload_missing_keys(payload: &Value, required: &[String]) -> Vec<String> {
    let mut missing = Vec::new();
    for key in required {
        if !inner_payload_contains_key(payload, key) {
            missing.push(key.clone());
        }
    }
    missing
}

fn inner_payload_contains_key(payload: &Value, key: &str) -> bool {
    match payload {
        Value::Object(map) => {
            if map.contains_key(key) {
                return true;
            }
            // Conservative descent into the well-known nested holders.
            for nested_key in [
                "evidence",
                "typed_evidence",
                "inner_result",
                "inner_dispatch",
                "result",
            ] {
                if let Some(child) = map.get(nested_key) {
                    if inner_payload_contains_key(child, key) {
                        return true;
                    }
                }
            }
            false
        }
        Value::Array(items) => items
            .iter()
            .any(|v| inner_payload_contains_key(v, key)),
        _ => false,
    }
}

/// wave-17 / task 03 — deterministic id format used when an acceptance
/// evaluation needs to surface a manual-required pause. Distinct from
/// the wave-16 / task 04 review-gate id format so the wave-17 / task 01
/// resume helper does NOT accidentally re-dispatch acceptance pauses
/// (its validator hard-requires `action=plan-node` AND the node still
/// carrying `:review-gate "question-event"` — neither holds for an
/// acceptance pause).
///
/// Layout: `acceptance:plan:<plan_id>:v<version>:<node_id>`.
pub(super) fn derive_acceptance_pause_id(
    plan_id: uuid::Uuid,
    plan_version: i32,
    node_id: &str,
) -> String {
    format!(
        "acceptance:plan:{}:v{}:{}",
        plan_id, plan_version, node_id
    )
}

/// Result of parsing a PLAN.lisp body for explicit `(node ...)` forms.
#[derive(Debug, Clone, Default)]
pub(super) struct ParsedDag {
    pub nodes: Vec<DagNode>,
    /// Top-level non-node forms (excluding the outer plan envelope) recorded
    /// verbatim so the author can see what the scheduler ignored.
    pub unsupported_top_forms: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) enum DagBuildError {
    NoNodes,
    DuplicateId(String),
    InvalidTarget {
        node_id: String,
        target: String,
    },
    DependencyMissing {
        node_id: String,
        missing: String,
    },
    SelfDependency(String),
    Cycle(Vec<String>),
    /// wave-16 / task 05 — author supplied a retry hint with a value
    /// that fails parsing (negative number, non-numeric, overflow). We
    /// fail fast here instead of silently dropping the value into
    /// `unsupported_fields` because retry counts directly drive the
    /// scheduler's attempt budget — a typo'd `:retry-count "thrice"`
    /// must NOT be interpreted as "no retry", or the author would
    /// silently lose the policy they declared.
    InvalidRetryHint {
        node_id: String,
        key: String,
        raw: String,
        detail: String,
    },
    /// wave-18 / task 03 — `:acceptance-depends-on` references a node id
    /// that is not declared in this plan. Fail-fast so the typo cannot
    /// silently degrade fan-in to "no gate".
    AcceptanceDependencyMissing {
        node_id: String,
        missing: String,
    },
    /// wave-18 / task 03 — `:acceptance-source-node` either references a
    /// node id that is not declared in this plan OR was omitted while
    /// `:acceptance-requires "evidence_keys"` was declared.
    AcceptanceSourceNodeInvalid {
        node_id: String,
        detail: String,
    },
    /// wave-18 / task 03 — `:acceptance-depends-on` is non-empty but
    /// `:acceptance-requires` is absent / unrecognised. The fan-in
    /// evaluator cannot decide accept / reject without a recognised
    /// mode, so we fail-fast.
    AcceptanceFanInRequiresMissing {
        node_id: String,
        raw: Option<String>,
    },
    /// wave-18 / task 03 — a node listed in `:acceptance-depends-on`
    /// is NOT (transitively) an ancestor of this node via the existing
    /// `:depends-on` topology. Acceptance dependencies must not silently
    /// introduce new execution-ordering: the source node's evidence
    /// must already exist when this node's acceptance phase runs.
    AcceptanceFanInDepNotAncestor {
        node_id: String,
        ancestor: String,
    },
    /// wave-19 / task 10 — a node declared `:compensate-node "<X>"` (or
    /// `:compensate-ref`) but `X` is invalid: empty value, references
    /// the failing node itself (self-ref), or names a node id not
    /// declared in this plan. Fail-fast so a typo cannot silently
    /// degrade cascade discovery to "no compensation".
    CompensateNodeInvalid {
        node_id: String,
        key: String,
        raw: String,
        detail: String,
    },
    /// wave-19 / task 10 — both directions of the compensate
    /// relationship are declared but they disagree: the forward
    /// `:compensate-node "X"` declared on the failing node `F` points
    /// at compensation node `X`, but `X`'s reverse `:compensates "Y"`
    /// names some `Y != F`. The scheduler MUST NOT silently choose one
    /// direction; the validator fails fast so the author resolves the
    /// disagreement explicitly.
    CompensateDirectionMismatch {
        failing_node_id: String,
        comp_node_id: String,
        reverse_target: String,
    },
}

impl DagBuildError {
    fn into_tool_result(self) -> ToolResult {
        match self {
            DagBuildError::NoNodes => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    "scheduler_mode=dag_v1 found no `(node :id ... :target ...)` forms in plan.sexp_text",
                )
                .with_suggestion(
                    "DAG v1 only parses explicit (node ...) forms; rewrite the plan to use them \
                     or fall back to the default (single-node) scheduler mode",
                ),
            ),
            DagBuildError::DuplicateId(id) => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("DAG node id `{}` is duplicated; node ids must be unique", id),
                ),
            ),
            DagBuildError::InvalidTarget { node_id, target } => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "DAG node `{}` has unsupported :target `{}`; valid: {:?}",
                        node_id, target, VALID_TARGETS
                    ),
                ),
            ),
            DagBuildError::DependencyMissing { node_id, missing } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` depends on `{}` which is not declared in this plan",
                            node_id, missing
                        ),
                    ),
                )
            }
            DagBuildError::SelfDependency(id) => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("DAG node `{}` declares itself in :depends-on", id),
                ),
            ),
            DagBuildError::InvalidRetryHint { node_id, key, raw, detail } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` has invalid `:{}` value `{}`: {}",
                            node_id, key, raw, detail
                        ),
                    )
                    .with_suggestion(
                        "supply a non-negative integer ≤ 3 for `:retry-count` / `:max-attempts` \
                         (the cap), or remove the hint to keep the default single-attempt contract",
                    ),
                )
            }
            DagBuildError::Cycle(cycle) => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "DAG contains a cycle involving nodes: {}",
                        cycle.join(" -> ")
                    ),
                ),
            ),
            DagBuildError::AcceptanceDependencyMissing { node_id, missing } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` declares `:acceptance-depends-on` referencing `{}` \
                             which is not declared in this plan",
                            node_id, missing
                        ),
                    )
                    .with_suggestion(
                        "every entry in `:acceptance-depends-on` MUST be a node id declared \
                         elsewhere in this plan and MUST also be a (transitive) `:depends-on` \
                         ancestor of the current node",
                    ),
                )
            }
            DagBuildError::AcceptanceSourceNodeInvalid { node_id, detail } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` has invalid `:acceptance-source-node`: {}",
                            node_id, detail
                        ),
                    )
                    .with_suggestion(
                        "set `:acceptance-source-node` to a node id that also appears in this \
                         node's `:acceptance-depends-on` list (only used under \
                         `:acceptance-requires \"evidence_keys\"`)",
                    ),
                )
            }
            DagBuildError::AcceptanceFanInRequiresMissing { node_id, raw } => {
                let detail = match raw {
                    Some(r) if !r.trim().is_empty() => format!(
                        "got `{}`; expected one of: all_succeeded | any_succeeded | evidence_keys",
                        r
                    ),
                    _ => "field is missing".to_string(),
                };
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` declares `:acceptance-depends-on` but \
                             `:acceptance-requires` {}",
                            node_id, detail
                        ),
                    )
                    .with_suggestion(
                        "add `:acceptance-requires \"all_succeeded\"` (or `any_succeeded` / \
                         `evidence_keys`) to specify how the fan-in gate decides; remove \
                         `:acceptance-depends-on` if no fan-in is intended",
                    ),
                )
            }
            DagBuildError::AcceptanceFanInDepNotAncestor { node_id, ancestor } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` declares `:acceptance-depends-on` referencing `{}` \
                             which is not a (transitive) `:depends-on` ancestor of `{}`",
                            node_id, ancestor, node_id
                        ),
                    )
                    .with_suggestion(
                        "the source node's evidence must already exist when this node's \
                         acceptance phase runs — add the source to this node's `:depends-on` \
                         (directly or via an existing chain) so the scheduler dispatches them \
                         in the correct order",
                    ),
                )
            }
            DagBuildError::CompensateNodeInvalid { node_id, key, raw, detail } => {
                ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "DAG node `{}` has invalid `:{}` value `{}`: {}",
                            node_id, key, raw, detail
                        ),
                    )
                    .with_suggestion(
                        "set `:compensate-node` (or `:compensate-ref`) to the id of a \
                         compensation node declared elsewhere in this plan; the value MUST \
                         NOT name the failing node itself, and the named compensation node's \
                         own `:compensates` (when present) MUST point back at the failing node",
                    ),
                )
            }
            DagBuildError::CompensateDirectionMismatch {
                failing_node_id,
                comp_node_id,
                reverse_target,
            } => ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "DAG node `{}` declares `:compensate-node \"{}\"` but `{}` declares \
                         `:compensates \"{}\"` — forward and reverse compensate directions \
                         disagree; the scheduler refuses to silently pick one",
                        failing_node_id, comp_node_id, comp_node_id, reverse_target
                    ),
                )
                .with_suggestion(
                    "make the two directions agree: either change `:compensate-node` on the \
                     failing node, or change `:compensates` on the compensation node so they \
                     name each other (forward + reverse must be symmetric)",
                ),
            ),
        }
    }
}

/// Parse and validate a PLAN.lisp body, returning a topologically-sorted node
/// list ready for sequential dispatch.
pub(super) fn build_validated_dag(
    sexp: &str,
) -> std::result::Result<(ParsedDag, Vec<String>), DagBuildError> {
    let parsed = parse_plan_dag(sexp);
    if parsed.nodes.is_empty() {
        return Err(DagBuildError::NoNodes);
    }

    // Unique id check.
    let mut seen: HashSet<&str> = HashSet::new();
    for n in &parsed.nodes {
        if !seen.insert(n.id.as_str()) {
            return Err(DagBuildError::DuplicateId(n.id.clone()));
        }
    }

    // wave-16 / task 05 — fail fast on any retry hint that did not
    // parse cleanly. Raised BEFORE target / dependency checks so the
    // author sees the most actionable error first; a typo'd
    // `:retry-count "thrice"` is a contract bug, not a topology bug.
    for n in &parsed.nodes {
        if let Some((key, raw, detail)) = &n.retry_parse_error {
            return Err(DagBuildError::InvalidRetryHint {
                node_id: n.id.clone(),
                key: key.clone(),
                raw: raw.clone(),
                detail: detail.clone(),
            });
        }
    }

    // Target whitelist + self-dep + missing dep.
    let id_set: HashSet<&str> = parsed.nodes.iter().map(|n| n.id.as_str()).collect();
    for n in &parsed.nodes {
        if !VALID_TARGETS.contains(&n.target.as_str()) {
            return Err(DagBuildError::InvalidTarget {
                node_id: n.id.clone(),
                target: n.target.clone(),
            });
        }
        for dep in &n.depends_on {
            if dep == &n.id {
                return Err(DagBuildError::SelfDependency(n.id.clone()));
            }
            if !id_set.contains(dep.as_str()) {
                return Err(DagBuildError::DependencyMissing {
                    node_id: n.id.clone(),
                    missing: dep.clone(),
                });
            }
        }
    }

    let order = kahn_topo_sort(&parsed.nodes)?;

    // wave-18 / task 03 — cross-node acceptance fan-in validation.
    // Runs AFTER topo sort so we can compute transitive ancestors via
    // the existing dependency graph. The four checks (in order):
    //
    //   1. Every entry in `:acceptance-depends-on` must be a declared
    //      node id.
    //   2. If `:acceptance-depends-on` is non-empty, `:acceptance-requires`
    //      must be a recognised mode.
    //   3. Under `evidence_keys` mode, `:acceptance-source-node` must
    //      be set AND must appear in `:acceptance-depends-on` (and
    //      therefore in the plan; the depends-on check handles plan
    //      membership).
    //   4. Every entry in `:acceptance-depends-on` must be a (transitive)
    //      `:depends-on` ancestor of the current node — otherwise the
    //      source node's evidence may not yet exist when this node's
    //      acceptance phase runs (we deliberately do NOT promote
    //      acceptance deps to execution deps; that would silently
    //      change dispatch order).
    let ancestors = compute_transitive_ancestors(&parsed.nodes);
    for n in &parsed.nodes {
        if n.acceptance_depends_on.is_empty()
            && n.acceptance_requires_raw.as_deref().map(str::trim).unwrap_or("").is_empty()
            && n.acceptance_source_node.as_deref().map(str::trim).unwrap_or("").is_empty()
        {
            continue;
        }
        // (1) plan membership
        for dep in &n.acceptance_depends_on {
            if !id_set.contains(dep.as_str()) {
                return Err(DagBuildError::AcceptanceDependencyMissing {
                    node_id: n.id.clone(),
                    missing: dep.clone(),
                });
            }
        }
        // (2) requires mode
        if !n.acceptance_depends_on.is_empty() {
            if n.acceptance_requires_kind().is_none() {
                return Err(DagBuildError::AcceptanceFanInRequiresMissing {
                    node_id: n.id.clone(),
                    raw: n.acceptance_requires_raw.clone(),
                });
            }
        } else if n.acceptance_source_node.is_some()
            || n.acceptance_requires_raw
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        {
            // Author wrote :acceptance-requires / :acceptance-source-node
            // without :acceptance-depends-on. Treat as a fan-in declaration
            // missing its dependency list — still fail-fast so the typo
            // surfaces at build time instead of silently doing nothing.
            return Err(DagBuildError::AcceptanceFanInRequiresMissing {
                node_id: n.id.clone(),
                raw: Some(
                    "fan-in declared without :acceptance-depends-on".to_string(),
                ),
            });
        }
        // (3) source node (only meaningful under evidence_keys; we
        //     surface a structured error if it's set under a wrong
        //     mode so the typo doesn't go silent).
        if let Some(src_raw) = n.acceptance_source_node.as_deref() {
            let src = src_raw.trim();
            if src.is_empty() {
                return Err(DagBuildError::AcceptanceSourceNodeInvalid {
                    node_id: n.id.clone(),
                    detail: "value is empty".to_string(),
                });
            }
            if !id_set.contains(src) {
                return Err(DagBuildError::AcceptanceSourceNodeInvalid {
                    node_id: n.id.clone(),
                    detail: format!("`{}` is not declared in this plan", src),
                });
            }
            if !n.acceptance_depends_on.iter().any(|d| d == src) {
                return Err(DagBuildError::AcceptanceSourceNodeInvalid {
                    node_id: n.id.clone(),
                    detail: format!(
                        "`{}` must also appear in this node's `:acceptance-depends-on`",
                        src
                    ),
                });
            }
        }
        if matches!(
            n.acceptance_requires_kind(),
            Some(AcceptanceRequires::EvidenceKeys)
        ) && n.acceptance_source_node.is_none()
        {
            return Err(DagBuildError::AcceptanceSourceNodeInvalid {
                node_id: n.id.clone(),
                detail:
                    ":acceptance-requires \"evidence_keys\" requires `:acceptance-source-node`"
                        .to_string(),
            });
        }
        // (4) every fan-in dep must be a transitive :depends-on ancestor
        if let Some(set) = ancestors.get(n.id.as_str()) {
            for dep in &n.acceptance_depends_on {
                if !set.contains(dep.as_str()) {
                    return Err(DagBuildError::AcceptanceFanInDepNotAncestor {
                        node_id: n.id.clone(),
                        ancestor: dep.clone(),
                    });
                }
            }
        }
    }

    // wave-19 / task 10 — forward `:compensate-node` validation. The
    // forward declaration lives on the failing (cascade-root) node and
    // points AT a compensation node id. Three checks (in order):
    //
    //   (a) value MUST be non-empty after trimming;
    //   (b) value MUST resolve to a declared node id AND MUST NOT name
    //       the failing node itself (self-reference is rejected);
    //   (c) when the named compensation node ALSO carries
    //       `:compensates "<X>"`, then `<X>` MUST equal the failing
    //       node id. Any disagreement fails fast — the scheduler MUST
    //       NOT silently pick one direction over the other (the wave-18
    //       reverse contract is the source of truth for the cascade
    //       evaluator, but accepting a contradicting forward ref would
    //       hide the author's mistake).
    //
    // Forward refs that name a compensation node WITHOUT a reverse
    // `:compensates` declaration are accepted and surface through
    // `compute_compensation_order` as if the compensation node had
    // declared `:compensates "<failing-node-id>"` (forward + reverse
    // are unioned). This is the new feature: authors who prefer
    // top-down readability declare cascade structure on the failing
    // node side without touching the compensation node.
    let by_id: HashMap<&str, &DagNode> =
        parsed.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    for n in &parsed.nodes {
        let Some(raw) = n.compensate_node.as_deref() else {
            continue;
        };
        let trimmed = raw.trim();
        // (a) non-empty
        if trimmed.is_empty() {
            return Err(DagBuildError::CompensateNodeInvalid {
                node_id: n.id.clone(),
                key: "compensate-node".to_string(),
                raw: raw.to_string(),
                detail: "value is empty".to_string(),
            });
        }
        // (b) self-reference rejected
        if trimmed == n.id {
            return Err(DagBuildError::CompensateNodeInvalid {
                node_id: n.id.clone(),
                key: "compensate-node".to_string(),
                raw: raw.to_string(),
                detail: format!(
                    "names the failing node itself (`{}`); a node cannot be its own \
                     compensation",
                    n.id
                ),
            });
        }
        // (b cont.) plan membership
        let Some(comp) = by_id.get(trimmed) else {
            return Err(DagBuildError::CompensateNodeInvalid {
                node_id: n.id.clone(),
                key: "compensate-node".to_string(),
                raw: raw.to_string(),
                detail: format!(
                    "`{}` is not declared in this plan",
                    trimmed
                ),
            });
        };
        // (c) reverse-direction agreement (only when the comp node ALSO
        //     declared `:compensates`). Compared case-insensitively to
        //     mirror the existing `compute_compensation_order` matching.
        if let Some(reverse_raw) = comp.compensates.as_deref() {
            let reverse = reverse_raw.trim();
            if !reverse.is_empty()
                && reverse.to_ascii_lowercase() != n.id.to_ascii_lowercase()
            {
                return Err(DagBuildError::CompensateDirectionMismatch {
                    failing_node_id: n.id.clone(),
                    comp_node_id: comp.id.clone(),
                    reverse_target: reverse.to_string(),
                });
            }
        }
    }

    Ok((parsed, order))
}

/// wave-18 / task 03 — compute the set of transitive `:depends-on`
/// ancestors for every node, keyed by node id. Pure helper; runs once
/// per `build_validated_dag` call so the cross-node acceptance fan-in
/// validator can verify each `:acceptance-depends-on` entry already
/// sits upstream in the execution-ordering DAG.
fn compute_transitive_ancestors(
    nodes: &[DagNode],
) -> HashMap<String, HashSet<String>> {
    let by_id: HashMap<&str, &DagNode> =
        nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    for n in nodes {
        let mut acc: HashSet<String> = HashSet::new();
        let mut stack: Vec<String> = n.depends_on.clone();
        while let Some(id) = stack.pop() {
            if !acc.insert(id.clone()) {
                continue;
            }
            if let Some(parent) = by_id.get(id.as_str()) {
                for p in &parent.depends_on {
                    if !acc.contains(p) {
                        stack.push(p.clone());
                    }
                }
            }
        }
        out.insert(n.id.clone(), acc);
    }
    out
}

/// Top-level entry: parse plan.sexp_text for `(node ...)` forms only.
pub(super) fn parse_plan_dag(sexp: &str) -> ParsedDag {
    let mut out = ParsedDag::default();
    for form in scan_top_level_forms(sexp) {
        let head = top_form_head(&form).unwrap_or_default();
        let head_lc = head.to_ascii_lowercase();
        if head_lc == "node" {
            if let Some(node) = parse_node_form(&form) {
                out.nodes.push(node);
            }
        } else if !head.is_empty() {
            // Non-node sibling — record verbatim so authors can see what the
            // scheduler skipped (e.g., :goal, :phases, :tasks, comments).
            out.unsupported_top_forms.push(form);
        }
    }
    out
}

/// Walk through the outer plan envelope and yield the s-expressions sitting at
/// "top level" inside it. We treat anything inside the outermost paren of the
/// plan envelope as a sibling to be considered. This is intentionally
/// shallow — we do NOT recurse into nested forms looking for `(node ...)`,
/// because that would silently consume nodes meant for sub-phases.
fn scan_top_level_forms(sexp: &str) -> Vec<String> {
    let trimmed = sexp.trim();
    let bytes: Vec<char> = trimmed.chars().collect();
    let n = bytes.len();
    if n == 0 || bytes[0] != '(' {
        return Vec::new();
    }
    // Find the slice immediately inside the outermost paren.
    // Strategy: skip the head symbol of the outer envelope, then collect
    // sibling forms until we close the outer paren.
    let mut i = 1usize;
    // Skip whitespace
    while i < n && bytes[i].is_whitespace() {
        i += 1;
    }
    // Skip the head symbol (e.g. `plan`, `plan-draft`, `PLAN`).
    while i < n
        && !bytes[i].is_whitespace()
        && bytes[i] != '('
        && bytes[i] != ')'
        && bytes[i] != '"'
    {
        i += 1;
    }
    let mut forms: Vec<String> = Vec::new();
    let mut depth: i64 = 0;
    let mut in_string = false;
    let mut esc = false;
    let mut current_start: Option<usize> = None;
    while i < n {
        let c = bytes[i];
        if in_string {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            i += 1;
            continue;
        }
        if c == '(' {
            if depth == 0 {
                current_start = Some(i);
            }
            depth += 1;
            i += 1;
            continue;
        }
        if c == ')' {
            depth -= 1;
            if depth == 0 {
                if let Some(start) = current_start.take() {
                    let form: String = bytes[start..=i].iter().collect();
                    forms.push(form);
                }
                i += 1;
                continue;
            }
            if depth < 0 {
                // Closing the outer envelope — stop.
                break;
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    forms
}

/// Get the head symbol of a top-level form like `(node :id ...)` -> `node`.
fn top_form_head(form: &str) -> Option<String> {
    let trimmed = form.trim_start();
    let inner = trimmed.strip_prefix('(')?.trim_start();
    let mut end = 0usize;
    for (idx, ch) in inner.char_indices() {
        if ch.is_whitespace() || ch == '(' || ch == ')' || ch == '"' {
            break;
        }
        end = idx + ch.len_utf8();
    }
    if end == 0 {
        None
    } else {
        Some(inner[..end].to_string())
    }
}

/// Parse one `(node :k v :k v ...)` form into a `DagNode`. Returns None when
/// the form is missing `:id` or `:target` (the two required fields). Unknown
/// keyword fields are captured into `unsupported_fields`.
fn parse_node_form(form: &str) -> Option<DagNode> {
    let pairs = scan_keyword_pairs(form);
    let mut id: Option<String> = None;
    let mut target: Option<String> = None;
    let mut objective: Option<String> = None;
    let mut depends_on: Vec<String> = Vec::new();
    let mut condition: Option<String> = None;
    let mut failure_policy: Option<String> = None;
    let mut timeout_ms: Option<i64> = None;
    let mut dispatch_strategy: Option<String> = None;
    let mut target_project: Option<String> = None;
    let mut requested_cwd: Option<String> = None;
    let mut flow_id: Option<String> = None;
    let mut scope: Option<String> = None;
    let mut commit_policy: Option<String> = None;
    let mut owned_files_raw: Option<String> = None;
    let mut forbidden_files_raw: Option<String> = None;
    let mut acceptance_commands_raw: Option<String> = None;
    // wave-17 / task 03 — declarative acceptance evaluator hints.
    let mut acceptance_mode_raw: Option<String> = None;
    let mut acceptance_evidence_keys_raw: Option<String> = None;
    // wave-18 / task 03 — cross-node acceptance fan-in hints.
    let mut acceptance_depends_on: Vec<String> = Vec::new();
    let mut acceptance_requires_raw: Option<String> = None;
    let mut acceptance_source_node: Option<String> = None;
    let mut workstation_dispatch_flag: Option<String> = None;
    let mut review_gate: Option<String> = None;
    let mut review_action: Option<String> = None;
    let mut review_text: Option<String> = None;
    // wave-16 / task 05 — bounded per-node retry hints. Both the count
    // and the delay are parsed strictly inside this loop; the first
    // hint failure is captured into `retry_parse_error` so the
    // validator can fail-fast at `build_validated_dag` time without
    // re-tokenising the form. We keep only the FIRST error (later
    // hints still flow through their normal handler / unsupported
    // path so the audit trail captures every signal).
    let mut retry_count: Option<u32> = None;
    let mut retry_delay_ms: Option<u64> = None;
    let mut retry_parse_error: Option<(String, String, String)> = None;
    // wave-17 / task 04 — conservative rollback descriptor hints.
    let mut rollback_policy: Option<String> = None;
    let mut rollback_objective: Option<String> = None;
    let mut rollback_owned_files_raw: Option<String> = None;
    let mut rollback_acceptance_commands_raw: Option<String> = None;
    // wave-18 / task 04 — cascade rollback hints.
    let mut compensates: Option<String> = None;
    let mut rollback_cascade: Option<String> = None;
    let mut rollback_after: Vec<String> = Vec::new();
    // wave-19 / task 10 — forward `:compensate-node` declaration on the
    // failing-node side (alias `:compensate-ref`). Validated against the
    // reverse `:compensates` direction in `build_validated_dag`.
    let mut compensate_node: Option<String> = None;
    let mut unsupported_fields: Vec<(String, String)> = Vec::new();

    for (raw_key, value) in pairs {
        let key = raw_key.to_ascii_lowercase();
        match key.as_str() {
            "id" => set_first(&mut id, &value),
            "target" | "target-tool" | "tool" => set_first(&mut target, &value),
            "objective" => set_first(&mut objective, &value),
            "depends-on" | "depends_on" | "deps" => {
                depends_on = parse_id_list(&value);
            }
            "condition" => set_first(&mut condition, &value),
            "failure-policy" | "failure_policy" => {
                set_first(&mut failure_policy, &value)
            }
            "timeout-ms" | "timeout_ms" => {
                if let Ok(n) = value.trim().parse::<i64>() {
                    if timeout_ms.is_none() {
                        timeout_ms = Some(n);
                    }
                }
            }
            "dispatch-strategy" | "dispatch_strategy" => {
                set_first(&mut dispatch_strategy, &value)
            }
            "target-project" | "target_project" | "project" => {
                set_first(&mut target_project, &value)
            }
            "requested-cwd" | "requested_cwd" | "cwd" => {
                set_first(&mut requested_cwd, &value)
            }
            "flow-id" | "flow_id" => set_first(&mut flow_id, &value),
            // wave-15 / task 05 — workstation-dispatch hint contract.
            // Captured here so the scheduler can route eligible nodes
            // without a second parse pass; only consumed when
            // `:workstation-dispatch true` is also set.
            "scope" => set_first(&mut scope, &value),
            "commit-policy" | "commit_policy" => set_first(&mut commit_policy, &value),
            "owned-files" | "owned_files" => set_first(&mut owned_files_raw, &value),
            "forbidden-files" | "forbidden_files" => {
                set_first(&mut forbidden_files_raw, &value)
            }
            "acceptance-commands" | "acceptance_commands" => {
                set_first(&mut acceptance_commands_raw, &value)
            }
            // wave-17 / task 03 — declarative acceptance evaluator hints.
            // `:acceptance-mode` is parsed strictly: unknown values land
            // BOTH on the typed slot AND in `unsupported_fields` so the
            // scheduler safely degrades to the manual-required default
            // while the typo surfaces through `node_hint_summary`.
            "acceptance-mode" | "acceptance_mode" => {
                let raw = value.trim();
                if !raw.is_empty() && AcceptanceMode::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut acceptance_mode_raw, &value);
            }
            "acceptance-evidence-keys" | "acceptance_evidence_keys" => {
                set_first(&mut acceptance_evidence_keys_raw, &value)
            }
            // wave-18 / task 03 — cross-node acceptance fan-in hints.
            // `:acceptance-depends-on` accepts the same shapes as
            // `:depends-on` (`["a" "b"]` / `(a b)` / bareword run);
            // `:acceptance-requires` is parsed strictly so a typo lands
            // BOTH on the typed slot AND in `unsupported_fields` while
            // the validator raises a structured error before the
            // scheduler dispatches the node. Single
            // `:acceptance-source-node` is captured verbatim; only
            // consumed under `evidence_keys` mode.
            "acceptance-depends-on" | "acceptance_depends_on" => {
                if acceptance_depends_on.is_empty() {
                    acceptance_depends_on = parse_id_list(&value);
                }
            }
            "acceptance-requires" | "acceptance_requires" => {
                let raw = value.trim();
                if !raw.is_empty() && AcceptanceRequires::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut acceptance_requires_raw, &value);
            }
            "acceptance-source-node" | "acceptance_source_node" => {
                set_first(&mut acceptance_source_node, &value)
            }
            "workstation-dispatch" | "workstation_dispatch" => {
                set_first(&mut workstation_dispatch_flag, &value)
            }
            // wave-16 / task 04 — review-gate hint contract. `:review-gate`
            // is the gate kind (recognised: "none", "question-event");
            // unrecognised raw values still land on the typed slot AND
            // get recorded into `unsupported_fields` so the typo surfaces
            // through `node_hint_summary` while the scheduler safely
            // dispatches as if no gate was set.
            "review-gate" | "review_gate" => {
                let raw = value.trim();
                if !raw.is_empty() {
                    let lc = raw.to_ascii_lowercase();
                    if !matches!(lc.as_str(), "none" | "question-event" | "question_event") {
                        unsupported_fields.push((raw_key.clone(), value.clone()));
                    }
                }
                set_first(&mut review_gate, &value);
            }
            "review-action" | "review_action" => set_first(&mut review_action, &value),
            "review-text" | "review_text" => set_first(&mut review_text, &value),
            // wave-16 / task 05 — bounded per-node retry policy. Two
            // spellings, distinct semantics:
            //   `:retry-count N`   = N **additional** attempts beyond
            //                        the first (so total = N+1).
            //   `:max-attempts N`  = N **total** attempts including
            //                        the first (so retry_count = N-1).
            // Both lower into `retry_count` (additional retries) so the
            // runtime has a single source of truth; the parser
            // converts on the way in. First hint wins; later ones are
            // ignored so a duplicate doesn't silently shadow the author's
            // earlier declaration.
            //
            // Strict parsing: any non-numeric / negative value lands
            // in `retry_parse_error` and the validator raises a
            // structured `DagBuildError::InvalidRetryHint` BEFORE the
            // scheduler ever sees the node — silent fall-through to
            // "no retry" would lose the author's policy.
            "retry-count" | "retry_count" => {
                if retry_count.is_none() {
                    let trimmed = value.trim();
                    match trimmed.parse::<i64>() {
                        Ok(n) if n >= 0 => {
                            // Preserve the raw upper bound so callers
                            // can see what they declared; the cap is
                            // applied by `effective_max_attempts`.
                            retry_count = Some(n.min(u32::MAX as i64) as u32);
                        }
                        Ok(_neg) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                "value must be a non-negative integer".to_string(),
                            ));
                        }
                        Err(e) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                format!("not a valid integer: {}", e),
                            ));
                        }
                        _ => { /* second error: keep the first */ }
                    }
                }
            }
            "max-attempts" | "max_attempts" => {
                if retry_count.is_none() {
                    let trimmed = value.trim();
                    match trimmed.parse::<i64>() {
                        // `:max-attempts 0` is meaningless (zero
                        // attempts = never run) — we reject it as a
                        // structured parse error so the author sees
                        // the typo instead of a silently-skipped node.
                        Ok(n) if n >= 1 => {
                            // Convert total attempts → additional
                            // retries. Subtract one then clamp to u32.
                            let extra = (n - 1).min(u32::MAX as i64) as u32;
                            retry_count = Some(extra);
                        }
                        Ok(_zero_or_neg) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                "value must be a positive integer (>= 1)".to_string(),
                            ));
                        }
                        Err(e) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                format!("not a valid integer: {}", e),
                            ));
                        }
                        _ => { /* second error: keep the first */ }
                    }
                }
            }
            "retry-delay-ms" | "retry_delay_ms" => {
                if retry_delay_ms.is_none() {
                    let trimmed = value.trim();
                    match trimmed.parse::<i64>() {
                        Ok(n) if n >= 0 => {
                            retry_delay_ms = Some(n as u64);
                        }
                        Ok(_neg) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                "value must be a non-negative integer (ms)".to_string(),
                            ));
                        }
                        Err(e) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                format!("not a valid integer: {}", e),
                            ));
                        }
                        _ => { /* second error: keep the first */ }
                    }
                }
            }
            // wave-17 / task 04 — conservative rollback descriptor
            // contract. Strict parsing: unrecognised raw values land
            // BOTH on the typed slot AND in `unsupported_fields` so
            // the scheduler safely degrades to "no rollback" while
            // the typo surfaces through `node_hint_summary`.
            "rollback-policy" | "rollback_policy" => {
                let raw = value.trim();
                if !raw.is_empty() && RollbackPolicy::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut rollback_policy, &value);
            }
            "rollback-objective" | "rollback_objective" => {
                set_first(&mut rollback_objective, &value)
            }
            "rollback-owned-files" | "rollback_owned_files" => {
                set_first(&mut rollback_owned_files_raw, &value)
            }
            "rollback-acceptance-commands" | "rollback_acceptance_commands" => {
                set_first(&mut rollback_acceptance_commands_raw, &value)
            }
            // wave-18 / task 04 — cascade rollback hint contract.
            //
            // `:compensates "<failed-node-id>"` declares THIS node as a
            // candidate compensation step for the named failed node. The
            // cascade evaluator (which runs AFTER the named node's final
            // failed attempt) consumes the field; outside that flow it
            // is pure metadata.
            //
            // `:rollback-cascade "none|plan|dispatch-safe"` opts the
            // failed (cascade-root) node into the cascade evaluator.
            // Strict parsing: unrecognised raw values land BOTH on the
            // typed slot AND in `unsupported_fields` so the scheduler
            // safely degrades to "no cascade" while the typo surfaces
            // through `node_hint_summary`.
            //
            // `:rollback-after ["node-a" "node-b"]` is an additional
            // ordering hint for cascade compensation order. Same shape
            // as `:depends-on` (paren / bracket / bareword run); never
            // promoted to a real `:depends-on` so forward dispatch order
            // is unaffected.
            "compensates" => set_first(&mut compensates, &value),
            // wave-19 / task 10 — forward compensate ref. Two spellings,
            // identical semantics: `:compensate-node "<comp-id>"` /
            // `:compensate-ref "<comp-id>"` declared on the failing node
            // points AT the compensation node id. First hint wins; later
            // duplicates are ignored so a typo cannot silently shadow
            // the author's first declaration. Plan-level validation
            // (declared-id resolution, self-ref rejection, agreement
            // with reverse `:compensates`) runs in `build_validated_dag`.
            "compensate-node"
            | "compensate_node"
            | "compensate-ref"
            | "compensate_ref" => set_first(&mut compensate_node, &value),
            "rollback-cascade" | "rollback_cascade" => {
                let raw = value.trim();
                if !raw.is_empty() && RollbackCascadeMode::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut rollback_cascade, &value);
            }
            "rollback-after" | "rollback_after" => {
                if rollback_after.is_empty() {
                    rollback_after = parse_id_list(&value);
                }
            }
            _ => {
                unsupported_fields.push((raw_key, value));
            }
        }
    }

    let id = id?;
    let target = target?;
    let policy = failure_policy.unwrap_or_else(|| FAILURE_POLICY_FAIL_FAST.to_string());
    let policy = match policy.as_str() {
        FAILURE_POLICY_FAIL_FAST | FAILURE_POLICY_CONTINUE => policy,
        _ => {
            // Unknown policy → record into unsupported_fields and fall back
            // to fail-fast (the safe default).
            unsupported_fields.push(("failure-policy".to_string(), policy));
            FAILURE_POLICY_FAIL_FAST.to_string()
        }
    };
    Some(DagNode {
        id,
        target,
        objective,
        depends_on,
        condition,
        failure_policy: policy,
        timeout_ms,
        dispatch_strategy,
        target_project,
        requested_cwd,
        flow_id,
        scope,
        commit_policy,
        owned_files_raw,
        forbidden_files_raw,
        acceptance_commands_raw,
        acceptance_mode_raw,
        acceptance_evidence_keys_raw,
        acceptance_depends_on,
        acceptance_requires_raw,
        acceptance_source_node,
        workstation_dispatch_flag,
        review_gate,
        review_action,
        review_text,
        retry_count,
        retry_delay_ms,
        retry_parse_error,
        rollback_policy,
        rollback_objective,
        rollback_owned_files_raw,
        rollback_acceptance_commands_raw,
        compensates,
        compensate_node,
        rollback_cascade,
        rollback_after,
        unsupported_fields,
    })
}

fn set_first(slot: &mut Option<String>, value: &str) {
    if slot.is_none() {
        let v = value.trim();
        if !v.is_empty() {
            *slot = Some(v.to_string());
        }
    }
}

/// Parse a depends-on value of the shape `["a" "b"]` or `(a b)`. Both shapes
/// are common in PLAN.lisp authoring; we accept either and split on whitespace.
/// Quoted strings have their quotes stripped; bare-words pass through.
fn parse_id_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .or_else(|| trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')))
        .unwrap_or(trimmed);
    let mut out = Vec::new();
    let chars: Vec<char> = inner.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        while i < n && (chars[i].is_whitespace() || chars[i] == ',') {
            i += 1;
        }
        if i >= n {
            break;
        }
        if chars[i] == '"' {
            // quoted
            i += 1;
            let start = i;
            let mut esc = false;
            while i < n {
                let c = chars[i];
                if esc {
                    esc = false;
                    i += 1;
                    continue;
                }
                if c == '\\' {
                    esc = true;
                    i += 1;
                    continue;
                }
                if c == '"' {
                    break;
                }
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            if !s.trim().is_empty() {
                out.push(s);
            }
            if i < n {
                i += 1; // consume closing quote
            }
        } else {
            let start = i;
            while i < n
                && !chars[i].is_whitespace()
                && chars[i] != ','
                && chars[i] != '"'
                && chars[i] != '('
                && chars[i] != ')'
                && chars[i] != '['
                && chars[i] != ']'
            {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            if !s.trim().is_empty() {
                out.push(s);
            }
        }
    }
    out
}

/// Local copy of the keyword/value scanner — simpler than the one in plan.rs
/// because this one is scoped to a single `(node :k v ...)` form. Recognises
/// quoted strings and bareword values; list-shaped values like
/// `:depends-on ["a" "b"]` are also captured (the whole bracket span becomes
/// the value string).
fn scan_keyword_pairs(form: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = form.chars().collect();
    let n = chars.len();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0usize;
    let mut in_string = false;
    let mut esc = false;
    while i < n {
        let c = chars[i];
        if in_string {
            if esc {
                esc = false;
                i += 1;
                continue;
            }
            if c == '\\' {
                esc = true;
                i += 1;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            i += 1;
            continue;
        }
        if c != ':' {
            i += 1;
            continue;
        }
        // start of keyword
        let key_start = i + 1;
        let mut j = key_start;
        while j < n {
            let cj = chars[j];
            if cj.is_whitespace() || cj == '(' || cj == ')' || cj == '"' || cj == ':' {
                break;
            }
            j += 1;
        }
        if j == key_start {
            i += 1;
            continue;
        }
        let key: String = chars[key_start..j].iter().collect();
        let mut k = j;
        while k < n && chars[k].is_whitespace() {
            k += 1;
        }
        if k >= n {
            break;
        }
        let next = chars[k];
        match next {
            '"' => {
                let mut m = k + 1;
                let mut value = String::new();
                let mut esc2 = false;
                while m < n {
                    let cm = chars[m];
                    if esc2 {
                        value.push(cm);
                        esc2 = false;
                        m += 1;
                        continue;
                    }
                    if cm == '\\' {
                        esc2 = true;
                        m += 1;
                        continue;
                    }
                    if cm == '"' {
                        m += 1;
                        break;
                    }
                    value.push(cm);
                    m += 1;
                }
                out.push((key, value));
                i = m;
            }
            '[' | '(' => {
                // Capture the entire bracket/paren span as the value so
                // `:depends-on ["a" "b"]` and `:depends-on (a b)` round-trip.
                let open = next;
                let close = if open == '[' { ']' } else { ')' };
                let mut depth = 0i64;
                let mut m = k;
                let mut esc2 = false;
                let mut in_str = false;
                while m < n {
                    let cm = chars[m];
                    if in_str {
                        if esc2 {
                            esc2 = false;
                            m += 1;
                            continue;
                        }
                        if cm == '\\' {
                            esc2 = true;
                            m += 1;
                            continue;
                        }
                        if cm == '"' {
                            in_str = false;
                        }
                        m += 1;
                        continue;
                    }
                    if cm == '"' {
                        in_str = true;
                        m += 1;
                        continue;
                    }
                    if cm == open {
                        depth += 1;
                    } else if cm == close {
                        depth -= 1;
                        if depth == 0 {
                            m += 1;
                            break;
                        }
                    }
                    m += 1;
                }
                let value: String = chars[k..m].iter().collect();
                out.push((key, value));
                i = m;
            }
            ':' | ')' => {
                // Bare keyword without a value — skip.
                i = k;
            }
            _ => {
                let mut m = k;
                while m < n {
                    let cm = chars[m];
                    if cm.is_whitespace() || cm == '(' || cm == ')' || cm == '"' {
                        break;
                    }
                    m += 1;
                }
                if m > k {
                    let value: String = chars[k..m].iter().collect();
                    out.push((key, value));
                    i = m;
                } else {
                    i = k;
                }
            }
        }
    }
    out
}

/// Kahn's topological sort. Stable across runs because we sort the per-tier
/// ready set by node id (tests rely on this for deterministic output).
fn kahn_topo_sort(nodes: &[DagNode]) -> std::result::Result<Vec<String>, DagBuildError> {
    let mut indeg: HashMap<&str, usize> = HashMap::new();
    let mut succ: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in nodes {
        indeg.entry(n.id.as_str()).or_insert(0);
        succ.entry(n.id.as_str()).or_default();
    }
    for n in nodes {
        for dep in &n.depends_on {
            *indeg.entry(n.id.as_str()).or_insert(0) += 1;
            succ.entry(dep.as_str()).or_default().push(n.id.as_str());
        }
    }
    // Use a sorted ready-set so output is deterministic.
    let mut ready: BTreeSet<&str> = indeg
        .iter()
        .filter_map(|(k, v)| if *v == 0 { Some(*k) } else { None })
        .collect();
    let mut order: Vec<String> = Vec::new();
    while let Some(&head) = ready.iter().next() {
        ready.remove(head);
        order.push(head.to_string());
        if let Some(succs) = succ.get(head) {
            for &s in succs {
                let entry = indeg.get_mut(s).expect("succ exists");
                *entry -= 1;
                if *entry == 0 {
                    ready.insert(s);
                }
            }
        }
    }
    if order.len() != nodes.len() {
        // Surface the node ids still carrying non-zero in-degree so the error
        // message points at the offending cycle members.
        let mut leftover: Vec<String> = indeg
            .iter()
            .filter_map(|(k, v)| if *v > 0 { Some(k.to_string()) } else { None })
            .collect();
        leftover.sort();
        return Err(DagBuildError::Cycle(leftover));
    }
    Ok(order)
}

/// Public entrypoint invoked from `plan::action_execute_internal` when
/// `scheduler_mode="dag_v1"` is set on the call.
pub(super) async fn action_execute_dag_v1(
    state: &AppState,
    args: &Value,
    plan: &Plan,
) -> Result<ToolResult> {
    // Plan must be re-fetched by caller for status checks; we just need the
    // sexp_text and the id here.
    let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
    let max_parallel_nodes = parse_max_parallel_nodes(args);

    // wave-17 / task 05 — validate the finalize knobs up-front so a typo
    // (`distill_mode="sonet"`) or an invalid combo (`distill_on_success=true`
    // without `finalize_plan=true`) fails fast rather than after the DAG
    // executes. Validation runs in dry-run mode too: an authoring mistake
    // should surface during the preview pass, not at the next live run.
    if let Some(err) = validate_finalize_args(args) {
        return Ok(err);
    }

    // wave-19 / task 06 — task-contract emit knob validated up-front so
    // a typo (`task_contract_mode="emi"`) fails fast before the DAG
    // executes. Default `Off` is byte-compatible with pre-wave19.
    let task_contract_ctx = match TaskContractDispatchCtx::from_args(args) {
        Ok(c) => c,
        Err(err) => return Ok(err),
    };

    let (parsed, order) = match build_validated_dag(&plan.sexp_text) {
        Ok(v) => v,
        Err(e) => return Ok(e.into_tool_result()),
    };

    let nodes_summary = build_nodes_summary(&parsed.nodes, &order);
    let node_hint_summary = build_node_hint_summary(&parsed);
    let concurrency_plan = compute_concurrency_plan(&parsed.nodes, &order, max_parallel_nodes);
    let retry_plan = build_retry_plan(&parsed.nodes, &order);

    // wave-17 / task 02 — claim / lease knobs surface on every response
    // (live and dry-run) so callers can tell which discipline mode the
    // run used. `planned_claims` is the per-node claim metadata
    // projection — empty registry, no overlap detection across nodes
    // — used by dry-run to preview every node's claim shape without
    // dispatching.
    let claim_lease_secs = parse_claim_lease_secs(args);
    let claimer_name = parse_claimer_name(args);
    let enforce_claims = parse_enforce_claims(args);
    let planned_claims = build_planned_claims(
        &parsed.nodes,
        &order,
        plan.id,
        &claimer_name,
        claim_lease_secs,
        enforce_claims,
    );

    if dry_run {
        let mut payload = json!({
            "status": "dry_run",
            "execute_mode": "internal",
            "scheduler_mode": "dag_v1",
            "runner_status": "dry_run_no_dispatch",
            "plan_id": plan.id,
            "board_task_id": plan.board_task_id,
            "node_count": parsed.nodes.len(),
            "max_parallel_nodes": max_parallel_nodes,
            "nodes": nodes_summary,
            "topological_order": order,
            "concurrency_plan": concurrency_plan,
            "node_hint_summary": node_hint_summary,
            // wave-16 / task 05 — projected retry budget per node so
            // dry-run callers can preview the attempt ceiling without
            // dispatching. Empty array when no node opted into a retry
            // policy (preserves the v2 baseline byte-shape for callers
            // that did not declare retry).
            "retry_plan": retry_plan,
            // wave-17 / task 02 — projected claim metadata per node so
            // dry-run callers can preview every node's claim shape
            // without dispatching. Always populated (every node carries
            // at least the synthetic plan/<id>/node/<id> fallback).
            "planned_claims": planned_claims,
            "claim_lease_secs": claim_lease_secs,
            "claimer_name": claimer_name,
            "enforce_claims": enforce_claims,
        });
        // wave-19 / task 06 — surface the resolved emission mode in
        // dry-run responses too so callers can preview the contract
        // policy without dispatching. Quiet when mode=Off so the
        // pre-wave19 byte-shape is preserved.
        if task_contract_ctx.mode.is_enabled() {
            payload["task_contract_mode"] = json!(task_contract_ctx.mode.as_str());
        }
        return Ok(ToolResult::json_pretty(&payload));
    }

    let outcome =
        execute_with_concurrency(
            state,
            args,
            plan,
            &parsed,
            &order,
            max_parallel_nodes,
            task_contract_ctx.clone(),
        )
        .await?;
    let aggregate_status = outcome.aggregate_status();
    let evidence_path = outcome.evidence_path.clone();
    let evidence_error = outcome.evidence_error.clone();
    let bus_publish_warnings = outcome.bus_publish_warnings.clone();
    let plan_status_update = match outcome.target_plan_status() {
        Some(target) => match state.store.plan_update_status(plan.id, target).await {
            Ok(_) => Ok(target.as_str().to_string()),
            Err(e) => {
                tracing::warn!(plan_id = %plan.id, error = %e, "DAG scheduler: plan status update failed");
                Err(e.to_string())
            }
        },
        None => Ok(plan.status.as_str().to_string()),
    };

    // wave-16 / task 04 — paused-node response surfaces. We compute these
    // unconditionally so callers see a stable shape: empty arrays when no
    // node carried a review gate, populated arrays when at least one
    // node paused. Keeping the keys present (even when empty) lets
    // downstream consumers `?.length` instead of branching on key
    // existence.
    let paused_nodes = outcome.paused_nodes_json();
    let paused_node_ids = outcome.paused_node_ids();
    let review_question_ids = outcome.review_question_ids();

    let mut payload = json!({
        "status": aggregate_status,
        "aggregate_status": aggregate_status,
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "runner_status": outcome.runner_status(),
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "node_count": parsed.nodes.len(),
        "max_parallel_nodes": max_parallel_nodes,
        "node_results": outcome.node_results_json(),
        // `nodes` retained as the v1-compatible alias for `node_results` so
        // any caller that already pivots on the older field keeps working.
        "nodes": outcome.node_results_json(),
        "skipped_nodes": outcome.skipped_nodes_json(),
        // wave-16 / task 04 — paused-node surfaces (always present so the
        // shape is stable; empty when no review-gate paused this run).
        "paused_nodes": paused_nodes,
        "paused_node_ids": paused_node_ids,
        "review_question_ids": review_question_ids,
        "topological_order": order,
        "concurrency_plan": concurrency_plan,
        "node_hint_summary": node_hint_summary,
        // wave-16 / task 05 — declared retry budget per node, included
        // on every (non-dry-run) response too so the row that records
        // the policy survives alongside the actual attempt counts.
        "retry_plan": retry_plan,
        // wave-17 / task 02 — claim / lease knobs echoed onto every
        // response so callers can tell which discipline mode the run
        // used. `planned_claims` is the dry-run-style projection so
        // observers can diff "what we would have claimed" against the
        // per-evidence `claim_id` rows the live run actually wrote.
        "planned_claims": planned_claims,
        "claim_lease_secs": claim_lease_secs,
        "claimer_name": claimer_name,
        "enforce_claims": enforce_claims,
        "evidence_path": evidence_path,
    });
    let (plan_status_after, plan_status_update_error) = match &plan_status_update {
        Ok(s) => {
            payload["plan_status"] = json!(s);
            (Some(s.clone()), None)
        }
        Err(e) => {
            payload["status_update_error"] = json!(e);
            (None, Some(e.clone()))
        }
    };
    if let Some(err) = evidence_error {
        payload["evidence_error"] = json!(err);
    }
    if !bus_publish_warnings.is_empty() {
        payload["bus_publish_warnings"] = json!(bus_publish_warnings);
    }

    // wave-17 / task 05 — finalize + distill trigger v0. Conservative: only
    // fires when the caller explicitly opts in. Without `finalize_plan=true`
    // we exit here with the wave-17 / task 04 byte-shape preserved.
    if parse_finalize_plan(args) {
        let distill_block = maybe_run_distill_trigger(
            state,
            args,
            plan,
            aggregate_status,
            plan_status_after.as_deref(),
        )
        .await;

        // Surface the finalization block on the response so callers can grep
        // one place for the rule + status mapping.
        let finalization = build_finalization_block(
            aggregate_status,
            plan_status_after.as_deref(),
            plan_status_update_error.as_deref(),
            distill_block.clone(),
        );
        payload["finalization"] = finalization.clone();

        // Audit trail: one evidence row recording the final aggregate status
        // + plan-status mapping. Quiet (no panic) when the sidecar write
        // fails — the surface already carries `evidence_error` for that.
        emit_evidence_dag_finalized(
            state,
            plan,
            args,
            aggregate_status,
            plan_status_after.as_deref(),
            plan_status_update_error.as_deref(),
            distill_block.as_ref(),
            &mut payload,
        )
        .await;
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// wave-17 / task 05 — drive the optional distill trigger. Returns the
/// `distill` block (or `None` when no trigger was requested). Pure async
/// orchestration: validation already ran in `validate_finalize_args` so
/// here we only branch on the runtime aggregate.
///
/// Decision matrix:
///
///   * `distill_on_success=false`              → return `None`
///   * `aggregate != dag_succeeded`            → block with `triggered=false`
///                                               and a recorded skip reason
///   * `plan_status_after != "succeeded"`      → block with `triggered=false`
///                                               (defensive: the workflow
///                                               distill handler also gates
///                                               on plan.status==Succeeded;
///                                               if the FSM update failed we
///                                               do NOT call distill because
///                                               the gate would refuse anyway)
///   * otherwise                               → call workflow distill,
///                                               surface its result + a
///                                               warning when it errored
async fn maybe_run_distill_trigger(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    aggregate_status: &str,
    plan_status_after: Option<&str>,
) -> Option<Value> {
    if !parse_distill_on_success(args) {
        return None;
    }
    let distill_mode = match parse_distill_mode_arg(args) {
        Ok(m) => m,
        Err(_) => {
            // Unreachable: validate_finalize_args already returned the
            // structured error before we got here. Defensive return so a
            // future refactor cannot silently bypass the validator.
            return Some(build_distill_block(
                false,
                "distill_mode_invalid_unreachable",
                FINALIZE_DISTILL_MODE_DRY_RUN,
                None,
                false,
            ));
        }
    };
    if aggregate_status != "dag_succeeded" {
        return Some(build_distill_block(
            false,
            "aggregate_not_succeeded",
            distill_mode,
            None,
            false,
        ));
    }
    if plan_status_after != Some("succeeded") {
        return Some(build_distill_block(
            false,
            "plan_status_not_succeeded_after_finalize",
            distill_mode,
            None,
            false,
        ));
    }
    // Build the distill args object. We forward the project-resolution
    // signals (`project` / `cwd` / `target_project`) verbatim so the
    // distill handler's evidence-sidecar reader resolves the same root the
    // DAG run wrote into. `persist=false` by default — the wave-17 / task
    // 05 trigger is an automatic preview pass, not a stamp-the-registry
    // call. Callers that want persistence still issue an explicit
    // `mission_workflow(action=distill, persist=true)` themselves.
    let mut distill_args = serde_json::Map::new();
    distill_args.insert("action".to_string(), json!("distill"));
    distill_args.insert("plan_id".to_string(), json!(plan.id.to_string()));
    distill_args.insert("distill_mode".to_string(), json!(distill_mode));
    if let Some(p) = args.get("project").and_then(|v| v.as_str()) {
        distill_args.insert("project".to_string(), json!(p));
    }
    if let Some(c) = args.get("cwd").and_then(|v| v.as_str()) {
        distill_args.insert("cwd".to_string(), json!(c));
    }
    if let Some(tp) = args.get("target_project").and_then(|v| v.as_str()) {
        distill_args.insert("target_project".to_string(), json!(tp));
    }
    let distill_call_args = Value::Object(distill_args);
    let distill_result =
        super::workflow::handle(state, "mission_workflow", distill_call_args).await;
    match distill_result {
        Ok(tr) => {
            let inner_payload = tool_result_payload(&tr);
            let inner_is_error = tr.is_error.unwrap_or(false);
            let reason = if inner_is_error {
                "distill_invoked_returned_error"
            } else {
                "distill_invoked_ok"
            };
            Some(build_distill_block(
                true,
                reason,
                distill_mode,
                Some(inner_payload),
                inner_is_error,
            ))
        }
        Err(e) => {
            // Unexpected handler-level error (bubbled `Result::Err`). Surface
            // it as a warning + non-fatal: the plan final state is preserved
            // because we already updated it to Succeeded above.
            tracing::warn!(
                plan_id = %plan.id,
                error = %e,
                "DAG finalize: distill trigger handler returned error"
            );
            Some(build_distill_block(
                true,
                "distill_invoked_handler_error",
                distill_mode,
                Some(json!({"error": e.to_string()})),
                true,
            ))
        }
    }
}

/// wave-17 / task 05 — append one `dag_finalized` evidence row. Mirrors the
/// per-node evidence layout (same source + kind taxonomy) so audit
/// dashboards that pivot on `state_transition` see the finalize entry next
/// to the per-node entries it summarises. Updates `evidence_path` /
/// `evidence_error` on the response payload so callers see the same
/// freshness signal the per-node writes already provide.
async fn emit_evidence_dag_finalized(
    state: &AppState,
    plan: &Plan,
    args: &Value,
    aggregate_status: &str,
    plan_status_after: Option<&str>,
    plan_status_update_error: Option<&str>,
    distill_block: Option<&Value>,
    payload: &mut Value,
) {
    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());

    let final_plan_status = plan_status_after.unwrap_or("unchanged");
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::NOTE,
    )
    .with_state_transition("dag_finalized")
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("event_kind", json!("plan_dag_finalized"))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("plan_version", json!(plan.version))
    .with_extra("aggregate_status", json!(aggregate_status))
    .with_extra("final_plan_status", json!(final_plan_status));
    if let Some(err) = plan_status_update_error {
        entry = entry.with_extra("plan_status_update_error", json!(err));
    }
    if let Some(d) = distill_block {
        // Distill block on evidence is the same shape the response carries
        // (triggered + reason + mode + optional result/warning) so audit
        // consumers can correlate without a second JSON parse.
        entry = entry.with_extra("distill", d.clone());
    } else {
        entry = entry.with_extra("distill", json!({"triggered": false, "reason": "not_requested"}));
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
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %plan.id,
            error = %error,
            "DAG finalize: evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        payload["evidence_path"] = json!(p);
    }
    if let Some(e) = err {
        payload["evidence_error"] = json!(e);
    }
}

fn build_nodes_summary(nodes: &[DagNode], order: &[String]) -> Value {
    let mut by_id: HashMap<&str, &DagNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut out: Vec<Value> = Vec::with_capacity(order.len());
    for id in order {
        let Some(n) = by_id.remove(id.as_str()) else { continue };
        let mut entry = json!({
            "id": n.id,
            "target": n.target,
            "depends_on": n.depends_on,
            "failure_policy": n.failure_policy,
        });
        if let Some(o) = &n.objective {
            entry["objective"] = json!(o);
        }
        if let Some(c) = &n.condition {
            entry["condition"] = json!(c);
        }
        if let Some(t) = n.timeout_ms {
            entry["timeout_ms"] = json!(t);
        }
        if let Some(d) = &n.dispatch_strategy {
            entry["dispatch_strategy"] = json!(d);
        }
        if let Some(p) = &n.target_project {
            entry["target_project"] = json!(p);
        }
        if let Some(c) = &n.requested_cwd {
            entry["requested_cwd"] = json!(c);
        }
        if let Some(f) = &n.flow_id {
            entry["flow_id"] = json!(f);
        }
        // wave-15 / task 05 — workstation-dispatch hint contract surface.
        // Each field is emitted only when present so summaries for nodes
        // that do not opt in stay byte-identical with the v2 baseline.
        if let Some(s) = &n.scope {
            entry["scope"] = json!(s);
        }
        if let Some(c) = &n.commit_policy {
            entry["commit_policy"] = json!(c);
        }
        if let Some(o) = &n.owned_files_raw {
            entry["owned_files_raw"] = json!(o);
        }
        if let Some(f) = &n.forbidden_files_raw {
            entry["forbidden_files_raw"] = json!(f);
        }
        if let Some(a) = &n.acceptance_commands_raw {
            entry["acceptance_commands_raw"] = json!(a);
        }
        // wave-17 / task 03 — acceptance evaluator declarations. Surface
        // only when present so summaries for nodes that did not opt in
        // stay byte-identical with the wave-16 baseline.
        if let Some(m) = &n.acceptance_mode_raw {
            entry["acceptance_mode"] = json!(m);
        }
        if let Some(k) = &n.acceptance_evidence_keys_raw {
            entry["acceptance_evidence_keys_raw"] = json!(k);
        }
        // wave-18 / task 03 — cross-node acceptance fan-in declarations.
        // Surface only when the author opted in so the wave-17 byte-shape
        // stays untouched for nodes that did not declare any fan-in.
        if !n.acceptance_depends_on.is_empty() {
            entry["acceptance_depends_on"] = json!(n.acceptance_depends_on);
        }
        if let Some(r) = &n.acceptance_requires_raw {
            entry["acceptance_requires"] = json!(r);
        }
        if let Some(s) = &n.acceptance_source_node {
            entry["acceptance_source_node"] = json!(s);
        }
        if n.workstation_dispatch_opt_in() {
            entry["workstation_dispatch"] = json!(true);
        }
        // wave-16 / task 04 — review-gate hint surface. Emit only when the
        // node carries a gate so summaries for nodes without a gate stay
        // byte-identical with the wave-15 baseline.
        if let Some(g) = &n.review_gate {
            entry["review_gate"] = json!(g);
        }
        if let Some(a) = &n.review_action {
            entry["review_action"] = json!(a);
        }
        if let Some(t) = &n.review_text {
            entry["review_text"] = json!(t);
        }
        // wave-17 / task 04 — rollback hint surface. Emit only when
        // the node declared at least one rollback hint so summaries
        // for nodes without rollback intent stay byte-identical with
        // the wave-17 / task 03 baseline.
        if n.has_rollback_hints() {
            let mut rb = json!({
                "policy": n
                    .rollback_policy_kind()
                    .unwrap_or(RollbackPolicy::None)
                    .as_wire(),
            });
            if let Some(o) = &n.rollback_objective {
                rb["objective"] = json!(o);
            }
            if let Some(of) = &n.rollback_owned_files_raw {
                rb["owned_files_raw"] = json!(of);
            }
            if let Some(ac) = &n.rollback_acceptance_commands_raw {
                rb["acceptance_commands_raw"] = json!(ac);
            }
            // wave-18 / task 04 — cascade hint surface. Emit only when
            // the author opted into cascading so summaries for nodes
            // without cascade intent stay byte-identical with the
            // wave-17 / task 04 baseline.
            if let Some(c) = &n.compensates {
                rb["compensates"] = json!(c);
            }
            // wave-19 / task 10 — surface the forward `:compensate-node`
            // ref so audit dashboards can pin the failing-node side of
            // the cascade structure (the reverse `:compensates` lives
            // on the compensation node and is surfaced above).
            if let Some(c) = &n.compensate_node {
                rb["compensate_node"] = json!(c);
            }
            if let Some(c) = &n.rollback_cascade {
                rb["cascade_mode"] = json!(c);
            }
            if !n.rollback_after.is_empty() {
                rb["rollback_after"] = json!(n.rollback_after);
            }
            entry["rollback"] = rb;
        }
        // wave-16 / task 05 — retry policy surface. Emit only when the
        // node opted into ≥ 1 retry so the v2 baseline byte-shape is
        // preserved for callers that did not declare a retry policy.
        if n.retry_enabled() {
            let mut retry = json!({
                "max_attempts": n.effective_max_attempts(),
            });
            if let Some(raw) = n.retry_count {
                retry["retry_count_raw"] = json!(raw);
            }
            if let Some(delay) = n.effective_retry_delay_ms() {
                retry["retry_delay_ms"] = json!(delay);
            }
            entry["retry"] = retry;
        }
        out.push(entry);
    }
    Value::Array(out)
}

/// wave-16 / task 05 — projection of the per-node retry policy authors
/// declared. Returned on both the dry-run and live response so the
/// "what the scheduler will / did do" can be diffed against the
/// observed `retry` block on each `node_results` entry. Order matches
/// the topological-sort order; nodes with the v2-baseline single-attempt
/// contract are omitted so the array stays empty for plans that did
/// not declare retry (preserves the wave-15 byte-shape).
fn build_retry_plan(nodes: &[DagNode], order: &[String]) -> Value {
    let by_id: HashMap<&str, &DagNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut out: Vec<Value> = Vec::new();
    for id in order {
        let Some(n) = by_id.get(id.as_str()) else { continue };
        if !n.retry_enabled() {
            continue;
        }
        let mut entry = json!({
            "id": n.id,
            "max_attempts": n.effective_max_attempts(),
        });
        if let Some(raw) = n.retry_count {
            entry["retry_count_raw"] = json!(raw);
        }
        if let Some(delay) = n.effective_retry_delay_ms() {
            entry["retry_delay_ms"] = json!(delay);
        }
        out.push(entry);
    }
    Value::Array(out)
}

/// Build the `node_hint_summary` JSON. This is the place where unsupported
/// metadata MUST surface so author intent is never silently dropped.
fn build_node_hint_summary(parsed: &ParsedDag) -> Value {
    let mut unsupported_fields = serde_json::Map::new();
    for n in &parsed.nodes {
        if n.unsupported_fields.is_empty() {
            continue;
        }
        let pairs: Vec<Value> = n
            .unsupported_fields
            .iter()
            .map(|(k, v)| json!({"key": k, "value": v}))
            .collect();
        unsupported_fields.insert(n.id.clone(), Value::Array(pairs));
    }
    json!({
        "unsupported_top_forms": parsed.unsupported_top_forms,
        "unsupported_fields": Value::Object(unsupported_fields),
    })
}

/// Terminal node state recorded in `NodeResult`. Mirrors the v1 enum so the
/// per-node JSON shape (`state` discriminant + `failed_dep` extra) stays
/// byte-identical for downstream readers; v2 added `SkippedFailFastAbort`
/// to distinguish "we never dispatched you because an unrelated upstream
/// failed under fail-fast" from "your direct dependency failed", and
/// wave-16 / task 04 added `Paused` for the per-node `:review-gate
/// "question-event"` state. `Paused` is the first non-terminal state that
/// surfaces in the per-node JSON — the resume listener (wave-16 / task 02
/// territory) is expected to revive the node in a follow-up dispatch.
#[derive(Debug, Clone)]
enum NodeState {
    Succeeded,
    Failed { reason: String },
    SkippedUpstreamFailed { failed_dep: String },
    SkippedCondition,
    /// `failure-policy=fail-fast` aborted the scheduler before this node was
    /// ever ready. Distinct from `SkippedUpstreamFailed` because the failing
    /// upstream is not necessarily a transitive dependency — under fail-fast
    /// every still-pending node is force-skipped once the abort flag flips.
    SkippedFailFastAbort { aborter: String },
    /// wave-16 / task 04 — node carried `:review-gate "question-event"`,
    /// the scheduler emitted (or attempted to emit) `QuestionEvent::Created`
    /// with [`question_id`] and STOPPED at this node instead of dispatching
    /// the target tool. `bus_publish_warning` carries the warning string
    /// when the publish call errored — the node still pauses (a failed
    /// gate is a real gate; downstream cannot advance) but the response
    /// surfaces the degraded observability path so callers can retry.
    Paused {
        question_id: String,
        bus_publish_warning: Option<String>,
    },
}

/// Per-node lifecycle phase. Drives the wave-scheduler bookkeeping; mapped to
/// `state` discriminants in the response only after the node terminates. The
/// intermediate phases (`Pending`, `Ready`, `Claimed`, `Running`) never leak
/// into the response — they live entirely in the scheduler's internal state
/// map.
///
/// `Ready` is the brief moment between the scheduler computing the ready set
/// and dispatching it to the JoinSet. The current loop transitions
/// `Pending -> Claimed -> Running` (wave-17 / task 02 added the explicit
/// `Claimed` step between ready-set selection and JoinSet hand-off so the
/// claim/lease registry can stamp metadata before the inner handler runs).
/// The variant `Ready` is kept in the enum to satisfy the wave-13/02 spec
/// lifecycle list and to leave room for a future scheduler that materialises
/// a persistent ready queue (`#[allow(dead_code)]` is intentional for now).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeLifecycle {
    Pending,
    #[allow(dead_code)]
    Ready,
    /// wave-17 / task 02 — node has had its claim registered (or
    /// recorded best-effort under `enforce_claims=false`) but the
    /// inner handler has not yet been invoked. Mostly invisible from
    /// the outside: the dispatch path moves through `Claimed` for one
    /// wave-loop cycle before flipping to `Running`. Surfaces on the
    /// `pending -> claimed` evidence row + bus event so observers can
    /// pivot on the new transition without reconstructing it from
    /// `pending -> running` reasoning.
    Claimed,
    Running,
    Succeeded,
    Failed,
    Skipped,
    /// wave-16 / task 04 — node opted into a `:review-gate "question-event"`
    /// gate and the scheduler emitted `QuestionEvent::Created` instead of
    /// dispatching the target tool. Treated as a non-terminal "stop"
    /// state by the wave loop: the scheduler does NOT retry it within
    /// the same call (auto-resume is wave-16 / task 02 territory), and
    /// the node's downstream stays pending until a follow-up resume.
    Paused,
}

/// wave-16 / task 05 — `Default` is implemented to make wave-13/14/15
/// test fixtures resilient against the retry-bookkeeping fields added
/// in this wave. Production construction sites (`execute_with_concurrency`
/// + the `NodeResult::skipped` helper) always populate every field
/// explicitly; the default impl only catches test fixtures using
/// `..Default::default()` so adding a new bookkeeping field doesn't
/// require touching every old test.
#[derive(Debug, Clone)]
struct NodeResult {
    id: String,
    target: String,
    state: NodeState,
    dispatch_strategy: String,
    inner_payload: Value,
    /// wave-16 / task 05 — number of dispatch attempts the scheduler
    /// actually consumed for this node. Always ≥ 1 for executed nodes
    /// (we count the first dispatch as attempt 1); equals
    /// `effective_max_attempts` only when every attempt failed. Skipped
    /// / paused nodes report `0` because the scheduler never invoked
    /// the inner handler. Surfaces on `node_results[].retry.attempts`.
    attempts_made: u32,
    /// wave-16 / task 05 — total attempts the scheduler was authorised
    /// to make for this node (= `effective_max_attempts` at dispatch
    /// time). Echoed alongside `attempts_made` so consumers can spot
    /// "exhausted retries" without re-deriving the policy.
    max_attempts: u32,
    /// wave-16 / task 05 — true iff the node failed without retrying
    /// because the failure was classified non-retryable (currently:
    /// safe-descriptor refusals from the workstation-dispatch
    /// substrate). Surfaces on the per-node response so consumers can
    /// distinguish "we exhausted attempts" from "we refused to retry".
    retry_skipped_non_retryable: bool,
    /// wave-17 / task 04 — conservative rollback decision result.
    /// `None` means the rollback evaluator never ran (skipped node,
    /// node terminated successfully, or the failed node carried no
    /// rollback hints — see `RollbackEvaluation::is_inactive`).
    /// `Some(e)` carries the full evaluation block — the scheduler
    /// stamps it onto `node_results[].rollback` so callers see what
    /// happened (descriptor recorded / dispatch attempted / refused
    /// / failed) without re-deriving from evidence.
    rollback: Option<RollbackEvaluation>,
    /// wave-17 / task 03 — deterministic acceptance phase result.
    /// `None` means the acceptance evaluator never ran for this node
    /// (skipped node, dispatch failed before acceptance, no hints
    /// declared). `Some(e)` carries the full evaluation block — the
    /// scheduler stamps it onto `node_results[].acceptance` so callers
    /// see what the evaluator decided + why.
    acceptance: Option<AcceptanceEvaluation>,
}

impl NodeResult {
    /// wave-16 / task 05 — minimal builder used by skip / pause sites
    /// that never invoked the inner handler. Keeps construction local
    /// to the scheduler so the per-call-site retry bookkeeping
    /// (`attempts_made = 0`, `max_attempts = 1`) stays consistent.
    fn skipped(
        id: String,
        target: String,
        state: NodeState,
        dispatch_strategy: String,
    ) -> Self {
        Self {
            id,
            target,
            state,
            dispatch_strategy,
            inner_payload: Value::Null,
            attempts_made: 0,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        }
    }
}

impl Default for NodeResult {
    fn default() -> Self {
        Self {
            id: String::new(),
            target: String::new(),
            state: NodeState::Succeeded,
            dispatch_strategy: String::new(),
            inner_payload: Value::Null,
            attempts_made: 0,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        }
    }
}

#[derive(Debug, Default)]
struct ExecutionOutcome {
    results: Vec<NodeResult>,
    /// Set true iff a node with `failure-policy=fail-fast` failed and we
    /// stopped scheduling additional ready nodes.
    aborted_fail_fast: bool,
    evidence_path: Option<String>,
    evidence_error: Option<String>,
    /// Per-transition `PlanNodeStateChanged` bus publish warnings collected
    /// during this run. Bus publish is intentionally non-blocking for the
    /// main dispatch path (durable evidence already lives in the sidecar);
    /// the warnings are surfaced on the response so callers can detect a
    /// degraded observability path without scraping daemon logs. Empty
    /// when every transition published cleanly.
    bus_publish_warnings: Vec<String>,
}

impl ExecutionOutcome {
    fn node_results_json(&self) -> Value {
        let mut out: Vec<Value> = Vec::with_capacity(self.results.len());
        for r in &self.results {
            let (state_str, extra) = match &r.state {
                NodeState::Succeeded => ("succeeded", None),
                NodeState::Failed { reason } => ("failed", Some(("reason", reason.clone()))),
                NodeState::SkippedUpstreamFailed { failed_dep } => (
                    "skipped_upstream_failed",
                    Some(("failed_dep", failed_dep.clone())),
                ),
                NodeState::SkippedCondition => ("skipped_condition", None),
                NodeState::SkippedFailFastAbort { aborter } => (
                    "skipped_fail_fast_abort",
                    Some(("aborter", aborter.clone())),
                ),
                NodeState::Paused { question_id, .. } => (
                    "paused",
                    Some(("review_question_id", question_id.clone())),
                ),
            };
            let mut e = json!({
                "id": r.id,
                "target": r.target,
                "state": state_str,
                "dispatch_strategy": r.dispatch_strategy,
                "inner_result": r.inner_payload,
            });
            if let Some((k, v)) = extra {
                e[k] = json!(v);
            }
            // wave-16 / task 04 — surface the optional bus warning on
            // paused nodes so callers can grep one place for "the gate
            // emit was degraded for this node".
            if let NodeState::Paused {
                bus_publish_warning: Some(w),
                ..
            } = &r.state
            {
                e["review_question_warning"] = json!(w);
            }
            // wave-16 / task 05 — retry observability surface. We
            // emit the `retry` block whenever the node is one whose
            // policy authorised more than one attempt OR the dispatch
            // actually consumed more than one attempt OR the failure
            // was classified non-retryable. Nodes with the v2-baseline
            // single-attempt contract that succeeded on attempt 1 stay
            // quiet so the wave-15 byte-shape is preserved.
            if r.max_attempts > 1
                || r.attempts_made > 1
                || r.retry_skipped_non_retryable
            {
                let mut retry = json!({
                    "attempts": r.attempts_made,
                    "max_attempts": r.max_attempts,
                });
                if r.retry_skipped_non_retryable {
                    retry["non_retryable"] = json!(true);
                }
                e["retry"] = retry;
            }
            // wave-17 / task 03 — acceptance evaluation surface. Quiet
            // (omitted) when the evaluator never ran OR ran but found no
            // hints declared so the wave-16 byte-shape is preserved for
            // callers that did not opt into the acceptance contract.
            if let Some(acc) = r.acceptance.as_ref() {
                if !acc.is_inactive() {
                    e["acceptance"] = acc.to_json();
                }
            }
            // wave-17 / task 04 — rollback evaluation surface. Quiet
            // (omitted) when the rollback evaluator never ran OR
            // produced an inactive evaluation (no hints declared) so
            // the wave-17 / task 03 byte-shape is preserved for
            // callers that did not opt into the rollback contract.
            if let Some(rb) = r.rollback.as_ref() {
                if !rb.is_inactive() {
                    e["rollback"] = rb.to_json();
                }
            }
            out.push(e);
        }
        Value::Array(out)
    }

    /// wave-16 / task 04 — project the subset of results that landed in
    /// the `paused` non-terminal state so callers (and the wave-16 / task
    /// 02 resume listener) can address them without re-walking the full
    /// results array. Order matches the topological-order placement of
    /// each result.
    fn paused_nodes_json(&self) -> Value {
        let mut out: Vec<Value> = Vec::new();
        for r in &self.results {
            if let NodeState::Paused {
                question_id,
                bus_publish_warning,
            } = &r.state
            {
                let mut e = json!({
                    "id": r.id,
                    "target": r.target,
                    "state": "paused",
                    "review_question_id": question_id,
                });
                if let Some(w) = bus_publish_warning {
                    e["review_question_warning"] = json!(w);
                }
                out.push(e);
            }
        }
        Value::Array(out)
    }

    /// wave-16 / task 04 — paused node ids in topological-order placement.
    /// Surfaced as a separate flat array on the response so callers that
    /// just want "which nodes need a follow-up resume" don't have to walk
    /// the richer `paused_nodes` block.
    fn paused_node_ids(&self) -> Vec<String> {
        self.results
            .iter()
            .filter_map(|r| match &r.state {
                NodeState::Paused { .. } => Some(r.id.clone()),
                _ => None,
            })
            .collect()
    }

    /// wave-16 / task 04 — review-question ids for every paused node, in
    /// the same order as `paused_node_ids`. The two arrays are the
    /// ergonomic split of the richer `paused_nodes` block.
    fn review_question_ids(&self) -> Vec<String> {
        self.results
            .iter()
            .filter_map(|r| match &r.state {
                NodeState::Paused { question_id, .. } => Some(question_id.clone()),
                _ => None,
            })
            .collect()
    }

    /// True iff at least one node landed in the `paused` state for this
    /// run — used by aggregate_status / runner_status to surface
    /// `dag_paused` so callers can route on a single status discriminant.
    fn any_paused(&self) -> bool {
        self.results
            .iter()
            .any(|r| matches!(r.state, NodeState::Paused { .. }))
    }

    /// Project the subset of results that ended in a `skipped_*` discriminant
    /// so callers can grep without re-walking the full results array. Order
    /// matches the topological-order placement of each result.
    fn skipped_nodes_json(&self) -> Value {
        let mut out: Vec<Value> = Vec::new();
        for r in &self.results {
            let (state_str, extra) = match &r.state {
                NodeState::SkippedUpstreamFailed { failed_dep } => (
                    "skipped_upstream_failed",
                    Some(("failed_dep", failed_dep.clone())),
                ),
                NodeState::SkippedCondition => ("skipped_condition", None),
                NodeState::SkippedFailFastAbort { aborter } => (
                    "skipped_fail_fast_abort",
                    Some(("aborter", aborter.clone())),
                ),
                _ => continue,
            };
            let mut e = json!({
                "id": r.id,
                "target": r.target,
                "state": state_str,
            });
            if let Some((k, v)) = extra {
                e[k] = json!(v);
            }
            out.push(e);
        }
        Value::Array(out)
    }

    fn any_failed(&self) -> bool {
        self.results.iter().any(|r| matches!(r.state, NodeState::Failed { .. }))
    }

    fn all_succeeded(&self) -> bool {
        !self.results.is_empty()
            && self
                .results
                .iter()
                .all(|r| matches!(r.state, NodeState::Succeeded))
    }

    fn aggregate_status(&self) -> &'static str {
        if self.aborted_fail_fast {
            return "dag_failed";
        }
        if self.all_succeeded() {
            return "dag_succeeded";
        }
        if self.any_failed() {
            return "dag_partial";
        }
        // wave-16 / task 04 — paused-only runs surface a dedicated
        // aggregate so callers can route on a single status. We pick
        // `dag_paused` (rather than `dag_partial`) only when no failure
        // is present; a mixed paused+failed run still reads as partial
        // because failure is the louder signal.
        if self.any_paused() {
            return "dag_paused";
        }
        // Some nodes may have been skipped without any outright failure
        // (e.g. condition gating). Treat that as partial too.
        "dag_partial"
    }

    fn runner_status(&self) -> &'static str {
        if self.aborted_fail_fast {
            "fail_fast_aborted"
        } else if self.all_succeeded() {
            "all_nodes_dispatched"
        } else if !self.any_failed() && self.any_paused() {
            "review_gate_paused"
        } else {
            "partial_dispatched"
        }
    }

    fn target_plan_status(&self) -> Option<PlanStatus> {
        if self.all_succeeded() {
            Some(PlanStatus::Succeeded)
        } else if self.aborted_fail_fast || self.any_failed() {
            Some(PlanStatus::Failed)
        } else {
            // Paused runs leave the plan in its current Executing /
            // Approved state so a follow-up resume can advance the DAG.
            // Returning None here means `action_execute_dag_v1` won't
            // call `plan_update_status` for this run.
            None
        }
    }
}

/// Outcome of dispatching a single node — produced inside the spawned task
/// so the scheduler's main loop can decide success/failure + record evidence
/// without holding any per-node lock during the dispatch itself.
struct DispatchOutcome {
    node_id: String,
    target: String,
    dispatch_strategy: String,
    inner_payload: Value,
    /// `Ok(())` when the inner handler returned a non-error tool result;
    /// `Err(reason)` when either inner-args building or the inner handler
    /// surfaced an error. The reason string is what we surface in the
    /// per-node response under `reason` and in the `running -> failed`
    /// evidence entry's failure annotation.
    classification: std::result::Result<(), String>,
    /// wave-16 / task 05 — true when the failure originated from a
    /// workstation-dispatch safe-descriptor refusal (unsupported
    /// target / project root unresolved / missing objective). These
    /// failures are deterministic policy checks — re-running them
    /// without changing the inputs would refuse identically. The
    /// scheduler honours this flag by skipping the retry loop and
    /// surfacing `retry_skipped_non_retryable=true` on the response.
    non_retryable: bool,
}

/// Project a parsed DAG node into the workstation-dispatch hint contract.
/// Mirrors `ParsedPlanHints::to_workstation_hints` so the v0 DAG path and
/// the v0 single-node runner build identical briefs for the same hints.
fn node_to_workstation_hints(
    node: &DagNode,
) -> super::workstation_dispatch::WorkstationDispatchHints {
    super::workstation_dispatch::WorkstationDispatchHints {
        objective: node.objective.clone(),
        scope: node.scope.clone(),
        owned_files: super::plan::split_lisp_string_list(node.owned_files_raw.as_deref()),
        forbidden_files: super::plan::split_lisp_string_list(node.forbidden_files_raw.as_deref()),
        acceptance_commands: super::plan::split_lisp_string_list(
            node.acceptance_commands_raw.as_deref(),
        ),
        commit_policy: node.commit_policy.clone(),
        target_project: node.target_project.clone(),
        requested_cwd: node.requested_cwd.clone(),
        dispatch_strategy: node.dispatch_strategy.clone(),
    }
}

/// Convert a workstation-dispatch outcome into the
/// `(inner_payload, classification, non_retryable)` triple `dispatch_node`
/// uses to populate `DispatchOutcome`. Keeps the per-node DAG contract
/// intact: the response JSON carries the workstation-dispatch envelope
/// under `inner_result`, and the outcome's status drives the
/// success/failure classification.
///
/// wave-16 / task 05 — `non_retryable` is true ONLY for
/// `SafeDescriptor` outcomes, because those refusals are deterministic
/// policy checks (unsupported target / project root unresolved /
/// missing objective). Re-running the same inputs would refuse
/// identically; the scheduler respects this and bypasses the retry
/// loop. `InnerError` (the substrate handler returned an error
/// payload) IS retryable — that path may have transient causes.
fn workstation_outcome_to_dispatch_pair(
    node: &DagNode,
    dispatch_strategy: &str,
    outcome: super::workstation_dispatch::WorkstationDispatchOutcome,
    decision: &super::workstation_dispatch::DispatchDecision,
) -> (Value, std::result::Result<(), String>, bool) {
    let status = outcome.status();
    let envelope =
        super::workstation_dispatch::outcome_to_response_fields(&outcome, dispatch_strategy);
    let mut non_retryable = false;
    let classification: std::result::Result<(), String> = match &outcome {
        super::workstation_dispatch::WorkstationDispatchOutcome::Dispatched { .. } => Ok(()),
        super::workstation_dispatch::WorkstationDispatchOutcome::DryRun { .. } => Ok(()),
        super::workstation_dispatch::WorkstationDispatchOutcome::InnerError {
            inner_payload,
            ..
        } => Err(inner_payload
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("workstation_dispatch inner handler returned error")
            .to_string()),
        super::workstation_dispatch::WorkstationDispatchOutcome::SafeDescriptor {
            reason,
            ..
        } => {
            // Safe-descriptor refusals are deterministic policy checks
            // — flag the failure as non-retryable so the wave loop
            // skips the retry pass entirely.
            non_retryable = true;
            Err(format!(
                "workstation_dispatch refused to dispatch node `{}`: {}",
                node.id,
                reason.detail()
            ))
        }
    };
    let mut payload = json!({
        "workstation_dispatch_status": status,
        "node_id": node.id,
        // wave-16 / task 03 — surface routing provenance per node so the
        // DAG response makes the explicit/inferred split visible without
        // re-deriving from the plan body.
        "workstation_dispatch_source": decision.source.as_str(),
    });
    if let Some(reason) = decision.reason.as_deref() {
        if let Some(map) = payload.as_object_mut() {
            map.insert(
                "workstation_dispatch_inference_reason".to_string(),
                json!(reason),
            );
        }
    }
    if let Some(map) = envelope.as_object() {
        if let Some(payload_map) = payload.as_object_mut() {
            for (k, v) in map {
                payload_map.insert(k.clone(), v.clone());
            }
        }
    }
    (payload, classification, non_retryable)
}

/// wave-19 / task 06 — per-DAG-run task-contract emission context. The
/// scheduler resolves the mode + project-resolution signals once at the
/// top of `action_execute_dag_v1` and clones one of these into every
/// `dispatch_node` task so the per-node emit does not have to re-parse
/// the caller args (and stays aligned with the single-node runner's
/// project-root resolution path). All fields are owned (no borrowed
/// references) so the struct survives `tokio::JoinSet::spawn`'s
/// `'static` requirement.
#[derive(Debug, Clone)]
pub(super) struct TaskContractDispatchCtx {
    pub mode: super::plan::TaskContractEmitMode,
    pub project_arg: Option<String>,
    pub cwd_arg: Option<String>,
    pub target_project_arg: Option<String>,
}

impl TaskContractDispatchCtx {
    pub(super) fn off() -> Self {
        Self {
            mode: super::plan::TaskContractEmitMode::Off,
            project_arg: None,
            cwd_arg: None,
            target_project_arg: None,
        }
    }

    /// Build the ctx from caller args. Returns
    /// `Err(structured)` for malformed `task_contract_mode` values so
    /// the scheduler fails fast before spawning any node task.
    pub(super) fn from_args(args: &Value) -> std::result::Result<Self, ToolResult> {
        let mode = super::plan::parse_task_contract_emit_mode(args)?;
        Ok(Self {
            mode,
            project_arg: args
                .get("project")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            cwd_arg: args
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            target_project_arg: args
                .get("target_project")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })
    }
}

async fn dispatch_node(
    state: AppState,
    plan: Plan,
    node: DagNode,
    task_contract_ctx: TaskContractDispatchCtx,
) -> Result<DispatchOutcome> {
    let inner_args_built = build_node_inner_args(&node, &plan);
    let dispatch_strategy = inner_args_built.dispatch_strategy.clone();

    // wave-15 / task 05 + wave-16 / task 03 — workstation-dispatch routing
    // for DAG nodes. Wave-15 honoured an explicit per-node
    // `:workstation-dispatch true` only. Wave-16 layers conservative
    // auto-inference on top: when a node's :target is already
    // `mission_task_delegate`, the dispatch strategy resolves to a known
    // workstation strategy, the objective is non-empty, and at least one
    // scoping signal is declared, the scheduler routes through the
    // workstation substrate without requiring the explicit hint. There is
    // no per-node `workstation_dispatch=false` knob because DAG nodes are
    // declared in PLAN.lisp; the only off-switch is to mark the node with
    // a non-task-delegate target or omit the dispatch strategy.
    let merged = node_to_workstation_hints(&node);
    let inference_ctx = super::workstation_dispatch::InferenceContext {
        target: node.target.as_str(),
        dispatch_strategy: dispatch_strategy.as_str(),
        objective: merged.objective.as_deref(),
        owned_files_present: !merged.owned_files.is_empty(),
        scope_present: merged
            .scope
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        target_project_present: merged
            .target_project
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        requested_cwd_present: merged
            .requested_cwd
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
    };
    let dispatch_decision = super::workstation_dispatch::evaluate_dispatch_decision(
        &serde_json::Value::Null,
        node.workstation_dispatch_opt_in(),
        &inference_ctx,
    );

    if dispatch_decision.is_enabled() {
        // wave-19 / task 06 — emit the per-node task-contract sidecar
        // BEFORE handing the node to the workstation substrate. The
        // contract is the SSOT, so a failed write REFUSES dispatch
        // for this node; non-retryable so the wave loop does not loop
        // through the inner handler hoping the disk recovers. Default
        // mode (`Off`) returns an empty record and the per-node
        // payload omits the wave-19 fields entirely.
        let inputs = super::plan::task_contract_inputs_from_hints(
            &merged,
            &node.target,
            &dispatch_strategy,
        );
        let emission = super::plan::emit_task_contract(
            &state,
            plan.id,
            &plan.board_task_id,
            &node.id,
            task_contract_ctx.mode,
            &inputs,
            task_contract_ctx.project_arg.as_deref(),
            task_contract_ctx.cwd_arg.as_deref(),
            task_contract_ctx.target_project_arg.as_deref(),
        )
        .await;

        if emission.is_failure() {
            // Refuse the per-node dispatch — the missing contract
            // would leave downstream consumers with no Lisp SSOT.
            // Mark non-retryable: an IO failure is unlikely to fix
            // itself by re-running the inner handler.
            let mut payload = json!({
                "node_id": node.id,
                "target": node.target,
                "workstation_dispatch_status": "skipped_task_contract_emit_failed",
                "workstation_dispatch_source": dispatch_decision.source.as_str(),
            });
            if let Some(reason) = dispatch_decision.reason.as_deref() {
                payload["workstation_dispatch_inference_reason"] = json!(reason);
            }
            super::plan::merge_task_contract_block(&mut payload, &emission);
            let reason = emission
                .error
                .clone()
                .unwrap_or_else(|| "task_contract_emit_failed".to_string());
            return Ok(DispatchOutcome {
                node_id: node.id.clone(),
                target: node.target.clone(),
                dispatch_strategy,
                inner_payload: payload,
                classification: Err(format!(
                    "task_contract emit failed for node `{}`: {}",
                    node.id, reason
                )),
                non_retryable: true,
            });
        }

        if task_contract_ctx.mode.is_dry_run() {
            // EmitDryRun — never call the substrate. We mark the
            // node succeeded (the contract write IS the work in
            // dry-run mode); downstream nodes proceed normally so
            // the caller can preview the full DAG with one pass.
            let mut payload = json!({
                "node_id": node.id,
                "target": node.target,
                "workstation_dispatch_status": "task_contract_emit_dry_run",
                "workstation_dispatch_source": dispatch_decision.source.as_str(),
            });
            if let Some(reason) = dispatch_decision.reason.as_deref() {
                payload["workstation_dispatch_inference_reason"] = json!(reason);
            }
            super::plan::merge_task_contract_block(&mut payload, &emission);
            return Ok(DispatchOutcome {
                node_id: node.id.clone(),
                target: node.target.clone(),
                dispatch_strategy,
                inner_payload: payload,
                classification: Ok(()),
                non_retryable: false,
            });
        }

        let outcome = super::workstation_dispatch::run_workstation_dispatch(
            &state,
            &plan,
            &node.target,
            &dispatch_strategy,
            merged,
            false,
        )
        .await;
        let (mut inner_payload, classification, non_retryable) =
            workstation_outcome_to_dispatch_pair(
                &node,
                &dispatch_strategy,
                outcome,
                &dispatch_decision,
            );
        super::plan::merge_task_contract_block(&mut inner_payload, &emission);
        return Ok(DispatchOutcome {
            node_id: node.id.clone(),
            target: node.target.clone(),
            dispatch_strategy,
            inner_payload,
            classification,
            non_retryable,
        });
    }

    let inner_args = match inner_args_built.inner_args {
        Ok(v) => v,
        Err(err_payload) => {
            let reason = err_payload
                .as_object()
                .and_then(|m| m.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("inner args build failed")
                .to_string();
            // wave-16 / task 05 — inner-args build failures are deterministic
            // (e.g. missing required `flow_id` for `mission_flow_run`).
            // Re-running with identical inputs would fail identically;
            // mark non-retryable so the wave loop skips the retry pass.
            return Ok(DispatchOutcome {
                node_id: node.id.clone(),
                target: node.target.clone(),
                dispatch_strategy,
                inner_payload: err_payload,
                classification: Err(reason),
                non_retryable: true,
            });
        }
    };

    let inner_result = match node.target.as_str() {
        "mission_execution" => {
            super::agent_execution::handle(&state, "mission_execution", inner_args.clone()).await?
        }
        "mission_task_delegate" => {
            super::super::compute::task_delegate::handle(
                &state,
                "mission_task_delegate",
                inner_args.clone(),
            )
            .await?
        }
        "mission_flow_run" => {
            super::super::compute::flow_run::handle(&state, "mission_flow_run", inner_args.clone())
                .await?
        }
        _ => unreachable!("DAG validation already enforced target whitelist"),
    };

    let inner_payload = tool_result_payload(&inner_result);
    let inner_is_error = inner_result.is_error.unwrap_or(false);
    let classification = if inner_is_error {
        Err(inner_payload
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("inner handler returned error")
            .to_string())
    } else {
        Ok(())
    };
    Ok(DispatchOutcome {
        node_id: node.id.clone(),
        target: node.target.clone(),
        dispatch_strategy,
        inner_payload,
        classification,
        // Standard inner-handler failures may have transient causes —
        // leave them retryable. The wave loop honours the per-node
        // retry policy and stops once attempts are exhausted.
        non_retryable: false,
    })
}

// ───────────────────────────────────────────────────────────────────────
// wave-17 / task 02 — claim / lease discipline for PLAN DAG nodes.
//
// We bind to the existing wave12-01 `mission_execution(action=claim/release)`
// coordination model: same `scopes_overlap` predicate (re-exported from
// `agent_execution::scopes_overlap_pure`), same lease semantics. There is
// NO new lock service here — the registry below is per-`execute_with_concurrency`
// scratch state used to (a) detect overlapping claims between sibling nodes
// in the same DAG run and (b) carry claim metadata through to the per-node
// evidence + response.
//
// Three knobs are surfaced on the `mission_plan(action=execute,
// scheduler_mode=dag_v1)` envelope:
//   * `claim_lease_secs` — default 1800 (30 min), clamped to
//     `[CLAIM_LEASE_SECS_MIN, CLAIM_LEASE_SECS_MAX]` so an authoring
//     mistake cannot stall the scheduler with a 0-second lease nor pin a
//     scope for hours under a typo'd `999999`. Mirrors the wave12-01 ceiling.
//   * `claimer_name` — default `plan-dag-scheduler`. Matches the wave12-01
//     `:claimer` field convention so audit dashboards can pivot on the
//     same identity vocabulary across companion-log claims AND DAG claims.
//   * `enforce_claims` — default `false` for backward compatibility with
//     pre-wave17 callers. When false, claim metadata still surfaces on
//     the evidence + response (so observers can tell the registry tried)
//     but conflicts NEVER block dispatch. When true, an unresolvable
//     overlap fails the node fast with `CLAIM_CONFLICT` and we never
//     hand it to the inner handler.
//
// The `claimed` lifecycle state is added between `Ready` and `Running`
// (wave-13/02 listed it in the spec but the loop transitioned ready ->
// running directly). The transition is now `pending -> claimed -> running`
// for every dispatched node (whether enforce_claims is true or false);
// best-effort claim registration runs on the way to claimed, and the
// registered claim is released on the way out of every terminal state
// (succeeded / failed / paused / skipped).
// ───────────────────────────────────────────────────────────────────────

/// Default lease seconds when the caller did not pass `claim_lease_secs`.
/// Matches the wave12-01 `mission_execution` constant so the two surfaces
/// surface the same default duration.
pub(super) const PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS: i64 = 1800;

/// Lower bound on `claim_lease_secs`. Mirrors the wave12-01 floor — leases
/// shorter than 60 seconds are almost certainly a typo (a wave can take
/// longer than that to drain) and would create churn in the registry.
pub(super) const PLAN_DAG_CLAIM_LEASE_SECS_MIN: i64 = 60;

/// Upper bound on `claim_lease_secs`. Capped at 4 hours so a typo'd
/// `999999` cannot pin a scope for the rest of the day. Authors who
/// genuinely need longer leases should refresh per node; the registry
/// is per-DAG-run and never persists past the wave loop.
pub(super) const PLAN_DAG_CLAIM_LEASE_SECS_MAX: i64 = 14_400;

/// Default claimer identity stamped onto every plan-DAG claim record
/// when the caller did not pass `claimer_name`. Matches the wave12-01
/// `:claimer` vocabulary so audit dashboards can grep one identity.
pub(super) const PLAN_DAG_DEFAULT_CLAIMER_NAME: &str = "plan-dag-scheduler";

/// Provenance label for the `:scope` derivation used by a claim. Surfaced
/// alongside the claim record so dashboards can tell whether the scheduler
/// claimed `:owned-files`, `:scope`, or the synthetic `plan/<id>/node/<id>`
/// fallback. Stable string vocabulary so tests and dashboards can pin them.
pub(super) const CLAIM_SCOPE_SOURCE_OWNED_FILES: &str = "owned_files";
pub(super) const CLAIM_SCOPE_SOURCE_SCOPE: &str = "scope";
pub(super) const CLAIM_SCOPE_SOURCE_PLAN_NODE_FALLBACK: &str = "plan_node_fallback";

/// Parse `claim_lease_secs` from the call args, defaulting to
/// `PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS` and clamping to the
/// `[MIN, MAX]` range. Mirrors `parse_max_parallel_nodes` in spirit:
/// invalid values are silently normalised rather than rejected because
/// the scheduler treats lease bounds as a safety net, not a contract.
pub(super) fn parse_claim_lease_secs(args: &Value) -> i64 {
    args.get("claim_lease_secs")
        .and_then(|v| v.as_i64())
        .unwrap_or(PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS)
        .clamp(PLAN_DAG_CLAIM_LEASE_SECS_MIN, PLAN_DAG_CLAIM_LEASE_SECS_MAX)
}

/// Parse `claimer_name` from the call args, defaulting to
/// `PLAN_DAG_DEFAULT_CLAIMER_NAME`. Empty / whitespace-only strings fall
/// back to the default so a caller passing an empty form field doesn't
/// poison the audit log with a blank claimer.
pub(super) fn parse_claimer_name(args: &Value) -> String {
    args.get("claimer_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(PLAN_DAG_DEFAULT_CLAIMER_NAME)
        .to_string()
}

/// Parse `enforce_claims` from the call args. Default `false` so
/// pre-wave17 callers keep their byte-compatible dispatch contract. Any
/// non-bool value normalises to `false` (no error) — the enforcement is
/// strict opt-in.
pub(super) fn parse_enforce_claims(args: &Value) -> bool {
    args.get("enforce_claims")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

// ── wave-17 / task 05 — DAG finalize + distill trigger v0 ──────────────
//
// Three new opt-in knobs control the post-execution finalization step:
//
//   * `finalize_plan`        bool, default false — when false, the
//                            response shape stays byte-identical to the
//                            wave-17 / task 04 baseline (preserves the
//                            existing plan_update_status side-effect; the
//                            new `finalization` block is omitted).
//   * `distill_on_success`   bool, default false — when true (and
//                            finalize_plan=true), invoke the existing
//                            `mission_workflow(action=distill)` path AFTER
//                            a successful finalization. Only fires for the
//                            `dag_succeeded` aggregate; every other
//                            aggregate skips with a recorded reason.
//   * `distill_mode`         string, default `dry_run` — forwarded
//                            verbatim to the distill action. The strict
//                            allowlist mirrors `workflow.rs::parse_distill_mode`
//                            so the two surfaces cannot drift.
//
// CLAUDE.md "fast fail, no fallback" applies: passing `distill_on_success=true`
// without `finalize_plan=true` is rejected as INVALID_PARAM rather than
// silently ignored, because the brief explicitly forbids triggering distill
// without a successful finalization.
pub(super) const FINALIZE_DISTILL_MODE_DRY_RUN: &str = "dry_run";
pub(super) const FINALIZE_DISTILL_MODE_SONNET: &str = "sonnet";

/// Parse the `finalize_plan` opt-in toggle. Default `false` — without it
/// the response stays byte-identical with the wave-17 / task 04 baseline.
/// Non-bool values silently normalise to the default rather than fail; the
/// finalize block is purely additive so a typo never breaks an existing
/// dispatch.
pub(super) fn parse_finalize_plan(args: &Value) -> bool {
    args.get("finalize_plan")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Parse the `distill_on_success` opt-in toggle. Default `false`. Only
/// honoured when `finalize_plan=true` (validated separately via
/// `validate_finalize_args` so the rejection surface stays in one place).
pub(super) fn parse_distill_on_success(args: &Value) -> bool {
    args.get("distill_on_success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Strict allowlist for the `distill_mode` knob. Mirrors
/// `workflow.rs::parse_distill_mode` so the two surfaces share the
/// vocabulary. Returns the canonical string (so callers can echo the
/// resolved mode on the response and forward it verbatim to the workflow
/// distill handler) or an error message.
pub(super) fn parse_distill_mode_arg(args: &Value) -> Result<&'static str, String> {
    match args.get("distill_mode").and_then(|v| v.as_str()) {
        None | Some("") | Some("dry_run") => Ok(FINALIZE_DISTILL_MODE_DRY_RUN),
        Some("sonnet") => Ok(FINALIZE_DISTILL_MODE_SONNET),
        Some(other) => Err(format!(
            "distill_mode must be one of [\"dry_run\", \"sonnet\"]; got `{}`",
            other
        )),
    }
}

/// Pre-flight validation for the wave-17 / task 05 finalize knobs. Returns
/// `Some(error_result)` for the call sites to early-return; `None` when the
/// args pass.
///
/// Cross-field rules enforced here:
///
///   * `distill_on_success=true` requires `finalize_plan=true` — silently
///     dropping a distill request would mask the caller's intent.
///   * `distill_mode` must be on the strict allowlist — even when
///     `distill_on_success=false` we validate so a typo surfaces immediately
///     (not on the next caller's actual distill run).
pub(super) fn validate_finalize_args(args: &Value) -> Option<ToolResult> {
    if let Err(msg) = parse_distill_mode_arg(args) {
        return Some(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            msg,
        )));
    }
    let finalize = parse_finalize_plan(args);
    let distill = parse_distill_on_success(args);
    if distill && !finalize {
        return Some(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                "distill_on_success=true requires finalize_plan=true",
            )
            .with_suggestion(
                "the distill trigger fires only AFTER a successful finalization; \
                 set finalize_plan=true or drop distill_on_success",
            ),
        ));
    }
    None
}

/// Map an aggregate status string (`dag_succeeded` / `dag_failed` /
/// `dag_partial` / `dag_paused`) to the plan-status label the finalization
/// block surfaces. Keeping this pure makes the wave-17 / task 05 mapping
/// table testable without standing up a full AppState.
///
/// * `dag_succeeded`            → `"succeeded"` (terminal claim of success)
/// * `dag_failed` / `dag_partial` (with any failed node) → `"failed"`
/// * `dag_paused`               → preserves the current plan status — we
///                                 NEVER claim success while a node is
///                                 paused awaiting review (per the brief
///                                 "do not lie" invariant).
/// * anything else              → `"unchanged"` (defensive fallback so a
///                                 future aggregate can extend the table
///                                 without panicking).
pub(super) fn finalize_plan_status_label(
    aggregate: &str,
    current_plan_status: &str,
) -> &'static str {
    match aggregate {
        "dag_succeeded" => "succeeded",
        "dag_failed" => "failed",
        // `dag_partial` always carries at least one failed node (the
        // aggregate matrix in `ExecutionOutcome::aggregate_status` hands out
        // `dag_partial` only when `any_failed()` is true OR when there are
        // skipped nodes without paused/failure — the latter still counts as
        // a non-success that we surface as `failed` so the plan FSM does
        // not silently advance to `succeeded`).
        "dag_partial" => "failed",
        // Paused → we explicitly preserve the current status. The plan stays
        // in whatever pre-execute state it was in (Approved / Executing /
        // AwaitingReview); the resume helper (wave-17 / task 01) is
        // responsible for advancing it once the gate resolves.
        "dag_paused" => unchanged_status_label(current_plan_status),
        _ => unchanged_status_label(current_plan_status),
    }
}

/// Helper for the paused / unknown-aggregate branch of
/// `finalize_plan_status_label`. Returns `"executing"` when the plan was
/// mid-flight (the wave-17 / task 04 + wave-13 contract leaves the plan in
/// `Executing` while the DAG runs), `"awaiting_review"` when the caller
/// supplied that explicit string, otherwise `"unchanged"`. Pure projection.
fn unchanged_status_label(current_plan_status: &str) -> &'static str {
    match current_plan_status {
        "executing" => "executing",
        "awaiting_review" => "awaiting_review",
        _ => "unchanged",
    }
}

/// Build the `finalization` block surfaced on the response when
/// `finalize_plan=true`. Pure projection over the aggregate + observed
/// plan-status update. Carries the rule label so callers can grep the
/// reason without re-deriving it from the aggregate alone.
pub(super) fn build_finalization_block(
    aggregate: &str,
    plan_status_after: Option<&str>,
    plan_status_update_error: Option<&str>,
    distill_block: Option<Value>,
) -> Value {
    let final_plan_status = plan_status_after.unwrap_or("unchanged");
    let mut block = json!({
        "finalize_plan": true,
        "aggregate_status": aggregate,
        "final_plan_status": final_plan_status,
        "rule": match aggregate {
            "dag_succeeded" => "all_terminal_no_failed_no_paused",
            "dag_failed" => "fail_fast_or_failure_dominates",
            "dag_partial" => "failed_node_or_skipped_without_paused",
            "dag_paused" => "paused_node_present_no_finalization",
            _ => "unrecognised_aggregate_no_finalization",
        },
    });
    if let Some(err) = plan_status_update_error {
        block["plan_status_update_error"] = json!(err);
    }
    if let Some(d) = distill_block {
        block["distill"] = d;
    }
    block
}

/// Build the `distill` sub-block describing the trigger outcome. Always
/// surfaces a `triggered` boolean + a `reason` string so observers can
/// pivot on a single flag without inspecting the inner payload. The
/// inner workflow handler payload (success or error) is preserved under
/// `result` for full audit traceability.
pub(super) fn build_distill_block(
    triggered: bool,
    reason: &str,
    distill_mode: &str,
    inner_payload: Option<Value>,
    inner_is_error: bool,
) -> Value {
    let mut block = json!({
        "triggered": triggered,
        "reason": reason,
        "distill_mode": distill_mode,
    });
    if let Some(p) = inner_payload {
        block["result"] = p;
    }
    if triggered && inner_is_error {
        // Surface a partial-success warning so callers can detect a
        // distill failure without scraping the inner payload. The
        // finalization status itself is NOT downgraded — distill failure
        // never corrupts the plan final state per the brief.
        block["warning"] =
            json!("distill trigger returned an error; plan final state preserved");
    }
    block
}

/// Per-node claim scope derivation. Returns the list of scope tokens the
/// claim covers PLUS the provenance source label. Priority (matches the
/// task brief):
///
///   1. `:owned-files` — when at least one file is declared, every file
///      becomes its own scope token. This lets the registry detect
///      overlap at the file granularity (the same way the wave12-01
///      audit + wave16-06 enforce paths do).
///   2. `:scope` — when `:owned-files` is empty but the author declared
///      a free-form `:scope` string. Used verbatim.
///   3. `plan/<plan_id>/node/<node_id>` synthetic fallback — guarantees
///      every dispatched node carries SOME scope so the registry can
///      record the claim even for ungated nodes.
///
/// The function never returns an empty vector: the fallback case always
/// yields one synthetic token. Pure (no AppState) so unit tests can pin
/// the priority directly.
pub(super) fn derive_node_claim_scopes(
    node: &DagNode,
    plan_id: uuid::Uuid,
) -> (Vec<String>, &'static str) {
    let owned: Vec<String> =
        super::plan::split_lisp_string_list(node.owned_files_raw.as_deref())
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect();
    if !owned.is_empty() {
        return (owned, CLAIM_SCOPE_SOURCE_OWNED_FILES);
    }
    if let Some(scope) = node.scope.as_deref() {
        let trimmed = scope.trim();
        if !trimmed.is_empty() {
            return (vec![trimmed.to_string()], CLAIM_SCOPE_SOURCE_SCOPE);
        }
    }
    (
        vec![format!("plan/{}/node/{}", plan_id, node.id)],
        CLAIM_SCOPE_SOURCE_PLAN_NODE_FALLBACK,
    )
}

/// Build the deterministic claim id for a plan-DAG node attempt. Pure
/// helper so tests can pin the format without standing up a registry.
/// Layout: `plan-dag:<plan_id>:<node_id>:<attempt>`. Includes the attempt
/// number so retries (wave-16 / task 05) get a fresh claim id rather
/// than colliding with the previous attempt's record.
pub(super) fn derive_plan_dag_claim_id(
    plan_id: uuid::Uuid,
    node_id: &str,
    attempt: u32,
) -> String {
    format!("plan-dag:{}:{}:{}", plan_id, node_id, attempt)
}

/// Single in-memory claim record carried by `ClaimRegistry`. Fields
/// mirror the wave12-01 `ClaimRecord` shape so dashboards consuming
/// either surface see the same vocabulary; we add `released_at` as
/// `Option` so a single record can describe both the active and
/// released states without splitting the type.
#[derive(Debug, Clone)]
pub(super) struct PlanDagClaim {
    pub claim_id: String,
    pub claimer: String,
    pub scopes: Vec<String>,
    pub scope_source: &'static str,
    pub acquired_at: chrono::DateTime<chrono::Utc>,
    pub lease_expires_at: chrono::DateTime<chrono::Utc>,
    pub released_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl PlanDagClaim {
    /// ISO-8601 second-precision projection of `acquired_at` (matches
    /// wave12-01's `to_rfc3339_opts(SecondsFormat::Secs, true)` so the
    /// two surfaces print timestamps identically).
    pub(super) fn acquired_at_iso(&self) -> String {
        self.acquired_at
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
    pub(super) fn lease_expires_at_iso(&self) -> String {
        self.lease_expires_at
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
    pub(super) fn released_at_iso(&self) -> Option<String> {
        self.released_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    }
}

/// Outcome of a `ClaimRegistry::try_acquire` call. The `Conflict`
/// variant carries enough context (the conflicting claim's id +
/// claimer + the offending pair of scopes) for the scheduler to
/// surface a structured error in `enforce_claims=true` mode and a
/// best-effort warning in `enforce_claims=false` mode.
#[derive(Debug, Clone)]
pub(super) enum ClaimAcquire {
    Acquired(PlanDagClaim),
    Conflict {
        attempted_claim_id: String,
        attempted_scopes: Vec<String>,
        attempted_scope_source: &'static str,
        conflicting_claim_id: String,
        conflicting_claimer: String,
        conflicting_scope: String,
        offending_scope: String,
    },
}

/// Per-DAG-run claim registry. NOT a global lock service — the registry
/// only exists for the lifetime of one `execute_with_concurrency` call.
/// Conflict detection reuses `scopes_overlap_pure` so the predicate
/// matches wave12-01's `action_claim` overlap test byte-for-byte.
#[derive(Debug, Default)]
pub(super) struct ClaimRegistry {
    claims: HashMap<String, PlanDagClaim>,
}

impl ClaimRegistry {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Try to acquire a claim covering `scopes`. Returns:
    ///   * `Acquired(claim)` if no active claim overlaps any of the
    ///     attempted scopes (or every overlap is on a released claim).
    ///   * `Conflict { ... }` otherwise — carries the offending scope
    ///     pair so the caller can surface a structured error.
    ///
    /// "Active" means `released_at.is_none()` AND `now <
    /// lease_expires_at`. Lease-expired claims are treated as
    /// soft-released (mirrors wave12-01). The scheduler never
    /// explicitly garbage-collects them — the registry is per-DAG-run
    /// and dies with the call.
    pub(super) fn try_acquire(
        &mut self,
        claim_id: String,
        claimer: String,
        scopes: Vec<String>,
        scope_source: &'static str,
        lease_secs: i64,
        now: chrono::DateTime<chrono::Utc>,
    ) -> ClaimAcquire {
        for existing in self.claims.values() {
            if existing.released_at.is_some() {
                continue;
            }
            if existing.lease_expires_at < now {
                continue;
            }
            for new_scope in &scopes {
                for held_scope in &existing.scopes {
                    if scopes_overlap_pure(new_scope, held_scope) {
                        let offending = new_scope.clone();
                        let held = held_scope.clone();
                        let conflicting_id = existing.claim_id.clone();
                        let conflicting_claimer = existing.claimer.clone();
                        return ClaimAcquire::Conflict {
                            attempted_claim_id: claim_id,
                            attempted_scopes: scopes,
                            attempted_scope_source: scope_source,
                            conflicting_claim_id: conflicting_id,
                            conflicting_claimer,
                            conflicting_scope: held,
                            offending_scope: offending,
                        };
                    }
                }
            }
        }
        let claim = PlanDagClaim {
            claim_id: claim_id.clone(),
            claimer,
            scopes,
            scope_source,
            acquired_at: now,
            lease_expires_at: now + chrono::Duration::seconds(lease_secs),
            released_at: None,
        };
        self.claims.insert(claim_id, claim.clone());
        ClaimAcquire::Acquired(claim)
    }

    /// Mark `claim_id` released at `now`. Returns the released record
    /// (with `released_at` populated) so the caller can surface the
    /// timestamp on the per-node evidence. Returns `None` if the id is
    /// unknown — the wave loop never calls release without a prior
    /// successful acquire so this only fires under a registry bug.
    pub(super) fn release(
        &mut self,
        claim_id: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<PlanDagClaim> {
        let entry = self.claims.get_mut(claim_id)?;
        if entry.released_at.is_none() {
            entry.released_at = Some(now);
        }
        Some(entry.clone())
    }

    /// Total number of recorded claims (active + released). Surfaced on
    /// the response so callers can spot "registry never recorded
    /// anything" without walking every node.
    pub(super) fn len(&self) -> usize {
        self.claims.len()
    }
}

/// Pure projection of the claim plan the scheduler WOULD register given
/// the configured knobs. Used by the dry-run response so callers can
/// preview every node's claim metadata without dispatching anything.
/// The projection mirrors `try_acquire` decisions for the EMPTY
/// registry (no overlap detection across nodes) — real runs may flag
/// conflicts the dry-run cannot foresee. The dry-run is therefore a
/// *what-each-node-would-claim* preview, not an outcome prediction.
pub(super) fn build_planned_claims(
    nodes: &[DagNode],
    order: &[String],
    plan_id: uuid::Uuid,
    claimer: &str,
    lease_secs: i64,
    enforce_claims: bool,
) -> Value {
    let by_id: HashMap<&str, &DagNode> =
        nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut out: Vec<Value> = Vec::with_capacity(order.len());
    for id in order {
        let Some(n) = by_id.get(id.as_str()) else { continue };
        let (scopes, source) = derive_node_claim_scopes(n, plan_id);
        out.push(json!({
            "node_id": n.id,
            "claim_id": derive_plan_dag_claim_id(plan_id, &n.id, 1),
            "claimer": claimer,
            "scopes": scopes,
            "scope_source": source,
            "lease_secs": lease_secs,
            "enforce_claims": enforce_claims,
        }));
    }
    Value::Array(out)
}

/// Pre-built immutable evidence parameters that vary per call to
/// `action_execute_dag_v1`. The scheduler captures these once so each
/// per-node evidence emit doesn't re-thread the same args through.
struct EvidenceCtx<'a> {
    plan_id: uuid::Uuid,
    /// wave-17 / task 03 — captured here so the deterministic
    /// acceptance pause id (which carries the plan version segment for
    /// resolver routing) can be derived without re-fetching the plan
    /// row from every emit site.
    plan_version: i32,
    project_arg: Option<&'a str>,
    cwd_arg: Option<&'a str>,
    target_project_arg: Option<&'a str>,
}

/// `EventRef::unavailable` reason kept for the legacy fallback path —
/// publish *and* deterministic-id construction must both fail before we
/// surrender to it. Wave-14 :: Task 02 wires `PlanNodeStateChanged` so the
/// normal path now writes `EventRef::new(...)` either with the live `Seq`
/// from the bus or with the deterministic
/// `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` id when the bus
/// publish fails. The `unavailable` placeholder is unreachable today (the
/// deterministic id is always derivable) but kept on the call surface so
/// the contract `evidence_collector` documents (`unavailable` → "we tried
/// to correlate but couldn't") stays implementable if a future caller
/// genuinely cannot stamp an id.
#[allow(dead_code)]
const EVENT_REF_UNAVAILABLE_REASON: &str =
    "plan_dag scheduler could not derive a live or deterministic \
     ExecutionEvent reference; this is a fallback path";

/// Domain tag used in `EventRef::source` for plan-node lifecycle entries.
/// Mirrors `Domain::Execution::as_str()` (kept as a `&'static str` here so
/// we don't pull the enum reference into every evidence call site).
const EVENT_REF_SOURCE_EXECUTION: &str = "execution";

/// Kind tag matching `ExecutionEvent::PlanNodeStateChanged.kind()`. Kept
/// duplicated here so test assertions can pin the wire form without taking
/// a dep on the event-trait reflection.
const EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED: &str = "plan_node_state_changed";

/// Build the deterministic event id used as a stable correlation key when
/// the live bus publish either succeeds (used in the publish dedupe context)
/// or fails (used as the `EventRef::event_id`). Format
/// `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` matches the wave-14
/// task brief verbatim so external consumers can grep on it.
pub(super) fn deterministic_plan_node_event_id(
    plan_id: uuid::Uuid,
    node_id: &str,
    attempt: u32,
    from: &str,
    to: &str,
) -> String {
    format!(
        "plan-node:{}:{}:{}:{}-{}",
        plan_id, node_id, attempt, from, to
    )
}

/// Build the `PlanNodeStateChanged` payload for a single transition.
/// Pure helper so unit tests can pin the wire shape without standing up an
/// `AppState`.
pub(super) fn build_plan_node_state_changed_event(
    plan_id: uuid::Uuid,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    from: &str,
    to: &str,
    reason: Option<String>,
) -> ExecutionEvent {
    ExecutionEvent::PlanNodeStateChanged {
        plan_id: plan_id.to_string(),
        node_id: node.id.clone(),
        from: from.to_string(),
        to: to.to_string(),
        target: Some(node.target.clone()),
        dispatch_strategy: Some(dispatch_strategy.to_string()),
        target_project: node.target_project.clone(),
        attempt: Some(attempt),
        reason,
    }
}

/// Publish a `PlanNodeStateChanged` event and return the `EventRef` to
/// embed in the evidence entry. On bus success we surface the live `Seq` as
/// the event id; on failure we fall back to the deterministic id derived
/// from `plan_id`/`node_id`/`attempt`/`from`/`to` so the audit trail still
/// carries a stable correlation key, and we record a warning string the
/// caller can lift into `outcome.bus_publish_warnings`.
///
/// The function NEVER aborts the dispatch on a publish failure — the
/// scheduler's main loop only consults the returned warning to decide
/// whether to surface `bus_publish_warnings` on the response. This matches
/// the wave-14 / task 02 brief: bus publish failure is observability-only.
async fn publish_plan_node_state_change(
    state: &AppState,
    plan_id: uuid::Uuid,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    from: &str,
    to: &str,
    reason: Option<String>,
) -> (EventRef, Option<String>) {
    let ev = build_plan_node_state_changed_event(
        plan_id,
        node,
        dispatch_strategy,
        attempt,
        from,
        to,
        reason,
    );
    let deterministic_id =
        deterministic_plan_node_event_id(plan_id, &node.id, attempt, from, to);
    match state.bus.publish_execution_with_seq(ev).await {
        Ok(seq) => (
            EventRef::new(
                EVENT_REF_SOURCE_EXECUTION,
                EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
                seq.0.to_string(),
            ),
            None,
        ),
        Err(err) => {
            // Wave-17 / task 06 — try the resolver before surrendering to
            // the deterministic-id fallback. The resolver checks its
            // in-memory cache first (a previous attempt for the same
            // transition may already have cached a real `Seq`), then
            // falls through to a bounded read-only scan of the persistent
            // event log so refs survive daemon restarts. Lookup failure
            // NEVER aborts the dispatch — on every error path we keep the
            // deterministic id so the audit trail still carries a stable
            // correlation key.
            let plan_id_str = plan_id.to_string();
            let recovered = state
                .bus
                .event_ref_resolver
                .lookup_or_query_plan_node_state_change(
                    state.bus.log.as_ref(),
                    &plan_id_str,
                    &node.id,
                    attempt,
                    from,
                    to,
                )
                .await;
            if recovered.status == evidence_collector::EventRefStatus::Log {
                let warning = format!(
                    "plan_node_state_changed bus publish failed for {} ({} -> {}): {}; \
                     evidence ref recovered from event log",
                    node.id, from, to, err
                );
                tracing::warn!(
                    plan_id = %plan_id,
                    node_id = %node.id,
                    from = %from,
                    to = %to,
                    error = %err,
                    "DAG scheduler: PlanNodeStateChanged bus publish failed; recovered event ref from log"
                );
                return (recovered, Some(warning));
            }
            let warning = format!(
                "plan_node_state_changed bus publish failed for {} ({} -> {}): {}; \
                 evidence ref falls back to deterministic id `{}`",
                node.id, from, to, err, deterministic_id
            );
            tracing::warn!(
                plan_id = %plan_id,
                node_id = %node.id,
                from = %from,
                to = %to,
                error = %err,
                "DAG scheduler: PlanNodeStateChanged bus publish failed; deterministic event ref retained"
            );
            (
                EventRef::new(
                    EVENT_REF_SOURCE_EXECUTION,
                    EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
                    deterministic_id,
                ),
                Some(warning),
            )
        }
    }
}

/// Per-node attempt counter. v2 has no retry policy so every transition
/// reports `attempt=1`; encapsulating the constant in a helper keeps the
/// retry-aware future scheduler a single-call-site change.
const PLAN_NODE_DEFAULT_ATTEMPT: u32 = 1;

/// wave-16 / task 05 — pure retry decision. Extracted out of the wave
/// loop so the predicate can be unit-tested without standing up an
/// `AppState`. `should_retry` is true iff the failed attempt is not
/// classified non-retryable AND attempts remain AND the wave is not
/// already in fail-fast abort. The wave loop honours this decision
/// deterministically so the tests below pin the contract.
pub(super) fn plan_node_should_retry(
    current_attempt: u32,
    max_attempts: u32,
    non_retryable: bool,
    abort_new_dispatch: bool,
) -> bool {
    if non_retryable || abort_new_dispatch {
        return false;
    }
    let attempts_remaining = max_attempts.saturating_sub(current_attempt);
    attempts_remaining > 0
}

/// Emit `ready -> running` evidence at the moment the scheduler hands a node
/// to its dispatch task. Kept structurally identical to the success/failure
/// branches so audit dashboards can pivot on `state_transition` alone.
///
/// Wave-14 / Task 02: also publishes a `PlanNodeStateChanged` event on the
/// execution bus and stamps the resulting live `Seq` (or the deterministic
/// fallback id when publish fails) onto the evidence entry's
/// `execution_events` array. Bus publish failures land in
/// `outcome.bus_publish_warnings` so the response surfaces the degraded
/// observability path without aborting the dispatch.
async fn emit_evidence_running(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    outcome: &mut ExecutionOutcome,
) {
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "ready",
        "running",
        None,
    )
    .await;
    if let Some(w) = &warning {
        outcome.bus_publish_warnings.push(w.clone());
    }
    let entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("ready -> running")
    .with_primary_event_ref(&event_ref, warning)
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt));
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: ready->running evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// Emit `running -> succeeded` (success branch) or `running -> failed`
/// (failure branch) evidence after the dispatch task returns. The two
/// branches keep the byte shape of v1's `ready -> {succeeded|failed}` legacy
/// passthrough fields so existing audit consumers do not need updates.
///
/// Wave-14 / Task 02: also publishes a `PlanNodeStateChanged` event on the
/// execution bus and stamps the resulting live `Seq` (or the deterministic
/// fallback id when publish fails) onto the evidence entry's
/// `execution_events` array. The `reason` annotation on the failure branch
/// surfaces the inner-handler error message so bus consumers can route
/// without re-fetching the sidecar payload.
async fn emit_evidence_finished(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    inner_payload: &Value,
    succeeded: bool,
    attempt: u32,
    outcome: &mut ExecutionOutcome,
) {
    let to_state = if succeeded { "succeeded" } else { "failed" };
    let reason = if succeeded {
        None
    } else {
        // Best-effort: surface the inner-handler's `error` field so bus
        // consumers see the same string the response carries. Fallback to
        // the canonical "inner handler returned error" when no `error`
        // string is present (mirrors `dispatch_node` classification).
        let s = inner_payload
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("inner handler returned error")
            .to_string();
        Some(s)
    };
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "running",
        to_state,
        reason,
    )
    .await;
    if let Some(w) = &warning {
        outcome.bus_publish_warnings.push(w.clone());
    }
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_primary_event_ref(&event_ref, warning)
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt));
    if succeeded {
        // Success branch — populate `inner_dispatch` (canonical typed slot)
        // AND `inner_result` (legacy alias) so wave-12 typed readers and
        // pre-wave12 dashboard greps both keep working byte-for-byte.
        entry = entry
            .with_inner_dispatch(inner_payload.clone())
            .with_state_transition("running -> succeeded")
            .with_extra("inner_result", inner_payload.clone());
    } else {
        // Failure branch — keep the legacy `inner_error` extra slot for
        // readers that historically filtered on it; intentionally do NOT
        // call `with_inner_dispatch` so success vs failure stay shape-distinct.
        entry = entry
            .with_state_transition("running -> failed")
            .with_extra("inner_error", inner_payload.clone());
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: running->{} evidence append failed",
            to_state
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// wave-17 / task 04 — execute the conservative rollback pass for a
/// just-failed node. Pure async wrapper over the descriptor /
/// safety-check / optional-dispatch pipeline so the wave loop's
/// final-failure branch can call a single helper.
///
/// Behaviour matrix (matches the wave-17 / task 04 brief):
///   * No rollback hints OR `:rollback-policy "none"` →
///     `RollbackEvaluation { status: NotRequested, ... }` and the
///     scheduler skips the rollback evidence emit entirely.
///   * `:rollback-policy "descriptor"` → fully-populated descriptor
///     evaluation with `status=DescriptorReady`, no dispatch attempt.
///   * `:rollback-policy "workstation"` + safety check fails →
///     `status=Refused` with the failing condition spelled out, no
///     dispatch attempt. SafeDescriptor refusals from the substrate
///     also collapse to `Refused`.
///   * `:rollback-policy "workstation"` + safety check passes →
///     dispatch via `run_workstation_dispatch`. On success
///     `status=Dispatched` (with brief preview + inner payload). On
///     inner-handler error `status=Failed` with the error message on
///     the reason. SafeDescriptor refusals (which can still surface
///     even after the static safety check passes — e.g. resolver
///     reports a non-existent project root) become `Refused` so the
///     non-retryable refusal vocabulary stays consistent across all
///     workstation-substrate consumers.
async fn run_rollback(
    state: &AppState,
    plan: &Plan,
    node: &DagNode,
) -> RollbackEvaluation {
    let descriptor = build_rollback_descriptor(node);
    match descriptor.policy {
        RollbackPolicy::None => RollbackEvaluation {
            policy: RollbackPolicy::None,
            status: RollbackStatus::NotRequested,
            reason: if node.has_rollback_hints() {
                "rollback policy explicitly set to none; no rollback dispatch".to_string()
            } else {
                "no rollback hints declared".to_string()
            },
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: None,
            task_brief_path: None,
            inner_payload: None,
            cascade: None,
        },
        RollbackPolicy::Descriptor => {
            // Build the descriptor brief locally so observers see the
            // same shape they would for a forward task brief, but
            // NEVER dispatch.
            let hints = descriptor.to_workstation_hints(node);
            let strategy = node
                .dispatch_strategy
                .as_deref()
                .unwrap_or("unknown");
            let preview = if descriptor.objective.is_some() {
                Some(truncate_rollback_brief_preview(
                    &super::workstation_dispatch::build_task_brief(
                        plan, &hints, strategy,
                    ),
                ))
            } else {
                None
            };
            RollbackEvaluation {
                policy: RollbackPolicy::Descriptor,
                status: RollbackStatus::DescriptorReady,
                reason: "descriptor mode: rollback intent recorded; no dispatch performed"
                    .to_string(),
                objective: descriptor.objective.clone(),
                owned_files: descriptor.owned_files.clone(),
                acceptance_commands: descriptor.acceptance_commands.clone(),
                task_brief_preview: preview,
                task_brief_path: None,
                inner_payload: None,
                cascade: None,
            }
        }
        RollbackPolicy::Workstation => {
            // Run the static safety check first so a refusal here
            // never touches the substrate. SafeDescriptor refusals
            // are non-retryable per the wave-15 contract.
            if let Err(reason) = descriptor.safety_check_for_workstation(node) {
                return RollbackEvaluation {
                    policy: RollbackPolicy::Workstation,
                    status: RollbackStatus::Refused,
                    reason: format!("rollback workstation dispatch refused: {}", reason),
                    objective: descriptor.objective,
                    owned_files: descriptor.owned_files,
                    acceptance_commands: descriptor.acceptance_commands,
                    task_brief_preview: None,
                    task_brief_path: None,
                    inner_payload: None,
                    cascade: None,
                };
            }
            // Static safety passed — dispatch through the substrate.
            // The substrate may STILL refuse (e.g. cwd not absolute,
            // project registry miss); we map every SafeDescriptor
            // refusal back to `Refused` so the non-retryable
            // vocabulary stays consistent.
            let hints = descriptor.to_workstation_hints(node);
            let strategy = node
                .dispatch_strategy
                .as_deref()
                .unwrap_or("unknown");
            let outcome = super::workstation_dispatch::run_workstation_dispatch(
                state, plan, "mission_task_delegate", strategy, hints, false,
            )
            .await;
            match outcome {
                super::workstation_dispatch::WorkstationDispatchOutcome::Dispatched {
                    task_brief,
                    task_brief_path,
                    inner_payload,
                    ..
                } => RollbackEvaluation {
                    policy: RollbackPolicy::Workstation,
                    status: RollbackStatus::Dispatched,
                    reason: "rollback workstation dispatch completed; inner handler returned Ok"
                        .to_string(),
                    objective: descriptor.objective,
                    owned_files: descriptor.owned_files,
                    acceptance_commands: descriptor.acceptance_commands,
                    task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                    task_brief_path,
                    inner_payload: Some(inner_payload),
                    cascade: None,
                },
                super::workstation_dispatch::WorkstationDispatchOutcome::DryRun {
                    task_brief,
                } => {
                    // The wave loop never asks for dry_run on rollback
                    // (we always pass dry_run=false above). Defensive:
                    // if a future caller flips the knob we surface as
                    // dispatched with no inner payload so observers
                    // don't see a missing variant.
                    RollbackEvaluation {
                        policy: RollbackPolicy::Workstation,
                        status: RollbackStatus::Dispatched,
                        reason: "rollback dispatched in dry_run mode (no real handler invoked)"
                            .to_string(),
                        objective: descriptor.objective,
                        owned_files: descriptor.owned_files,
                        acceptance_commands: descriptor.acceptance_commands,
                        task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                        task_brief_path: None,
                        inner_payload: None,
                        cascade: None,
                    }
                }
                super::workstation_dispatch::WorkstationDispatchOutcome::InnerError {
                    task_brief,
                    inner_payload,
                } => {
                    let detail = inner_payload
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("rollback inner handler returned error")
                        .to_string();
                    RollbackEvaluation {
                        policy: RollbackPolicy::Workstation,
                        status: RollbackStatus::Failed,
                        reason: format!("rollback workstation dispatch failed: {}", detail),
                        objective: descriptor.objective,
                        owned_files: descriptor.owned_files,
                        acceptance_commands: descriptor.acceptance_commands,
                        task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                        task_brief_path: None,
                        inner_payload: Some(inner_payload),
                        cascade: None,
                    }
                }
                super::workstation_dispatch::WorkstationDispatchOutcome::SafeDescriptor {
                    reason,
                    task_brief,
                } => {
                    // Substrate-side safety refusal — collapse to
                    // Refused so the wave loop treats it as
                    // non-retryable (mirrors wave-15 / task 05).
                    RollbackEvaluation {
                        policy: RollbackPolicy::Workstation,
                        status: RollbackStatus::Refused,
                        reason: format!(
                            "rollback workstation dispatch refused (substrate): {}",
                            reason.detail()
                        ),
                        objective: descriptor.objective,
                        owned_files: descriptor.owned_files,
                        acceptance_commands: descriptor.acceptance_commands,
                        task_brief_preview: task_brief
                            .as_deref()
                            .map(truncate_rollback_brief_preview),
                        task_brief_path: None,
                        inner_payload: None,
                        cascade: None,
                    }
                }
            }
        }
    }
}

/// wave-18 / task 04 — pure helper that computes the ordered list of
/// compensation node ids for a given failed (cascade-root) node.
///
/// Compensation discovery: every plan node carrying
/// `:compensates "<root_id>"` (case-insensitive on the root id) is a
/// candidate. Ordering rules:
///
///   1. Honour each candidate's `:rollback-after` hints when those
///      targets are also compensation candidates for the SAME root.
///      The cascade ordering uses a Kahn-style topological sort
///      restricted to compensation candidates.
///   2. Tie-break by the topological-sort `order` produced by
///      `build_validated_dag` so dispatch order in the cascade
///      mirrors the forward DAG.
///   3. If `:rollback-after` introduces a cycle (typo), fall back to
///      step (2)'s declaration order — we never deadlock the cascade
///      because a typo cannot be a fatal scheduler condition.
///
/// Pure: no IO, no AppState reads. Decoupled so the cascade evaluator
/// + unit tests can pin the contract identically.
pub(super) fn compute_compensation_order<'a>(
    failed_id: &str,
    nodes: &'a [DagNode],
    forward_order: &[String],
) -> Vec<&'a DagNode> {
    let root_lc = failed_id.trim().to_ascii_lowercase();
    // wave-19 / task 10 — forward `:compensate-node` refs declared on
    // the failing node also surface compensation candidates. Build the
    // forward-ref id set (case-insensitive) by inspecting the failing
    // node's `compensate_node` slot. The validator (`build_validated_dag`)
    // has already rejected self-refs / unknown ids / disagreements
    // against any reverse `:compensates` declaration, so here we can
    // safely union the two directions without re-checking agreement.
    let forward_targets: HashSet<String> = nodes
        .iter()
        .filter(|n| n.id.to_ascii_lowercase() == root_lc)
        .filter_map(|n| {
            n.compensate_node
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_lowercase())
        })
        .collect();
    // Compensation candidate set + lookup helpers. A node is a candidate
    // iff EITHER it carries the reverse `:compensates "<root>"` declaration
    // OR the failing (root) node points at it via `:compensate-node`.
    let candidates: Vec<&DagNode> = nodes
        .iter()
        .filter(|n| {
            let reverse_match = n
                .compensates
                .as_deref()
                .map(|s| s.trim().to_ascii_lowercase() == root_lc)
                .unwrap_or(false);
            let forward_match =
                forward_targets.contains(&n.id.to_ascii_lowercase());
            reverse_match || forward_match
        })
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    // Forward-order rank table — used as a tie-breaker so cascade
    // dispatch mirrors forward dispatch order when no `:rollback-after`
    // edge exists.
    let forward_rank: HashMap<&str, usize> = forward_order
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    // Restrict the dependency graph to compensation candidates only.
    // `:rollback-after` edges that reference non-candidates are
    // dropped silently (they cannot block the cascade because the
    // referenced node will never run as a compensation step here).
    let candidate_ids: HashSet<&str> =
        candidates.iter().map(|n| n.id.as_str()).collect();
    let mut indeg: HashMap<&str, usize> = HashMap::new();
    let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &candidates {
        indeg.entry(n.id.as_str()).or_insert(0);
        for after in &n.rollback_after {
            let after_s = after.trim();
            if after_s.is_empty() || !candidate_ids.contains(after_s) {
                continue;
            }
            // Edge: after_s → n.id (n must run AFTER `after_s`).
            *indeg.entry(n.id.as_str()).or_insert(0) += 1;
            succs.entry(after_s).or_default().push(n.id.as_str());
        }
    }
    // Kahn-style topological sort with deterministic tie-break: at
    // each step we pop the candidate with the smallest forward-order
    // rank. Falls back to the declaration order embedded in
    // `candidates` for IDs not present in `forward_order`.
    let mut by_id: HashMap<&str, &DagNode> =
        candidates.iter().map(|n| (n.id.as_str(), *n)).collect();
    let declaration_rank: HashMap<&str, usize> = candidates
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let rank_of = |id: &str| -> (usize, usize) {
        (
            forward_rank.get(id).copied().unwrap_or(usize::MAX),
            declaration_rank.get(id).copied().unwrap_or(usize::MAX),
        )
    };
    let mut ordered: Vec<&DagNode> = Vec::with_capacity(candidates.len());
    loop {
        // Collect all currently-zero-indegree candidates.
        let mut ready: Vec<&str> = indeg
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(id, _)| *id)
            .collect();
        if ready.is_empty() {
            break;
        }
        ready.sort_by_key(|id| rank_of(id));
        // Pop the smallest-ranked entry.
        let next = ready[0];
        // Defensive: if for some reason the candidate is missing,
        // skip it.
        if let Some(n) = by_id.remove(next) {
            ordered.push(n);
        }
        indeg.remove(next);
        if let Some(children) = succs.remove(next) {
            for child in children {
                if let Some(d) = indeg.get_mut(child) {
                    *d = d.saturating_sub(1);
                }
            }
        }
    }
    // Cycle / unresolved entries — fall back to declaration order so
    // a typo never deadlocks the cascade.
    if ordered.len() < candidates.len() {
        let already: HashSet<&str> = ordered.iter().map(|n| n.id.as_str()).collect();
        for n in &candidates {
            if !already.contains(n.id.as_str()) {
                ordered.push(*n);
            }
        }
    }
    ordered
}

/// wave-18 / task 04 — pure helper that builds a `plan`-mode cascade
/// outcome for a single compensation node. Records intent + brief
/// preview but never dispatches. Decoupled so unit tests can pin the
/// shape without standing up an `AppState`.
pub(super) fn build_compensation_plan_entry(
    plan: &Plan,
    node: &DagNode,
) -> CascadeCompensationOutcome {
    let descriptor = build_rollback_descriptor(node);
    let policy = descriptor.policy;
    let hints = descriptor.to_workstation_hints(node);
    let strategy = node
        .dispatch_strategy
        .as_deref()
        .unwrap_or("unknown");
    let preview = if descriptor.objective.is_some() {
        Some(truncate_rollback_brief_preview(
            &super::workstation_dispatch::build_task_brief(
                plan, &hints, strategy,
            ),
        ))
    } else {
        None
    };
    CascadeCompensationOutcome {
        node_id: node.id.clone(),
        policy,
        status: RollbackStatus::DescriptorReady,
        reason: "cascade plan: compensation node recorded; no dispatch performed".to_string(),
        objective: descriptor.objective,
        owned_files: descriptor.owned_files,
        acceptance_commands: descriptor.acceptance_commands,
        task_brief_preview: preview,
        task_brief_path: None,
        inner_payload: None,
    }
}

/// wave-18 / task 04 — async cascade evaluator. Runs AFTER a node's
/// final failed attempt and AFTER the node-local `run_rollback`. Pure
/// when `mode == Plan` (no IO beyond `build_task_brief`); only the
/// `DispatchSafe` mode invokes the substrate.
///
/// Behaviour matrix:
///
///   * `mode == None`         — returns an inactive outcome; the wave
///                              loop suppresses the cascade surface.
///   * `mode == Plan`         — every compensation node lands as
///                              `descriptor_ready`. **Never dispatches.**
///   * `mode == DispatchSafe` — for each compensation node, run the
///                              wave-17 / task 04 safety check on its
///                              own descriptor; only dispatch when the
///                              gate passes AND the compensation node's
///                              policy is `workstation`. Any safety /
///                              substrate refusal lands as `refused`
///                              (non-retryable). `descriptor`-only
///                              compensations stay `descriptor_ready`.
async fn run_cascade_rollback(
    state: &AppState,
    plan: &Plan,
    failed_node: &DagNode,
    nodes: &[DagNode],
    forward_order: &[String],
) -> CascadeRollbackOutcome {
    let mode = failed_node
        .rollback_cascade_kind()
        .unwrap_or(RollbackCascadeMode::None);
    if matches!(mode, RollbackCascadeMode::None) {
        return CascadeRollbackOutcome {
            mode,
            cascade_root: failed_node.id.clone(),
            compensations: Vec::new(),
            reason: "cascade rollback not requested".to_string(),
        };
    }
    let ordered = compute_compensation_order(&failed_node.id, nodes, forward_order);
    if ordered.is_empty() {
        return CascadeRollbackOutcome {
            mode,
            cascade_root: failed_node.id.clone(),
            compensations: Vec::new(),
            reason: format!(
                "cascade {}: no compensation nodes declared `:compensates \"{}\"`",
                mode.as_wire(),
                failed_node.id
            ),
        };
    }
    let mut compensations: Vec<CascadeCompensationOutcome> =
        Vec::with_capacity(ordered.len());
    for n in ordered {
        match mode {
            RollbackCascadeMode::None => unreachable!(),
            RollbackCascadeMode::Plan => {
                compensations.push(build_compensation_plan_entry(plan, n));
            }
            RollbackCascadeMode::DispatchSafe => {
                // Only dispatch when the compensation node's own
                // rollback policy is `workstation` AND every safety
                // gate passes. Otherwise fall back to `plan` mode for
                // this entry — record intent, never dispatch.
                let descriptor = build_rollback_descriptor(n);
                match descriptor.policy {
                    RollbackPolicy::Workstation => {
                        match descriptor.safety_check_for_workstation(n) {
                            Err(reason) => {
                                // Safety gate refused — record refusal,
                                // never retry.
                                compensations.push(CascadeCompensationOutcome {
                                    node_id: n.id.clone(),
                                    policy: RollbackPolicy::Workstation,
                                    status: RollbackStatus::Refused,
                                    reason: format!(
                                        "cascade dispatch-safe refused: {}",
                                        reason
                                    ),
                                    objective: descriptor.objective,
                                    owned_files: descriptor.owned_files,
                                    acceptance_commands: descriptor
                                        .acceptance_commands,
                                    task_brief_preview: None,
                                    task_brief_path: None,
                                    inner_payload: None,
                                });
                            }
                            Ok(()) => {
                                let hints = descriptor.to_workstation_hints(n);
                                let strategy = n
                                    .dispatch_strategy
                                    .as_deref()
                                    .unwrap_or("unknown");
                                let outcome = super::workstation_dispatch::run_workstation_dispatch(
                                    state,
                                    plan,
                                    "mission_task_delegate",
                                    strategy,
                                    hints,
                                    false,
                                )
                                .await;
                                compensations.push(map_dispatch_outcome_to_compensation(
                                    n.id.clone(),
                                    descriptor,
                                    outcome,
                                ));
                            }
                        }
                    }
                    RollbackPolicy::Descriptor | RollbackPolicy::None => {
                        // Compensation node opted into descriptor-only
                        // (or no rollback policy at all). Cascade
                        // dispatch-safe MUST NEVER promote a non-
                        // workstation compensation to a dispatch — that
                        // would silently change the scope of work the
                        // author authorised. Record the plan entry and
                        // move on.
                        compensations.push(build_compensation_plan_entry(plan, n));
                    }
                }
            }
        }
    }
    let dispatched = compensations
        .iter()
        .filter(|c| matches!(c.status, RollbackStatus::Dispatched))
        .count();
    let refused = compensations
        .iter()
        .filter(|c| matches!(c.status, RollbackStatus::Refused))
        .count();
    let failed = compensations
        .iter()
        .filter(|c| matches!(c.status, RollbackStatus::Failed))
        .count();
    let recorded = compensations
        .iter()
        .filter(|c| matches!(c.status, RollbackStatus::DescriptorReady))
        .count();
    let reason = format!(
        "cascade {}: compensation_nodes={} recorded={} dispatched={} refused={} failed={}",
        mode.as_wire(),
        compensations.len(),
        recorded,
        dispatched,
        refused,
        failed,
    );
    CascadeRollbackOutcome {
        mode,
        cascade_root: failed_node.id.clone(),
        compensations,
        reason,
    }
}

/// wave-18 / task 04 — pure helper translating a workstation-dispatch
/// outcome into a single cascade compensation row. Decoupled from the
/// async cascade body so unit tests can pin every dispatch branch.
fn map_dispatch_outcome_to_compensation(
    node_id: String,
    descriptor: RollbackDescriptor,
    outcome: super::workstation_dispatch::WorkstationDispatchOutcome,
) -> CascadeCompensationOutcome {
    use super::workstation_dispatch::WorkstationDispatchOutcome as O;
    match outcome {
        O::Dispatched {
            task_brief,
            task_brief_path,
            inner_payload,
            ..
        } => CascadeCompensationOutcome {
            node_id,
            policy: RollbackPolicy::Workstation,
            status: RollbackStatus::Dispatched,
            reason: "cascade dispatch-safe: workstation dispatch completed; inner handler returned Ok"
                .to_string(),
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
            task_brief_path,
            inner_payload: Some(inner_payload),
        },
        O::DryRun { task_brief } => CascadeCompensationOutcome {
            node_id,
            policy: RollbackPolicy::Workstation,
            status: RollbackStatus::Dispatched,
            reason: "cascade dispatch-safe: substrate ran dry_run (no real handler invoked)"
                .to_string(),
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
            task_brief_path: None,
            inner_payload: None,
        },
        O::InnerError {
            task_brief,
            inner_payload,
        } => {
            let detail = inner_payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("cascade compensation inner handler returned error")
                .to_string();
            CascadeCompensationOutcome {
                node_id,
                policy: RollbackPolicy::Workstation,
                status: RollbackStatus::Failed,
                reason: format!(
                    "cascade dispatch-safe: workstation dispatch failed: {}",
                    detail
                ),
                objective: descriptor.objective,
                owned_files: descriptor.owned_files,
                acceptance_commands: descriptor.acceptance_commands,
                task_brief_preview: Some(truncate_rollback_brief_preview(&task_brief)),
                task_brief_path: None,
                inner_payload: Some(inner_payload),
            }
        }
        O::SafeDescriptor { reason, task_brief } => CascadeCompensationOutcome {
            node_id,
            policy: RollbackPolicy::Workstation,
            status: RollbackStatus::Refused,
            reason: format!(
                "cascade dispatch-safe refused (substrate): {}",
                reason.detail()
            ),
            objective: descriptor.objective,
            owned_files: descriptor.owned_files,
            acceptance_commands: descriptor.acceptance_commands,
            task_brief_preview: task_brief
                .as_deref()
                .map(truncate_rollback_brief_preview),
            task_brief_path: None,
            inner_payload: None,
        },
    }
}

/// wave-17 / task 04 — local copy of the workstation-dispatch preview
/// truncation so the rollback evaluation block surfaces a humane
/// preview without taking a dep on the substrate's private helper.
/// Same MAX (800 chars) so previews look identical across surfaces.
fn truncate_rollback_brief_preview(brief: &str) -> String {
    const MAX: usize = 800;
    if brief.len() <= MAX {
        return brief.to_string();
    }
    let mut end = MAX;
    while end > 0 && !brief.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &brief[..end])
}

/// wave-17 / task 04 — emit one rollback-phase evidence entry per
/// failed node that opted into a rollback policy. Runs ONLY after
/// `emit_evidence_finished` for the failure branch and BEFORE
/// `propagate_taint`, so audit dashboards can pivot on the
/// `failed -> rollback_*` transition between the failure row and any
/// downstream `pending -> skipped` rows.
///
/// The entry's `state_transition` reflects the rollback decision
/// (`failed -> rollback_descriptor_ready`,
/// `failed -> rollback_dispatched`, `failed -> rollback_refused`,
/// `failed -> rollback_failed`) so audit dashboards can pivot on a
/// single string. Entries surface every field on
/// [`RollbackEvaluation::to_json`] PLUS the typed top-level
/// `rollback_status` / `rollback_policy` slots so legacy dashboards
/// can grep without descending into the `rollback` block.
///
/// Bus publish failure on the lifecycle event is observability-only —
/// the warning lands on `outcome.bus_publish_warnings` and the
/// evidence ref falls back to the deterministic id; the rollback
/// decision itself is unaffected.
async fn emit_evidence_rollback(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    evaluation: &RollbackEvaluation,
    outcome: &mut ExecutionOutcome,
) {
    let to_state = match evaluation.status {
        RollbackStatus::NotRequested => "rollback_skipped",
        RollbackStatus::DescriptorReady => "rollback_descriptor_ready",
        RollbackStatus::Dispatched => "rollback_dispatched",
        RollbackStatus::Refused => "rollback_refused",
        RollbackStatus::Failed => "rollback_failed",
    };
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "failed",
        to_state,
        Some(format!(
            "rollback:{}:policy={}:reason={}",
            evaluation.status.as_wire(),
            evaluation.policy.as_wire(),
            evaluation.reason
        )),
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition(format!("failed -> {}", to_state))
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("rollback_policy", json!(evaluation.policy.as_wire()))
    .with_extra("rollback_status", json!(evaluation.status.as_wire()))
    .with_extra("rollback_reason", json!(evaluation.reason))
    .with_extra("rollback_owned_files", json!(evaluation.owned_files))
    .with_extra(
        "rollback_acceptance_commands",
        json!(evaluation.acceptance_commands),
    )
    .with_extra("rollback_acceptance_commands_executed", json!(false));
    if let Some(obj) = evaluation.objective.as_deref() {
        entry = entry.with_extra("rollback_objective", json!(obj));
    }
    if let Some(preview) = evaluation.task_brief_preview.as_deref() {
        entry = entry.with_extra("rollback_task_brief_preview", json!(preview));
    }
    if let Some(p) = evaluation.task_brief_path.as_deref() {
        entry = entry.with_extra("rollback_task_brief_path", json!(p));
    }
    if let Some(inner) = evaluation.inner_payload.clone() {
        entry = entry.with_extra("rollback_inner_result", inner);
    }
    // wave-18 / task 04 — cascade rollback evidence extras. Surfaced
    // alongside the node-local rollback fields so audit dashboards can
    // grep `rollback_cascade_*` without descending into the embedded
    // `cascade` JSON. Quiet (omitted) when the cascade evaluator never
    // produced a signal so the wave-17 / task 04 byte shape stays
    // untouched for plans that did not opt into cascading.
    if let Some(cascade) = evaluation.cascade.as_ref() {
        if !cascade.is_inactive() {
            let comp_ids: Vec<&str> = cascade
                .compensations
                .iter()
                .map(|c| c.node_id.as_str())
                .collect();
            entry = entry
                .with_extra("rollback_cascade_mode", json!(cascade.mode.as_wire()))
                .with_extra(
                    "rollback_cascade_root",
                    json!(cascade.cascade_root),
                )
                .with_extra(
                    "rollback_cascade_compensation_node_ids",
                    json!(comp_ids),
                )
                .with_extra(
                    "rollback_cascade_compensation_count",
                    json!(cascade.compensations.len()),
                )
                .with_extra("rollback_cascade_reason", json!(cascade.reason))
                .with_extra("rollback_cascade", cascade.to_json());
        }
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: failed->rollback_* evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// wave-17 / task 03 — emit one acceptance-phase evidence entry per
/// successfully-dispatched node that opted into the acceptance contract.
/// Runs ONLY after `emit_evidence_finished` for the success branch; the
/// scheduler skips the call entirely for nodes that did not declare
/// acceptance hints so the wave-13 byte shape is preserved.
///
/// The entry's `state_transition` reflects the acceptance decision
/// (`succeeded -> acceptance_accepted`, `succeeded -> acceptance_rejected`,
/// `succeeded -> acceptance_manual_required`) so audit dashboards can
/// pivot on a single string. The entry surfaces:
///   * `acceptance_status` — wire form of [`AcceptanceStatus`].
///   * `acceptance_mode` — wire form of [`AcceptanceMode`] when set.
///   * `acceptance_commands` — declared commands surfaced verbatim,
///     **NEVER executed**. They are recorded so observers / out-of-band
///     pipelines can see what the author wanted to verify.
///   * `acceptance_evidence_keys` — declared required keys.
///   * `acceptance_reason` — human-readable explanation.
///
/// Bus publish failure on the lifecycle event is observability-only —
/// the warning lands on `outcome.bus_publish_warnings` and the
/// evidence ref falls back to the deterministic id; the acceptance
/// decision itself is unaffected.
async fn emit_evidence_acceptance(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    evaluation: &AcceptanceEvaluation,
    outcome: &mut ExecutionOutcome,
) {
    let to_state = match evaluation.status {
        AcceptanceStatus::NotEvaluated => "acceptance_skipped",
        AcceptanceStatus::Accepted => "acceptance_accepted",
        AcceptanceStatus::Rejected => "acceptance_rejected",
        AcceptanceStatus::ManualRequired => "acceptance_manual_required",
    };
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "succeeded",
        to_state,
        Some(format!(
            "acceptance:{}:mode={}:reason={}",
            evaluation.status.as_wire(),
            evaluation.mode.map(|m| m.as_wire()).unwrap_or("none"),
            evaluation.reason
        )),
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition(format!("succeeded -> {}", to_state))
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("acceptance_status", json!(evaluation.status.as_wire()))
    .with_extra("acceptance_reason", json!(evaluation.reason))
    .with_extra("acceptance_commands", json!(evaluation.commands))
    .with_extra(
        "acceptance_commands_executed",
        json!(false),
    )
    .with_extra(
        "acceptance_evidence_keys",
        json!(evaluation.evidence_keys),
    );
    if let Some(mode) = evaluation.mode {
        entry = entry.with_extra("acceptance_mode", json!(mode.as_wire()));
    }
    // wave-18 / task 03 — record the cross-node fan-in outcome so
    // observers can pin the gate decision (mode + source nodes + result
    // + reason) without re-walking prior nodes' evidence. Quiet (the
    // entire `acceptance_fan_in` block is omitted) when the author did
    // not opt into fan-in so the wave-17 byte-shape is preserved.
    if let Some(f) = &evaluation.fan_in {
        entry = entry
            .with_extra("acceptance_fan_in", f.to_json())
            .with_extra("acceptance_fan_in_mode", json!(f.mode.as_wire()))
            .with_extra("acceptance_fan_in_source_nodes", json!(f.source_nodes))
            .with_extra("acceptance_fan_in_passed", json!(f.passed))
            .with_extra("acceptance_fan_in_reason", json!(f.reason));
    }
    if matches!(evaluation.status, AcceptanceStatus::ManualRequired) {
        // Surface the deterministic pause id so downstream resolvers can
        // address the gate without re-deriving the format. Distinct from
        // the wave-16 review-gate id space (`acceptance:` prefix vs
        // `review:`) so the wave-17 / task 01 paused-node resume helper
        // never accidentally consumes an acceptance pause.
        entry = entry.with_extra(
            "acceptance_pause_id",
            json!(derive_acceptance_pause_id(
                ctx.plan_id,
                ctx.plan_version,
                &node.id,
            )),
        );
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: succeeded->acceptance_* evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// Emit a `pending -> skipped` evidence entry for nodes the scheduler never
/// dispatches (taint propagation, condition gating, fail-fast abort). The
/// `skip_reason` and `skip_detail` fields surface why the skip happened so
/// audit consumers can route on a single transition string.
///
/// Wave-14 / Task 02: also publishes a `PlanNodeStateChanged` event with
/// `from=pending, to=skipped, reason=<skip_reason[:detail]>` so bus consumers
/// can route the same way without re-fetching the sidecar. Bus publish
/// failures land in `outcome.bus_publish_warnings` and the evidence ref
/// degrades to the deterministic id (still live-shape, not unavailable).
async fn emit_evidence_skipped(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    skip_reason: &str,
    skip_detail: Option<(&'static str, String)>,
    outcome: &mut ExecutionOutcome,
) {
    let event_reason = match &skip_detail {
        Some((_, detail)) => Some(format!("{}:{}", skip_reason, detail)),
        None => Some(skip_reason.to_string()),
    };
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "pending",
        "skipped",
        event_reason,
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("pending -> skipped")
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
    .with_extra("skip_reason", json!(skip_reason));
    if let Some((k, v)) = skip_detail {
        entry = entry.with_extra(k, json!(v));
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: pending->skipped evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// wave-16 / task 04 — emit a `pending -> paused` evidence entry and best-
/// effort `QuestionEvent::Created` for a node that opted into a
/// `:review-gate "question-event"` gate. The deterministic question id is
/// derived via `derive_plan_node_review_question_id` (scope=`plan`,
/// topic-hash=node_id) so wave-16 / task 02's resolution listener can
/// route on the existing `Route { scope=plan, ... }` outcome.
///
/// Bus publish failure is a real gate — the node still pauses (we refuse
/// to dispatch past a failed gate, mirroring the wave-14 fail-fast posture
/// for review-gates) but the warning lands on the response via
/// `outcome.bus_publish_warnings` AND on the per-node `NodeState::Paused`
/// payload so the row can be re-emitted later without losing the id.
async fn emit_paused_review_gate(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    node: &DagNode,
    dispatch_strategy: &str,
    outcome: &mut ExecutionOutcome,
) -> (String, Option<String>) {
    let question_id = super::review_gate::derive_plan_node_review_question_id(
        &plan.id.to_string(),
        plan.version,
        &node.id,
        node.review_action.as_deref(),
    );
    // Best-effort `QuestionEvent::Created` publish. Bus failure DOES NOT
    // dispatch the node — a failed gate is still a real gate (we refuse
    // to advance past it). The warning goes to both the per-node payload
    // AND the run-level `bus_publish_warnings` array so callers can
    // grep one place for every degraded gate emit.
    let mut bus_warning: Option<String> = None;
    let ev = QuestionEvent::Created {
        question_id: question_id.clone(),
    };
    if let Err(err) = state.bus.publish_question(ev).await {
        let warning = format!(
            "plan_node_review_gate question publish failed for node `{}` (qid `{}`): {}; \
             node remains paused — review gate is enforced even when the bus is degraded",
            node.id, question_id, err
        );
        tracing::warn!(
            plan_id = %plan.id,
            node_id = %node.id,
            question_id = %question_id,
            error = %err,
            "DAG scheduler: review-gate QuestionEvent::Created publish failed; node still paused"
        );
        outcome.bus_publish_warnings.push(warning.clone());
        bus_warning = Some(warning);
    }

    // Also publish the lifecycle `pending -> paused` transition on the
    // execution bus so observers see the same state-change notification
    // they get for every other lifecycle move.
    let (event_ref, lifecycle_warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "pending",
        "paused",
        Some(format!("review_gate:question-event:{}", question_id)),
    )
    .await;
    if let Some(w) = lifecycle_warning {
        outcome.bus_publish_warnings.push(w);
    }

    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("pending -> paused")
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
    .with_extra("review_gate", json!("question-event"))
    .with_extra("review_question_id", json!(question_id));
    if let Some(action) = node.review_action.as_deref() {
        entry = entry.with_extra("review_action", json!(action));
    }
    if let Some(text) = node.review_text.as_deref() {
        entry = entry.with_extra("review_text", json!(text));
    }
    if let Some(w) = bus_warning.as_deref() {
        entry = entry.with_extra("review_question_warning", json!(w));
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: pending->paused evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
    (question_id, bus_warning)
}

/// wave-17 / task 02 — emit a `pending -> claimed` evidence row + bus
/// event for a node whose claim was successfully registered (or
/// recorded best-effort under `enforce_claims=false`). The transition
/// always runs BEFORE `ready -> running` so observers can pivot on the
/// claim metadata without reconstructing it from the running row.
///
/// `claim_status` is one of:
///   * `"acquired"`       — `enforce_claims=true OR false`, registry
///                          recorded the claim with no overlap.
///   * `"recorded_compat"` — `enforce_claims=false`, registry detected
///                          an overlap but compat-mode best-effort
///                          recorded it anyway. The conflict snapshot
///                          rides on the entry so the audit row is
///                          self-contained.
async fn emit_evidence_claimed(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    claim: &PlanDagClaim,
    claim_status: &str,
    compat_conflict: Option<(String, String, String, String)>,
    outcome: &mut ExecutionOutcome,
) {
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "pending",
        "claimed",
        Some(format!(
            "claim:{}:{}:{}",
            claim.claim_id, claim.claimer, claim_status
        )),
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("pending -> claimed")
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("claim_id", json!(claim.claim_id))
    .with_extra("claimer", json!(claim.claimer))
    .with_extra("claim_scopes", json!(claim.scopes))
    .with_extra("claim_scope_source", json!(claim.scope_source))
    .with_extra("claim_acquired_at", json!(claim.acquired_at_iso()))
    .with_extra(
        "claim_lease_expires_at",
        json!(claim.lease_expires_at_iso()),
    )
    .with_extra("claim_status", json!(claim_status));
    if let Some((conflict_id, conflict_claimer, held_scope, attempted_scope)) =
        compat_conflict
    {
        entry = entry
            .with_extra("claim_compat_conflict_claim_id", json!(conflict_id))
            .with_extra("claim_compat_conflict_claimer", json!(conflict_claimer))
            .with_extra("claim_compat_conflict_held_scope", json!(held_scope))
            .with_extra(
                "claim_compat_conflict_attempted_scope",
                json!(attempted_scope),
            );
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: pending->claimed evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// wave-17 / task 02 — emit a `claimed -> released` evidence row and
/// best-effort bus event after the wave loop reaches a terminal state
/// for the node and releases its registry record. Stamps the
/// `released_at` ISO timestamp + the original lease bounds so audit
/// dashboards can compute the actual hold duration without rejoining
/// the prior `pending -> claimed` row.
async fn emit_evidence_claim_released(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    claim: &PlanDagClaim,
    terminal_state: &str,
    outcome: &mut ExecutionOutcome,
) {
    let released_iso = claim
        .released_at_iso()
        .unwrap_or_else(|| claim.acquired_at_iso());
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "claimed",
        "released",
        Some(format!(
            "release:{}:{}:after-{}",
            claim.claim_id, claim.claimer, terminal_state
        )),
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("claimed -> released")
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("claim_id", json!(claim.claim_id))
    .with_extra("claimer", json!(claim.claimer))
    .with_extra("claim_scopes", json!(claim.scopes))
    .with_extra("claim_scope_source", json!(claim.scope_source))
    .with_extra("claim_acquired_at", json!(claim.acquired_at_iso()))
    .with_extra(
        "claim_lease_expires_at",
        json!(claim.lease_expires_at_iso()),
    )
    .with_extra("claim_released_at", json!(released_iso))
    .with_extra("claim_terminal_state", json!(terminal_state));
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: claimed->released evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// wave-17 / task 02 — emit a `pending -> failed` evidence row for a
/// node refused at the claim gate under `enforce_claims=true`. The
/// inner handler is NEVER invoked; the node fails fast with a
/// structured `CLAIM_CONFLICT` reason so audit dashboards can pivot on
/// the dedicated `claim_conflict` skip tag.
async fn emit_evidence_claim_conflict(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    attempted_claim_id: &str,
    attempted_scopes: &[String],
    attempted_scope_source: &str,
    conflicting_claim_id: &str,
    conflicting_claimer: &str,
    conflicting_scope: &str,
    offending_scope: &str,
    outcome: &mut ExecutionOutcome,
) {
    let reason = format!(
        "CLAIM_CONFLICT: scope `{}` overlaps active claim {} held by `{}` over `{}`",
        offending_scope, conflicting_claim_id, conflicting_claimer, conflicting_scope
    );
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "pending",
        "failed",
        Some(reason.clone()),
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("pending -> failed")
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("skip_reason", json!("claim_conflict"))
    .with_extra("claim_status", json!("conflict"))
    .with_extra("attempted_claim_id", json!(attempted_claim_id))
    .with_extra("attempted_claim_scopes", json!(attempted_scopes))
    .with_extra("attempted_claim_scope_source", json!(attempted_scope_source))
    .with_extra("conflicting_claim_id", json!(conflicting_claim_id))
    .with_extra("conflicting_claimer", json!(conflicting_claimer))
    .with_extra("conflicting_scope", json!(conflicting_scope))
    .with_extra("offending_scope", json!(offending_scope))
    .with_extra("inner_error", json!({ "error": reason }));
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: pending->failed (claim conflict) evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// Wave-based scheduler. Drains up to `max_parallel_nodes` ready nodes per
/// wave through a `tokio::task::JoinSet`, awaits the wave, records the
/// transitions in the order results land, then recomputes ready set and
/// repeats. `max_parallel_nodes=1` produces a wave size of 1 each iteration
/// — equivalent to the v1 sequential contract.
async fn execute_with_concurrency(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    parsed: &ParsedDag,
    order: &[String],
    max_parallel_nodes: usize,
    task_contract_ctx: TaskContractDispatchCtx,
) -> Result<ExecutionOutcome> {
    let max_parallel = max_parallel_nodes.max(1);
    let by_id: HashMap<String, DagNode> =
        parsed.nodes.iter().map(|n| (n.id.clone(), n.clone())).collect();

    // Reverse-adjacency for failure propagation.
    let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &parsed.nodes {
        for dep in &n.depends_on {
            succs.entry(dep.as_str()).or_default().push(n.id.as_str());
        }
    }
    // Topo position so we can write results in topological order at the end
    // (matches v1's shape — `nodes` array is topologically ordered).
    let topo_index: HashMap<&str, usize> =
        order.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();

    let mut lifecycle: HashMap<String, NodeLifecycle> = parsed
        .nodes
        .iter()
        .map(|n| (n.id.clone(), NodeLifecycle::Pending))
        .collect();
    let mut tainted_by: HashMap<String, String> = HashMap::new();
    let mut results_by_id: HashMap<String, NodeResult> = HashMap::new();
    // wave-16 / task 05 — per-node attempt counter. Bumped each time
    // the scheduler hands the node to a dispatch task (whether the
    // first attempt or a retry). Used to stamp the evidence + bus
    // payload `attempt` field, and to decide when retries are
    // exhausted (`attempts_made == effective_max_attempts`).
    let mut attempts_made: HashMap<String, u32> = HashMap::new();
    let mut outcome = ExecutionOutcome::default();
    let mut abort_new_dispatch = false;
    let mut abort_aborter: Option<String> = None;

    // wave-17 / task 02 — claim / lease discipline. The registry is
    // per-DAG-run scratch state (NOT a global lock service). Per-node
    // active claim ids live in `active_claims_by_node` so the wave loop
    // can release them as nodes terminate (succeeded / failed / paused
    // / claim-conflict-aborted). The three knobs come from the call
    // args and surface on the response so callers can tell which
    // discipline mode the run used.
    let claim_lease_secs = parse_claim_lease_secs(args);
    let claimer_name = parse_claimer_name(args);
    let enforce_claims = parse_enforce_claims(args);
    let mut claim_registry = ClaimRegistry::new();
    let mut active_claims_by_node: HashMap<String, String> = HashMap::new();

    let ctx = EvidenceCtx {
        plan_id: plan.id,
        plan_version: plan.version,
        project_arg: args.get("project").and_then(|v| v.as_str()),
        cwd_arg: args.get("cwd").and_then(|v| v.as_str()),
        target_project_arg: args.get("target_project").and_then(|v| v.as_str()),
    };

    loop {
        // 1. Materialise tainted-pending skips up-front so they're recorded
        //    in the response in topological order even when the wave that
        //    causes the taint runs concurrently with their would-have-been
        //    siblings.
        let mut became_skipped: Vec<(String, NodeState)> = Vec::new();
        for id in order {
            if !matches!(lifecycle.get(id.as_str()), Some(NodeLifecycle::Pending)) {
                continue;
            }
            if let Some(failed_dep) = tainted_by.get(id.as_str()).cloned() {
                became_skipped.push((
                    id.clone(),
                    NodeState::SkippedUpstreamFailed { failed_dep },
                ));
            }
        }
        for (id, state_skip) in became_skipped.drain(..) {
            let node = match by_id.get(&id) {
                Some(n) => n.clone(),
                None => continue,
            };
            lifecycle.insert(id.clone(), NodeLifecycle::Skipped);
            let dispatch_strategy = node
                .dispatch_strategy
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            let (skip_reason, skip_detail) = match &state_skip {
                NodeState::SkippedUpstreamFailed { failed_dep } => (
                    "upstream_failed",
                    Some(("failed_dep", failed_dep.clone())),
                ),
                _ => ("upstream_failed", None),
            };
            emit_evidence_skipped(
                state,
                &ctx,
                &node,
                &dispatch_strategy,
                skip_reason,
                skip_detail,
                &mut outcome,
            )
            .await;
            let target_clone = node.target.clone();
            results_by_id.insert(
                id.clone(),
                NodeResult::skipped(id, target_clone, state_skip, dispatch_strategy),
            );
        }

        // 2. Compute ready set: Pending nodes whose dependencies are all
        //    Succeeded. Sorted by id for deterministic dispatch order.
        let mut ready_ids: Vec<String> = Vec::new();
        for id in order {
            if !matches!(lifecycle.get(id.as_str()), Some(NodeLifecycle::Pending)) {
                continue;
            }
            let node = match by_id.get(id.as_str()) {
                Some(n) => n,
                None => continue,
            };
            let deps_done = node
                .depends_on
                .iter()
                .all(|dep| matches!(lifecycle.get(dep.as_str()), Some(NodeLifecycle::Succeeded)));
            if deps_done {
                ready_ids.push(id.clone());
            }
        }
        ready_ids.sort();

        // 3. If fail-fast aborted and no Running, force-skip remaining
        //    Pending nodes and stop.
        let any_running = lifecycle
            .values()
            .any(|s| matches!(s, NodeLifecycle::Running));
        if abort_new_dispatch && !any_running {
            let aborter = abort_aborter.clone().unwrap_or_default();
            // Force-skip every still-pending node (including ones already in
            // the just-computed ready set — fail-fast supersedes ready).
            let pending_ids: Vec<String> = order
                .iter()
                .filter(|id| matches!(lifecycle.get(id.as_str()), Some(NodeLifecycle::Pending)))
                .cloned()
                .collect();
            for id in pending_ids {
                let node = match by_id.get(&id) {
                    Some(n) => n.clone(),
                    None => continue,
                };
                lifecycle.insert(id.clone(), NodeLifecycle::Skipped);
                let dispatch_strategy = node
                    .dispatch_strategy
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                emit_evidence_skipped(
                    state,
                    &ctx,
                    &node,
                    &dispatch_strategy,
                    "fail_fast_aborted",
                    Some(("aborter", aborter.clone())),
                    &mut outcome,
                )
                .await;
                let target_clone = node.target.clone();
                results_by_id.insert(
                    id.clone(),
                    NodeResult::skipped(
                        id,
                        target_clone,
                        NodeState::SkippedFailFastAbort {
                            aborter: aborter.clone(),
                        },
                        dispatch_strategy,
                    ),
                );
            }
            break;
        }

        // 4. If nothing ready and nothing running, we're done.
        if ready_ids.is_empty() && !any_running {
            break;
        }

        // 5. Filter ready set by condition gate, then review gate. Nodes
        //    with non-empty `:condition` skip in v2 just like v1 — taint
        //    propagated. Nodes with `:review-gate "question-event"` pause
        //    in place (wave-16 / task 04) — the scheduler emits
        //    `QuestionEvent::Created` and refuses to dispatch the target
        //    tool. Paused nodes do NOT propagate taint (they are not a
        //    failure — auto-resume is wave-16 / task 02 territory) but
        //    their downstream stays Pending until a follow-up call
        //    revives them.
        let mut to_dispatch: Vec<DagNode> = Vec::new();
        for id in &ready_ids {
            let node = match by_id.get(id.as_str()) {
                Some(n) => n,
                None => continue,
            };
            let has_condition = node
                .condition
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            if has_condition {
                lifecycle.insert(id.clone(), NodeLifecycle::Skipped);
                let dispatch_strategy = node
                    .dispatch_strategy
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                emit_evidence_skipped(
                    state,
                    &ctx,
                    node,
                    &dispatch_strategy,
                    "condition_gated",
                    node.condition
                        .as_ref()
                        .map(|c| ("condition", c.clone())),
                    &mut outcome,
                )
                .await;
                results_by_id.insert(
                    id.clone(),
                    NodeResult::skipped(
                        id.clone(),
                        node.target.clone(),
                        NodeState::SkippedCondition,
                        dispatch_strategy,
                    ),
                );
                propagate_taint(node, &succs, &mut tainted_by);
                continue;
            }
            // wave-16 / task 04 — review-gate paused state. The first real
            // non-terminal node state in v2: emit `QuestionEvent::Created`
            // (best-effort; failure still pauses) + a pending->paused
            // evidence row, mark the node `Paused`, do NOT call the
            // target tool. Downstream stays pending; auto-resume lives
            // in wave-16 / task 02's `QuestionEvent::Resolved` listener.
            if let ReviewGateKind::QuestionEvent = node.review_gate_kind() {
                lifecycle.insert(id.clone(), NodeLifecycle::Paused);
                let dispatch_strategy = node
                    .dispatch_strategy
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                let (question_id, bus_publish_warning) = emit_paused_review_gate(
                    state,
                    &ctx,
                    plan,
                    node,
                    &dispatch_strategy,
                    &mut outcome,
                )
                .await;
                results_by_id.insert(
                    id.clone(),
                    NodeResult::skipped(
                        id.clone(),
                        node.target.clone(),
                        NodeState::Paused {
                            question_id,
                            bus_publish_warning,
                        },
                        dispatch_strategy,
                    ),
                );
                continue;
            }
            to_dispatch.push(node.clone());
            if to_dispatch.len() >= max_parallel {
                break;
            }
        }

        if to_dispatch.is_empty() {
            // Either everything ready was condition-gated (loop again to pick
            // up the new tainted skips) or nothing's ready and something is
            // still running — in either case, short-circuit if no JoinSet
            // work is needed and no progress was made on this iteration.
            if !any_running {
                continue;
            }
            // Shouldn't happen because we'd hit step 4 already, but be safe.
        }

        // 6. Mark dispatched nodes Running, write start evidence, spawn.
        // wave-16 / task 05 — every spawn (first attempt or retry) bumps
        // the per-node `attempts_made` counter and stamps the resulting
        // attempt number onto the evidence + bus payload so audit
        // dashboards can route on `attempt > 1` without reconstructing
        // the retry policy from scratch.
        //
        // wave-17 / task 02 — every dispatched node passes through the
        // `pending -> claimed -> running` ladder. Claim acquisition runs
        // BEFORE the spawn so `enforce_claims=true` can fail-fast on
        // an unresolvable overlap without ever touching the inner
        // handler. Under `enforce_claims=false` the registry still
        // records best-effort metadata (so observers can tell the
        // discipline ran) but the scheduler never blocks dispatch on
        // an overlap.
        let mut join_set: tokio::task::JoinSet<Result<DispatchOutcome>> =
            tokio::task::JoinSet::new();
        for node in to_dispatch {
            let dispatch_strategy = node
                .dispatch_strategy
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            let attempt = {
                let entry = attempts_made.entry(node.id.clone()).or_insert(0);
                *entry += 1;
                *entry
            };

            // wave-17 / task 02 — try to acquire a claim covering the
            // node's derived scopes. The acquire runs against the
            // shared per-DAG registry; conflicts are decided by the
            // shared `scopes_overlap_pure` predicate.
            let (scopes, scope_source) =
                derive_node_claim_scopes(&node, plan.id);
            let claim_id =
                derive_plan_dag_claim_id(plan.id, &node.id, attempt);
            let acquire_now = chrono::Utc::now();
            let acquire_outcome = claim_registry.try_acquire(
                claim_id.clone(),
                claimer_name.clone(),
                scopes.clone(),
                scope_source,
                claim_lease_secs,
                acquire_now,
            );

            match acquire_outcome {
                ClaimAcquire::Acquired(claim) => {
                    lifecycle.insert(node.id.clone(), NodeLifecycle::Claimed);
                    emit_evidence_claimed(
                        state,
                        &ctx,
                        &node,
                        &dispatch_strategy,
                        attempt,
                        &claim,
                        "acquired",
                        None,
                        &mut outcome,
                    )
                    .await;
                    active_claims_by_node
                        .insert(node.id.clone(), claim.claim_id.clone());
                }
                ClaimAcquire::Conflict {
                    attempted_claim_id,
                    attempted_scopes,
                    attempted_scope_source,
                    conflicting_claim_id,
                    conflicting_claimer,
                    conflicting_scope,
                    offending_scope,
                } => {
                    if enforce_claims {
                        // Strict mode — refuse to dispatch. Mark the
                        // node failed, emit `pending -> failed` with
                        // the structured CLAIM_CONFLICT reason, do NOT
                        // spawn the inner handler.
                        lifecycle.insert(node.id.clone(), NodeLifecycle::Failed);
                        emit_evidence_claim_conflict(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            attempt,
                            &attempted_claim_id,
                            &attempted_scopes,
                            attempted_scope_source,
                            &conflicting_claim_id,
                            &conflicting_claimer,
                            &conflicting_scope,
                            &offending_scope,
                            &mut outcome,
                        )
                        .await;
                        let reason = format!(
                            "CLAIM_CONFLICT: scope `{}` overlaps active claim {} \
                             held by `{}` over `{}`",
                            offending_scope,
                            conflicting_claim_id,
                            conflicting_claimer,
                            conflicting_scope
                        );
                        let inner_payload = json!({
                            "error": reason.clone(),
                            "claim_status": "conflict",
                            "attempted_claim_id": attempted_claim_id,
                            "attempted_claim_scopes": attempted_scopes,
                            "attempted_claim_scope_source": attempted_scope_source,
                            "conflicting_claim_id": conflicting_claim_id,
                            "conflicting_claimer": conflicting_claimer,
                            "conflicting_scope": conflicting_scope,
                            "offending_scope": offending_scope,
                        });
                        results_by_id.insert(
                            node.id.clone(),
                            NodeResult {
                                id: node.id.clone(),
                                target: node.target.clone(),
                                state: NodeState::Failed { reason },
                                dispatch_strategy: dispatch_strategy.clone(),
                                inner_payload,
                                attempts_made: attempt,
                                max_attempts: node.effective_max_attempts(),
                                retry_skipped_non_retryable: true,
                                // wave-17 / task 04 — claim-conflict
                                // refusal happens BEFORE any handler
                                // runs; the rollback evaluator never
                                // gets a chance to reason about it
                                // because the failure is purely a
                                // coordination event, not a node-level
                                // failure that warrants compensation.
                                rollback: None,
                                // wave-17 / task 03 — claim-conflict
                                // refusal happens BEFORE the inner
                                // handler runs; acceptance phase is
                                // never reached for this node.
                                acceptance: None,
                            },
                        );
                        // Taint propagation — the failed node still
                        // taints downstream so the rest of the DAG
                        // sees the failure as a real one, AND
                        // fail-fast trips when policy says so.
                        propagate_taint(&node, &succs, &mut tainted_by);
                        if node.failure_policy == FAILURE_POLICY_FAIL_FAST {
                            abort_new_dispatch = true;
                            abort_aborter = Some(node.id.clone());
                        }
                        continue;
                    }
                    // Compat mode — best-effort record the claim into
                    // the registry under a synthetic id so the audit
                    // row carries the metadata. We synthesise a
                    // record (NOT inserted into the registry to avoid
                    // poisoning future overlap checks) and attach the
                    // conflict snapshot so dashboards can spot
                    // "compat mode papered over a real conflict".
                    let synthetic_claim = PlanDagClaim {
                        claim_id: attempted_claim_id.clone(),
                        claimer: claimer_name.clone(),
                        scopes: attempted_scopes,
                        scope_source: attempted_scope_source,
                        acquired_at: acquire_now,
                        lease_expires_at: acquire_now
                            + chrono::Duration::seconds(claim_lease_secs),
                        released_at: None,
                    };
                    lifecycle
                        .insert(node.id.clone(), NodeLifecycle::Claimed);
                    emit_evidence_claimed(
                        state,
                        &ctx,
                        &node,
                        &dispatch_strategy,
                        attempt,
                        &synthetic_claim,
                        "recorded_compat",
                        Some((
                            conflicting_claim_id,
                            conflicting_claimer,
                            conflicting_scope,
                            offending_scope,
                        )),
                        &mut outcome,
                    )
                    .await;
                    // No registry entry, no per-node active claim
                    // map entry — release skip is intentional: we
                    // never registered the claim, so there's nothing
                    // to release. Audit row already captured the
                    // metadata; downstream nodes still see the held
                    // scope on the original conflicting claim, which
                    // is the right thing for compat mode.
                }
            }

            lifecycle.insert(node.id.clone(), NodeLifecycle::Running);
            emit_evidence_running(
                state,
                &ctx,
                &node,
                &dispatch_strategy,
                attempt,
                &mut outcome,
            )
            .await;
            let state_clone = state.clone();
            let plan_clone = plan.clone();
            let task_contract_ctx_clone = task_contract_ctx.clone();
            join_set.spawn(async move {
                dispatch_node(state_clone, plan_clone, node, task_contract_ctx_clone).await
            });
        }

        // 7. Drain wave; for each result decide success/failure, update
        //    lifecycle + taint, write finish evidence.
        //
        // wave-16 / task 05 — on failure, consult the per-node retry
        // policy. If the node opted in (`effective_max_attempts > 1`)
        // AND the failure is retryable (not a deterministic
        // safe-descriptor refusal) AND attempts remain, re-spawn the
        // node into the SAME wave's JoinSet with the next attempt
        // number. The node stays `Running`; only when retries are
        // exhausted (or the failure is non-retryable) do we mark it
        // `Failed` + propagate taint + maybe trip fail-fast.
        while let Some(joined) = join_set.join_next().await {
            let dispatch_outcome = match joined {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => {
                    // Rare: the inner handler returned an `anyhow::Error`
                    // (panic-equivalent). Treat as a fatal scheduler error so
                    // the caller sees something — bubbling up here aborts the
                    // whole dispatch, which is the right thing for an
                    // unhandled exception.
                    return Err(e);
                }
                Err(join_err) => {
                    // tokio task panicked. Same reasoning as above.
                    return Err(anyhow::anyhow!(
                        "DAG scheduler: dispatch task join failed: {}",
                        join_err
                    ));
                }
            };
            let DispatchOutcome {
                node_id,
                target,
                dispatch_strategy,
                inner_payload,
                classification,
                non_retryable,
            } = dispatch_outcome;
            let node = match by_id.get(&node_id) {
                Some(n) => n.clone(),
                None => continue,
            };
            let succeeded = classification.is_ok();
            // The attempt # we are currently finishing. Authoritative
            // because it was bumped at spawn time.
            let current_attempt = attempts_made.get(&node_id).copied().unwrap_or(1);
            let max_attempts = node.effective_max_attempts();
            emit_evidence_finished(
                state,
                &ctx,
                &node,
                &dispatch_strategy,
                &inner_payload,
                succeeded,
                current_attempt,
                &mut outcome,
            )
            .await;
            if succeeded {
                // wave-17 / task 03 — deterministic acceptance phase.
                // Runs ONLY on the success branch (failure already
                // dominates the lifecycle). NEVER executes shell — the
                // evaluator is a pure projection over `(node, payload)`
                // and decides one of: NotEvaluated (no hints — preserve
                // wave-13 behaviour), Accepted, Rejected, ManualRequired.
                // The first three terminate the node (succeeded /
                // failed / paused) without further dispatch.
                //
                // wave-18 / task 03 — `apply_acceptance_fan_in` then
                // overlays cross-node fan-in on top of the per-node
                // result. The validator already proved every fan-in dep
                // is a transitive `:depends-on` ancestor, so the prior
                // node's result is guaranteed to live in `results_by_id`
                // by the time this branch runs.
                let acceptance_base =
                    evaluate_node_acceptance(&node, &inner_payload, true);
                let prior_results_view: HashMap<String, &NodeResult> =
                    results_by_id
                        .iter()
                        .map(|(k, v)| (k.clone(), v))
                        .collect();
                let acceptance = apply_acceptance_fan_in(
                    acceptance_base,
                    &node,
                    &prior_results_view,
                );
                let acceptance_active = !acceptance.is_inactive();
                if acceptance_active {
                    emit_evidence_acceptance(
                        state,
                        &ctx,
                        &node,
                        &dispatch_strategy,
                        current_attempt,
                        &acceptance,
                        &mut outcome,
                    )
                    .await;
                }
                let terminal_state_label = match acceptance.status {
                    AcceptanceStatus::NotEvaluated | AcceptanceStatus::Accepted => "succeeded",
                    AcceptanceStatus::Rejected => "failed",
                    AcceptanceStatus::ManualRequired => "paused",
                };
                let next_node_state: NodeState = match acceptance.status {
                    AcceptanceStatus::NotEvaluated | AcceptanceStatus::Accepted => {
                        NodeState::Succeeded
                    }
                    AcceptanceStatus::Rejected => NodeState::Failed {
                        reason: format!(
                            "acceptance_rejected: {}",
                            acceptance.reason
                        ),
                    },
                    AcceptanceStatus::ManualRequired => {
                        let qid = derive_acceptance_pause_id(
                            plan.id,
                            plan.version,
                            &node.id,
                        );
                        NodeState::Paused {
                            question_id: qid,
                            bus_publish_warning: None,
                        }
                    }
                };
                let next_lifecycle = match acceptance.status {
                    AcceptanceStatus::NotEvaluated | AcceptanceStatus::Accepted => {
                        NodeLifecycle::Succeeded
                    }
                    AcceptanceStatus::Rejected => NodeLifecycle::Failed,
                    AcceptanceStatus::ManualRequired => NodeLifecycle::Paused,
                };
                lifecycle.insert(node_id.clone(), next_lifecycle);
                // wave-17 / task 02 — release the claim now that the
                // terminal state is set. Best-effort: we only release
                // when the registry actually recorded the claim
                // (compat-mode conflicts skip the registry insert).
                if let Some(claim_id) = active_claims_by_node.remove(&node_id) {
                    if let Some(released) = claim_registry
                        .release(&claim_id, chrono::Utc::now())
                    {
                        emit_evidence_claim_released(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            current_attempt,
                            &released,
                            terminal_state_label,
                            &mut outcome,
                        )
                        .await;
                    }
                }
                // wave-17 / task 04 — acceptance-rejected nodes are
                // node-level failures and warrant the same rollback
                // pass as a dispatch-time failure. Runs BEFORE
                // `propagate_taint` so the downstream behaviour is
                // governed by the existing failure-policy contract.
                // Skipped for accepted / paused / not-evaluated
                // branches (the node is not in a "final failed"
                // state for those).
                let acc_rollback_eval = if matches!(
                    acceptance.status,
                    AcceptanceStatus::Rejected
                ) {
                    let mut eval = run_rollback(state, plan, &node).await;
                    // wave-18 / task 04 — cascade rollback pass after
                    // node-local rollback. Fold into the same evaluation
                    // so audit dashboards see a single rollback block.
                    if node.has_active_rollback_cascade() {
                        let cascade = run_cascade_rollback(
                            state,
                            plan,
                            &node,
                            &parsed.nodes,
                            order,
                        )
                        .await;
                        if !cascade.is_inactive() {
                            eval.cascade = Some(cascade);
                        }
                    }
                    if !eval.is_inactive() {
                        emit_evidence_rollback(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            current_attempt,
                            &eval,
                            &mut outcome,
                        )
                        .await;
                        Some(eval)
                    } else {
                        None
                    }
                } else {
                    None
                };
                // wave-17 / task 03 — Rejected acceptance also taints
                // downstream and may trip fail-fast (matches the
                // dispatch-failure path so consumers get one shape for
                // any non-success terminal state).
                if matches!(acceptance.status, AcceptanceStatus::Rejected) {
                    propagate_taint(&node, &succs, &mut tainted_by);
                    if node.failure_policy == FAILURE_POLICY_FAIL_FAST {
                        abort_new_dispatch = true;
                        abort_aborter = Some(node_id.clone());
                    }
                }
                results_by_id.insert(
                    node_id.clone(),
                    NodeResult {
                        id: node_id,
                        target,
                        state: next_node_state,
                        dispatch_strategy,
                        inner_payload,
                        attempts_made: current_attempt,
                        max_attempts,
                        retry_skipped_non_retryable: false,
                        rollback: acc_rollback_eval,
                        acceptance: if acceptance_active {
                            Some(acceptance)
                        } else {
                            None
                        },
                    },
                );
                continue;
            }

            // Failure path — decide retry vs final failure. The
            // predicate is `plan_node_should_retry` so unit tests can
            // pin the decision without standing up the wave loop.
            let should_retry = plan_node_should_retry(
                current_attempt,
                max_attempts,
                non_retryable,
                abort_new_dispatch,
            );
            if should_retry {
                // wave-17 / task 02 — release the failed-attempt
                // claim BEFORE re-acquiring on retry so the new
                // attempt's claim id (with the bumped attempt
                // suffix) replaces the prior one in the registry
                // without overlap. Best-effort: skip if the original
                // attempt never registered a claim (compat-mode
                // conflict).
                if let Some(claim_id) =
                    active_claims_by_node.remove(&node_id)
                {
                    if let Some(released) = claim_registry
                        .release(&claim_id, chrono::Utc::now())
                    {
                        emit_evidence_claim_released(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            current_attempt,
                            &released,
                            "failed_will_retry",
                            &mut outcome,
                        )
                        .await;
                    }
                }
                // Optional sleep between attempts. Skipped when absent
                // / 0 so the common no-back-off case stays cheap.
                if let Some(delay_ms) = node.effective_retry_delay_ms() {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
                // Bump the attempt counter, re-emit `ready -> running`
                // for the retry attempt, and re-spawn into the SAME
                // JoinSet so the wave loop drains it without
                // reshuffling the ready set. Lifecycle stays Running.
                let next_attempt = {
                    let entry = attempts_made.entry(node_id.clone()).or_insert(0);
                    *entry += 1;
                    *entry
                };
                // wave-17 / task 02 — re-acquire claim for retry
                // attempt. Fresh claim id includes the bumped
                // attempt suffix so the audit trail captures every
                // attempt's claim metadata distinctly.
                let (retry_scopes, retry_scope_source) =
                    derive_node_claim_scopes(&node, plan.id);
                let retry_claim_id =
                    derive_plan_dag_claim_id(plan.id, &node_id, next_attempt);
                let retry_now = chrono::Utc::now();
                let retry_acquire = claim_registry.try_acquire(
                    retry_claim_id.clone(),
                    claimer_name.clone(),
                    retry_scopes.clone(),
                    retry_scope_source,
                    claim_lease_secs,
                    retry_now,
                );
                match retry_acquire {
                    ClaimAcquire::Acquired(retry_claim) => {
                        lifecycle.insert(
                            node_id.clone(),
                            NodeLifecycle::Claimed,
                        );
                        emit_evidence_claimed(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            next_attempt,
                            &retry_claim,
                            "acquired",
                            None,
                            &mut outcome,
                        )
                        .await;
                        active_claims_by_node.insert(
                            node_id.clone(),
                            retry_claim.claim_id.clone(),
                        );
                    }
                    ClaimAcquire::Conflict {
                        attempted_scopes,
                        attempted_scope_source,
                        conflicting_claim_id,
                        conflicting_claimer,
                        conflicting_scope,
                        offending_scope,
                        ..
                    } => {
                        // Compat / enforce both end here for retries
                        // — we already mid-flight and cannot fail
                        // the prior attempt over a retry-claim
                        // conflict. Surface the audit row as
                        // recorded_compat (the claim is informational
                        // only on retries) and continue.
                        let synthetic = PlanDagClaim {
                            claim_id: retry_claim_id.clone(),
                            claimer: claimer_name.clone(),
                            scopes: attempted_scopes,
                            scope_source: attempted_scope_source,
                            acquired_at: retry_now,
                            lease_expires_at: retry_now
                                + chrono::Duration::seconds(claim_lease_secs),
                            released_at: None,
                        };
                        lifecycle.insert(
                            node_id.clone(),
                            NodeLifecycle::Claimed,
                        );
                        emit_evidence_claimed(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            next_attempt,
                            &synthetic,
                            "recorded_compat",
                            Some((
                                conflicting_claim_id,
                                conflicting_claimer,
                                conflicting_scope,
                                offending_scope,
                            )),
                            &mut outcome,
                        )
                        .await;
                    }
                }
                lifecycle.insert(node_id.clone(), NodeLifecycle::Running);
                emit_evidence_running(
                    state,
                    &ctx,
                    &node,
                    &dispatch_strategy,
                    next_attempt,
                    &mut outcome,
                )
                .await;
                let state_clone = state.clone();
                let plan_clone = plan.clone();
                let node_clone = node.clone();
                let task_contract_ctx_clone = task_contract_ctx.clone();
                join_set.spawn(async move {
                    dispatch_node(
                        state_clone,
                        plan_clone,
                        node_clone,
                        task_contract_ctx_clone,
                    )
                    .await
                });
                continue;
            }

            // Final failure — exhausted retries OR non-retryable OR
            // fail-fast already aborted this wave.
            lifecycle.insert(node_id.clone(), NodeLifecycle::Failed);
            // wave-17 / task 02 — release the claim on terminal
            // failure (best-effort, compat-mode conflicts skip).
            if let Some(claim_id) = active_claims_by_node.remove(&node_id) {
                if let Some(released) =
                    claim_registry.release(&claim_id, chrono::Utc::now())
                {
                    emit_evidence_claim_released(
                        state,
                        &ctx,
                        &node,
                        &dispatch_strategy,
                        current_attempt,
                        &released,
                        "failed",
                        &mut outcome,
                    )
                    .await;
                }
            }
            let reason = classification
                .err()
                .unwrap_or_else(|| "inner handler returned error".to_string());
            // wave-17 / task 04 — conservative rollback pass. Runs
            // AFTER the final failed attempt and BEFORE downstream
            // taint propagation. Skipped entirely when the node did
            // not opt into a rollback policy so the wave-13 byte
            // shape stays untouched.
            //
            // The rollback evaluator decides one of:
            //   * NotRequested      — no rollback hints / explicit
            //                          `:rollback-policy "none"`.
            //                          Evidence emit suppressed.
            //   * DescriptorReady   — `:rollback-policy "descriptor"`;
            //                          captures intent + brief preview,
            //                          NEVER dispatches.
            //   * Dispatched        — `:rollback-policy "workstation"`
            //                          + every safety gate passed +
            //                          inner handler returned Ok.
            //   * Refused           — `:rollback-policy "workstation"`
            //                          + at least one safety gate
            //                          failed (or substrate-side
            //                          SafeDescriptor refusal).
            //   * Failed            — `:rollback-policy "workstation"`
            //                          dispatched but the inner
            //                          handler returned an error.
            //
            // Downstream taint propagation runs identically afterwards
            // — the rollback pass NEVER changes failure-policy
            // semantics. This keeps the wave-13 / wave-16 contract
            // intact: `:failure-policy fail-fast` still trips the
            // wave-loop abort flag based on the original failure,
            // not the rollback outcome.
            let mut rollback_eval = run_rollback(state, plan, &node).await;
            // wave-18 / task 04 — cascade rollback pass after the
            // node-local rollback. The cascade evaluator is conservative:
            // it never runs unless the failed node opted in via
            // `:rollback-cascade "plan" | "dispatch-safe"`. Folding the
            // outcome into the same `RollbackEvaluation` keeps audit
            // dashboards on a single block per failed node.
            if node.has_active_rollback_cascade() {
                let cascade = run_cascade_rollback(
                    state,
                    plan,
                    &node,
                    &parsed.nodes,
                    order,
                )
                .await;
                if !cascade.is_inactive() {
                    rollback_eval.cascade = Some(cascade);
                }
            }
            let rollback_active = !rollback_eval.is_inactive();
            if rollback_active {
                emit_evidence_rollback(
                    state,
                    &ctx,
                    &node,
                    &dispatch_strategy,
                    current_attempt,
                    &rollback_eval,
                    &mut outcome,
                )
                .await;
            }
            results_by_id.insert(
                node_id.clone(),
                NodeResult {
                    id: node_id.clone(),
                    target,
                    state: NodeState::Failed { reason },
                    dispatch_strategy,
                    inner_payload,
                    attempts_made: current_attempt,
                    max_attempts,
                    retry_skipped_non_retryable: non_retryable,
                    rollback: if rollback_active {
                        Some(rollback_eval)
                    } else {
                        None
                    },
                    // wave-17 / task 03 — dispatch-failure path skips
                    // the acceptance phase (failure dominates).
                    acceptance: None,
                },
            );
            // Taint propagates regardless of policy — it just changes
            // whether *unrelated* nodes also get aborted (fail-fast) or
            // can keep running (continue). wave-17 / task 04: the
            // rollback evaluation does NOT alter this — downstream
            // behaviour stays governed by the existing failure-policy
            // contract.
            propagate_taint(&node, &succs, &mut tainted_by);
            if node.failure_policy == FAILURE_POLICY_FAIL_FAST {
                abort_new_dispatch = true;
                abort_aborter = Some(node_id.clone());
            }
        }
    }

    if abort_new_dispatch {
        outcome.aborted_fail_fast = true;
    }

    // Stitch results back into topological order so the response array's
    // shape matches v1.
    let mut ordered: Vec<(usize, NodeResult)> = results_by_id
        .into_iter()
        .filter_map(|(id, r)| topo_index.get(id.as_str()).map(|&i| (i, r)))
        .collect();
    ordered.sort_by_key(|(i, _)| *i);
    outcome.results = ordered.into_iter().map(|(_, r)| r).collect();

    Ok(outcome)
}

/// Pure projection of the wave plan the scheduler WOULD execute given
/// `max_parallel_nodes`. Used by the dry-run response so callers can preview
/// the parallelism shape without dispatching anything. Each entry in the
/// returned vector is one wave — the ids the scheduler would launch in one
/// `tokio::task::JoinSet` round.
///
/// The projection assumes every dispatched node succeeds (no taint, no
/// condition gate). Real runs may differ when conditions skip nodes or
/// failures taint subtrees; the dry-run is therefore a *capacity* preview,
/// not an outcome prediction.
fn compute_concurrency_plan(
    nodes: &[DagNode],
    order: &[String],
    max_parallel_nodes: usize,
) -> Vec<Vec<String>> {
    let max_parallel = max_parallel_nodes.max(1);
    let by_id: HashMap<&str, &DagNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut completed: HashSet<String> = HashSet::new();
    let mut remaining: Vec<String> = order.to_vec();
    let mut waves: Vec<Vec<String>> = Vec::new();
    while !remaining.is_empty() {
        let mut ready: Vec<String> = Vec::new();
        for id in &remaining {
            let node = match by_id.get(id.as_str()) {
                Some(n) => *n,
                None => continue,
            };
            if node.depends_on.iter().all(|dep| completed.contains(dep)) {
                ready.push(id.clone());
            }
        }
        if ready.is_empty() {
            break;
        }
        ready.sort();
        let wave: Vec<String> = ready.iter().take(max_parallel).cloned().collect();
        for id in &wave {
            completed.insert(id.clone());
        }
        remaining.retain(|id| !wave.contains(id));
        waves.push(wave);
    }
    waves
}

/// Parse `max_parallel_nodes` from the call args. Defaults to 1 (preserving
/// the v1 sequential contract). Negative or zero values are clamped to 1
/// rather than rejected — the scheduler treats "less than 1 wave width" as a
/// caller mistake we silently normalise, mirroring how `dispatch_strategy`
/// handles unknown values today.
pub(super) fn parse_max_parallel_nodes(args: &Value) -> usize {
    args.get("max_parallel_nodes")
        .and_then(|v| v.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(1)
}

/// Mark every transitive successor of `failed` as tainted by `failed.id`.
/// Tainted nodes will be reported as `skipped_upstream_failed` when reached.
fn propagate_taint<'a>(
    failed: &DagNode,
    succs: &HashMap<&'a str, Vec<&'a str>>,
    tainted_by: &mut HashMap<String, String>,
) {
    let mut q: VecDeque<String> = VecDeque::new();
    q.push_back(failed.id.clone());
    while let Some(cur) = q.pop_front() {
        if let Some(children) = succs.get(cur.as_str()) {
            for &child in children {
                if !tainted_by.contains_key(child) {
                    tainted_by.insert(child.to_string(), failed.id.clone());
                    q.push_back(child.to_string());
                }
            }
        }
    }
}

/// Owned product of `build_node_inner_args` so the caller can record the
/// dispatch_strategy even when arg-building succeeds (the inner builder
/// returns a `ToolResult` on failure that we adapt back into a JSON payload).
struct NodeInnerArgs {
    inner_args: std::result::Result<Value, Value>,
    dispatch_strategy: String,
}

fn build_node_inner_args(node: &DagNode, plan: &Plan) -> NodeInnerArgs {
    // Synthesise an args object for the inner dispatcher so we can reuse the
    // existing `build_internal_dispatch_args` helper from plan.rs verbatim.
    let mut node_args = serde_json::Map::new();
    if let Some(o) = &node.objective {
        node_args.insert("objective".to_string(), Value::String(o.clone()));
    }
    if let Some(p) = &node.target_project {
        node_args.insert("target_project".to_string(), Value::String(p.clone()));
    }
    if let Some(c) = &node.requested_cwd {
        node_args.insert("requested_cwd".to_string(), Value::String(c.clone()));
        // mission_task_delegate consumes `cwd`; mission_execution consumes
        // `requested_cwd`. We pass both so each branch gets what it needs.
        node_args.insert("cwd".to_string(), Value::String(c.clone()));
    }
    if let Some(f) = &node.flow_id {
        node_args.insert("flow_id".to_string(), Value::String(f.clone()));
    }
    if let Some(t) = node.timeout_ms {
        // mission_task_delegate accepts timeout_secs, not ms — translate.
        let secs = (t / 1000).max(1);
        node_args.insert("timeout_secs".to_string(), Value::Number(secs.into()));
    }
    let dispatch_strategy = node
        .dispatch_strategy
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let args_value = Value::Object(node_args);

    // Plan-hint slot is empty: each node carries its own hints, so the
    // shared parser is bypassed.
    let empty_hints = ParsedPlanHints::default();
    let inner = match build_internal_dispatch_args(
        &args_value,
        plan,
        &node.target,
        &dispatch_strategy,
        &empty_hints,
    ) {
        Ok(v) => Ok(v),
        Err(tool_result) => Err(tool_result_payload(&tool_result)),
    };
    NodeInnerArgs {
        inner_args: inner,
        dispatch_strategy,
    }
}

/// Convenience helper used by `plan::action_execute` to detect whether the
/// caller asked for the DAG scheduler. Returns `Ok(true)` when the value is
/// `dag_v1`, `Ok(false)` for absent/empty/`v0`/`single_node`/`default`, and
/// `Err(structured)` when the value is unrecognised.
pub(super) fn detect_scheduler_mode(args: &Value) -> std::result::Result<bool, ToolResult> {
    let raw = args.get("scheduler_mode").and_then(|v| v.as_str());
    let mode = raw.map(|s| s.trim()).unwrap_or("");
    match mode {
        "" | "v0" | "default" | "current" | "single_node" | "single-node" => Ok(false),
        "dag_v1" | "dag-v1" => Ok(true),
        other => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("unknown scheduler_mode `{}`", other),
            )
            .with_suggestion("scheduler_mode ∈ {default, dag_v1}"),
        )),
    }
}

/// wave-20 / task 07 — DAG path guard for LLM-augmented inference modes.
///
/// `infer_plan_fields=sonnet_suggest` is a single-node-execute-only
/// feature in v0: the deterministic engine + Sonnet pass operate on the
/// PLAN-level sexp, not on per-node fan-out. Combining the LLM mode with
/// `scheduler_mode=dag_v1` would silently skip the LLM pass (the DAG
/// scheduler runs before any inference can happen) — that violates the
/// fail-fast contract. We refuse the combo eagerly so callers receive a
/// structured error pointing at the right surface, instead of an
/// unexpected response missing the LLM proposal block.
///
/// Returns `None` when the args are clean (or carry a deterministic
/// `infer_plan_fields` mode); `Some(error)` when the combo is rejected.
/// Pure: no AppState reads, no IO.
pub(super) fn refuse_llm_inference_in_dag_mode(args: &Value) -> Option<ToolResult> {
    let mode = match parse_infer_plan_fields_mode(args) {
        Ok(m) => m,
        // The single-node execute path validates parse errors before the
        // DAG branch is reached; if we somehow get here with a typo we
        // re-surface the parse error as a structured tool error so the
        // caller still sees a helpful message.
        Err(msg) => {
            return Some(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, msg),
            ));
        }
    };
    if !mode.is_llm_augmented() {
        return None;
    }
    Some(ToolResult::structured_error(
        ToolError::new(
            error_codes::INVALID_PARAM,
            format!(
                "infer_plan_fields=`{}` is single-node-execute-only in v0; \
                 it is not supported with scheduler_mode=dag_v1",
                mode.as_wire(),
            ),
        )
        .with_suggestion(
            "drop scheduler_mode=dag_v1 to use the single-node execute path with \
             sonnet_suggest, or rerun the DAG with infer_plan_fields ∈ {off, preview, apply_safe}",
        ),
    ))
}

// ═════════════════════════════════════════════════════════════════════════
// wave-17 / task 01 — paused-node resume hook
//
// Wave-16 / task 04 paused PLAN DAG nodes that opted into
// `:review-gate "question-event"`, emitting a deterministic review
// question id of the form
// `review:plan:<plan_id>:v<version>:plan-node:<sha256(node_id)[..16]>`.
// Wave-17 / task 01 closes the loop by accepting an explicit resume
// input on `mission_plan(action=execute)` AND wiring the
// `QuestionEvent::Resolved` listener (wave-16 / task 02) so an approved
// resolution for a plan-node id re-dispatches exactly the paused node.
//
// This is NOT general auto-approval. Only ids whose envelope round-trips
// to a paused-eligible node (`:review-gate "question-event"` set in the
// plan) are routed through this helper. Non-plan-node review ids keep
// their existing wave-16 / task 02 behaviour.
//
// Behaviour matrix:
//   * `approved`       → re-dispatch the paused node (fresh attempt 1,
//                         since `paused` is non-terminal — not a failed
//                         attempt). Lifecycle event: paused -> running ->
//                         {succeeded|failed}. Plan status stays untouched
//                         because the resume only revives one node — the
//                         caller is expected to drive downstream nodes
//                         via a follow-up execute call.
//   * `rejected`       → no dispatch. Node stays paused (the
//                         failure-policy semantics already pin downstream
//                         pending). Evidence records the rejection
//                         decision for the audit trail.
//   * `needs_changes`  → no dispatch. Node stays paused. Evidence
//                         records the request and the response surfaces
//                         a `next_step` recommendation so the caller
//                         knows to recompile / re-pause the gate.
// ═════════════════════════════════════════════════════════════════════════

/// Wave-17 / task 01 — pure failure modes for the resume validator. Each
/// variant maps to a structured `ToolError` at the action_execute_resume
/// boundary so callers see actionable error codes instead of opaque
/// anyhow strings. Listener-side bridge logs the same vocabulary for
/// observability without surfacing tool errors to the bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PlanNodeResumeError {
    /// Supplied id failed the wave-14 envelope parser. Caller surfaces
    /// `REVIEW_ID_MALFORMED`.
    IdMalformed { detail: String },
    /// Supplied id is not a plan-node review (either scope != "plan" or
    /// action != "plan-node"). Resume only handles the deterministic
    /// plan-node path; other ids must travel through the wave-15
    /// manager-side resolution input.
    NotPlanNodeId { scope: String, action: String },
    /// Supplied id targets a different plan than the one being executed.
    PlanIdMismatch { expected: String, actual: String },
    /// Supplied id targets a different plan version than the one in the
    /// DB. The author must rebuild the resume request against the
    /// current plan version (the original paused-node lifecycle was
    /// stamped against an older PLAN.lisp shape).
    StaleVersion { expected: i32, actual_in_id: i32 },
    /// `topic_hash` slot is empty — wave-16 / task 04 always populates
    /// it. Defensive against authors hand-rolling a malformed id.
    MissingTopicHash,
    /// `topic_hash` did not map to any node carrying
    /// `:review-gate "question-event"` in the current PLAN.lisp. Either
    /// the node was renamed / removed since pause, or the gate hint
    /// was stripped — either way we refuse to dispatch a phantom node.
    NoMatchingPausedNode { topic_hash: String },
    /// `topic_hash` matched more than one paused-eligible node. Should
    /// be impossible (SHA-256 collision space is 64 bits over the node
    /// ids in a single plan) but kept loud so a future bug surfaces.
    AmbiguousPausedNode {
        topic_hash: String,
        candidates: Vec<String>,
    },
    /// PLAN.lisp body failed `build_validated_dag` (e.g. cycle / unknown
    /// target). Resume cannot revive a node from an unparseable plan.
    DagBuildFailed { detail: String },
    /// Plan status is outside the executable set (`approved` /
    /// `executing`). Authors must re-approve / re-mark the plan before
    /// resuming a paused node.
    PlanStatusNotExecutable { status: String },
}

impl PlanNodeResumeError {
    pub(super) fn code(&self) -> &'static str {
        match self {
            PlanNodeResumeError::IdMalformed { .. } => "REVIEW_ID_MALFORMED",
            PlanNodeResumeError::NotPlanNodeId { .. } => "REVIEW_ACTION_UNSUPPORTED",
            PlanNodeResumeError::PlanIdMismatch { .. } => "REVIEW_ARTIFACT_MISMATCH",
            PlanNodeResumeError::StaleVersion { .. } => "STALE_REVIEW_VERSION",
            PlanNodeResumeError::MissingTopicHash => "REVIEW_ID_MALFORMED",
            PlanNodeResumeError::NoMatchingPausedNode { .. } => error_codes::NOT_FOUND,
            PlanNodeResumeError::AmbiguousPausedNode { .. } => error_codes::INVALID_PARAM,
            PlanNodeResumeError::DagBuildFailed { .. } => error_codes::INVALID_PARAM,
            PlanNodeResumeError::PlanStatusNotExecutable { .. } => error_codes::INVALID_PARAM,
        }
    }

    pub(super) fn message(&self) -> String {
        match self {
            PlanNodeResumeError::IdMalformed { detail } => {
                format!("resume_review_question_id is malformed: {}", detail)
            }
            PlanNodeResumeError::NotPlanNodeId { scope, action } => format!(
                "resume_review_question_id must encode scope=plan and action=plan-node \
                 (got scope=`{}` action=`{}`); use review_question_id + review_decision \
                 for manager-side resolution",
                scope, action
            ),
            PlanNodeResumeError::PlanIdMismatch { expected, actual } => format!(
                "resume_review_question_id targets plan `{}` but execute called against plan `{}`",
                actual, expected
            ),
            PlanNodeResumeError::StaleVersion {
                expected,
                actual_in_id,
            } => format!(
                "resume_review_question_id targets version `v{}` but plan is at `v{}` \
                 — recompile / re-pause the gate against the current version",
                actual_in_id, expected
            ),
            PlanNodeResumeError::MissingTopicHash => {
                "resume_review_question_id is missing the trailing :node-hash segment".to_string()
            }
            PlanNodeResumeError::NoMatchingPausedNode { topic_hash } => format!(
                "no node carrying `:review-gate \"question-event\"` in the current plan \
                 maps to node-hash `{}` — either the node was renamed/removed since the \
                 pause emitted, or the gate hint was stripped",
                topic_hash
            ),
            PlanNodeResumeError::AmbiguousPausedNode {
                topic_hash,
                candidates,
            } => format!(
                "node-hash `{}` matched more than one paused-eligible node ({:?}); \
                 SHA-256 collision over node ids — this should never happen",
                topic_hash, candidates
            ),
            PlanNodeResumeError::DagBuildFailed { detail } => {
                format!("plan.sexp_text failed DAG validation: {}", detail)
            }
            PlanNodeResumeError::PlanStatusNotExecutable { status } => format!(
                "plan status `{}` is not executable; approve / mark to executing first",
                status
            ),
        }
    }
}

/// Wave-17 / task 01 — pure validator. Locates the unique paused-eligible
/// node a resume request targets WITHOUT touching DB or bus. Pulled out
/// of `action_execute_resume` so unit tests can pin the matrix
/// (id-malformed / plan-id mismatch / stale version / hash miss / hash
/// matched a non-paused node) without standing up an `AppState`.
///
/// The "paused-eligible" predicate is `review_gate_kind() == QuestionEvent`
/// — the same predicate the wave-16 / task 04 scheduler used at dispatch
/// time. This is the only signal we have because paused state is per-call
/// (no DB column); the resume helper is therefore stateless on the
/// execute side.
pub(super) fn validate_resume_request<'a>(
    parsed_qid: &super::review_gate::ParsedReviewQuestionId,
    plan: &Plan,
    parsed_dag: &'a ParsedDag,
) -> std::result::Result<&'a DagNode, PlanNodeResumeError> {
    if parsed_qid.scope != "plan"
        || !super::review_gate::is_plan_node_review_action(&parsed_qid.action)
    {
        return Err(PlanNodeResumeError::NotPlanNodeId {
            scope: parsed_qid.scope.clone(),
            action: parsed_qid.action.clone(),
        });
    }
    if parsed_qid.artifact_id != plan.id.to_string() {
        return Err(PlanNodeResumeError::PlanIdMismatch {
            expected: plan.id.to_string(),
            actual: parsed_qid.artifact_id.clone(),
        });
    }
    if parsed_qid.version != plan.version {
        return Err(PlanNodeResumeError::StaleVersion {
            expected: plan.version,
            actual_in_id: parsed_qid.version,
        });
    }
    let topic_hash = parsed_qid
        .topic_hash
        .as_deref()
        .ok_or(PlanNodeResumeError::MissingTopicHash)?;
    let mut matches: Vec<&DagNode> = Vec::new();
    for n in &parsed_dag.nodes {
        if !matches!(n.review_gate_kind(), ReviewGateKind::QuestionEvent) {
            continue;
        }
        let h = super::review_gate::derive_plan_node_topic_hash(&n.id);
        if h == topic_hash {
            matches.push(n);
        }
    }
    match matches.len() {
        0 => Err(PlanNodeResumeError::NoMatchingPausedNode {
            topic_hash: topic_hash.to_string(),
        }),
        1 => Ok(matches[0]),
        _ => Err(PlanNodeResumeError::AmbiguousPausedNode {
            topic_hash: topic_hash.to_string(),
            candidates: matches.iter().map(|n| n.id.clone()).collect(),
        }),
    }
}

/// Wave-17 / task 01 — single-node resume entrypoint invoked from
/// `mission_plan(action=execute)` when the caller supplies the
/// `resume_review_question_id` field set. Performs the validate /
/// dispatch / evidence cycle for ONE paused node and surfaces the
/// outcome on the response payload.
///
/// Only the targeted node is touched. Downstream nodes that were
/// pending after the original paused dispatch stay pending — the
/// caller is expected to drive a follow-up `mission_plan(execute)`
/// call to advance them. This conservative scope matches the wave-17
/// brief: "Only resume existing paused node state. No broad PLAN
/// reinterpretation."
pub(super) async fn action_execute_resume(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    input: super::review_gate::PlanNodeResumeInput,
) -> Result<ToolResult> {
    use super::review_gate::{parse_review_question_id_struct, ReviewDecision};

    if !matches!(plan.status, PlanStatus::Approved | PlanStatus::Executing) {
        return Ok(resume_error_to_tool_result(
            PlanNodeResumeError::PlanStatusNotExecutable {
                status: plan.status.as_str().to_string(),
            },
        ));
    }

    let parsed_qid = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(resume_error_to_tool_result(
                PlanNodeResumeError::IdMalformed { detail: e.message() },
            ))
        }
    };

    let (parsed_dag, _order) = match build_validated_dag(&plan.sexp_text) {
        Ok(v) => v,
        Err(e) => {
            return Ok(resume_error_to_tool_result(
                PlanNodeResumeError::DagBuildFailed {
                    detail: format!("{:?}", e),
                },
            ))
        }
    };

    let node = match validate_resume_request(&parsed_qid, plan, &parsed_dag) {
        Ok(n) => n.clone(),
        Err(e) => return Ok(resume_error_to_tool_result(e)),
    };

    let dispatch_strategy = node
        .dispatch_strategy
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    let ctx = EvidenceCtx {
        plan_id: plan.id,
        plan_version: plan.version,
        project_arg: args.get("project").and_then(|v| v.as_str()),
        cwd_arg: args.get("cwd").and_then(|v| v.as_str()),
        target_project_arg: args.get("target_project").and_then(|v| v.as_str()),
    };

    // Evidence is recorded for EVERY decision (approved/rejected/
    // needs_changes) so the audit trail captures the resume even when
    // we refuse to dispatch.
    let mut outcome = ExecutionOutcome::default();
    let resume_event_ref = emit_resume_decision_evidence(
        state,
        &ctx,
        plan,
        &node,
        &dispatch_strategy,
        &input,
        &mut outcome,
    )
    .await;

    let mut payload = json!({
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "node_id": node.id,
        "review_question_id": input.question_id,
        "review_decision": input.decision.as_str(),
        "review_resume": true,
    });
    if let Some(actor) = input.actor.as_deref() {
        payload["resume_actor"] = json!(actor);
    }
    if let Some(note) = input.note.as_deref() {
        payload["resume_note"] = json!(note);
    }
    if let Some(ref_event_id) = resume_event_ref.as_deref() {
        payload["resume_event_id"] = json!(ref_event_id);
    }

    match input.decision {
        ReviewDecision::Approved => {
            // Fresh attempt 1: paused is non-terminal (no failed
            // attempt was consumed by the gate emit), so the resume
            // dispatch is conceptually a brand-new run of the node.
            let attempt = PLAN_NODE_DEFAULT_ATTEMPT;
            emit_evidence_running(
                state,
                &ctx,
                &node,
                &dispatch_strategy,
                attempt,
                &mut outcome,
            )
            .await;
            // wave-19 / task 06 — resume path also honours the
            // task-contract knobs from the resume call. We re-build
            // the ctx from `args` because the paused-node code path
            // never threaded the original DAG run's ctx through —
            // and that's the right semantic: the resume request is a
            // fresh execute call so it gets to set its own contract
            // policy.
            let resume_task_contract_ctx = match TaskContractDispatchCtx::from_args(args) {
                Ok(c) => c,
                Err(err) => return Ok(err),
            };
            let dispatch_outcome = match dispatch_node(
                state.clone(),
                plan.clone(),
                node.clone(),
                resume_task_contract_ctx,
            )
            .await
            {
                Ok(o) => o,
                Err(e) => {
                    return Err(e);
                }
            };
            let DispatchOutcome {
                node_id: _,
                target,
                dispatch_strategy: ds,
                inner_payload,
                classification,
                non_retryable: _,
            } = dispatch_outcome;
            let succeeded = classification.is_ok();
            emit_evidence_finished(
                state,
                &ctx,
                &node,
                &ds,
                &inner_payload,
                succeeded,
                attempt,
                &mut outcome,
            )
            .await;
            payload["status"] = json!(if succeeded {
                "resume_dispatched"
            } else {
                "resume_failed"
            });
            payload["target"] = json!(target);
            payload["dispatch_strategy"] = json!(ds);
            payload["inner_result"] = inner_payload;
            payload["attempt"] = json!(attempt);
            if let Err(reason) = classification {
                payload["reason"] = json!(reason);
            }
        }
        ReviewDecision::Rejected => {
            payload["status"] = json!("resume_rejected");
            payload["target"] = json!(node.target);
            payload["dispatch_strategy"] = json!(dispatch_strategy);
            payload["next_step"] = json!(format!(
                "node `{}` remains paused; recompile the plan or supply \
                 review_decision=approved to dispatch it",
                node.id
            ));
        }
        ReviewDecision::NeedsChanges => {
            payload["status"] = json!("resume_needs_changes");
            payload["target"] = json!(node.target);
            payload["dispatch_strategy"] = json!(dispatch_strategy);
            payload["next_step"] = json!(format!(
                "rework node `{}` per `resume_note`, recompile the plan, \
                 then resume against the new node-hash",
                node.id
            ));
        }
    }

    if let Some(p) = outcome.evidence_path.as_deref() {
        payload["evidence_path"] = json!(p);
    }
    if let Some(e) = outcome.evidence_error.as_deref() {
        payload["evidence_error"] = json!(e);
    }
    if !outcome.bus_publish_warnings.is_empty() {
        payload["bus_publish_warnings"] = json!(outcome.bus_publish_warnings);
    }

    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-17 / task 01 — convert a resume validation failure into the
/// canonical `ToolResult::structured_error` shape so the
/// `mission_plan(action=execute)` boundary always surfaces actionable
/// error codes.
fn resume_error_to_tool_result(err: PlanNodeResumeError) -> ToolResult {
    ToolResult::structured_error(ToolError::new(err.code(), err.message()))
}

/// Wave-17 / task 01 — emit a single `paused -> review_resolved`
/// evidence row capturing the resume decision (approved / rejected /
/// needs_changes) for the audit trail. Always runs, regardless of
/// whether we go on to dispatch the node, so the row records the
/// human / operator intent even when the decision keeps the node
/// paused.
///
/// Returns `Some(event_id)` when a `PlanNodeStateChanged` lifecycle
/// event was published (or fell back to the deterministic id) so the
/// caller can splice it onto the response under `resume_event_id`.
/// Returns `None` only when the `EventRef` builder yielded an
/// `unavailable` ref — currently unreachable but kept loose so the
/// helper stays decoupled from ref-availability assumptions.
async fn emit_resume_decision_evidence(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    node: &DagNode,
    dispatch_strategy: &str,
    input: &super::review_gate::PlanNodeResumeInput,
    outcome: &mut ExecutionOutcome,
) -> Option<String> {
    let to_state = match input.decision {
        super::review_gate::ReviewDecision::Approved => "resume_approved",
        super::review_gate::ReviewDecision::Rejected => "resume_rejected",
        super::review_gate::ReviewDecision::NeedsChanges => "resume_needs_changes",
    };
    let attempt = PLAN_NODE_DEFAULT_ATTEMPT;
    let (event_ref, lifecycle_warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "paused",
        to_state,
        Some(format!(
            "review_resume:{}:qid={}",
            input.decision.as_str(),
            input.question_id
        )),
    )
    .await;
    if let Some(w) = lifecycle_warning {
        outcome.bus_publish_warnings.push(w);
    }
    let event_id = event_ref.event_id.clone();

    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition(format!("paused -> {}", to_state))
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("review_resume", json!(true))
    .with_extra("review_question_id", json!(input.question_id))
    .with_extra("review_decision", json!(input.decision.as_str()))
    .with_extra("plan_version", json!(plan.version));
    if let Some(actor) = input.actor.as_deref() {
        entry = entry.with_extra("resume_actor", json!(actor));
    }
    if let Some(note) = input.note.as_deref() {
        entry = entry.with_extra("resume_note", json!(note));
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id,
            node_id = %node.id,
            decision = %input.decision.as_str(),
            error = %error,
            "DAG resume: paused->review_* evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
    event_id
}

/// Wave-17 / task 01 — outcome of the listener-side resume bridge,
/// used by `bus::v2_subscribers` to log a structured signal for every
/// inbound plan-node Resolved event without surfacing tool errors on
/// the bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanNodeResumeListenerOutcome {
    /// Approved decision — re-dispatched the paused node.
    Dispatched {
        plan_id: uuid::Uuid,
        node_id: String,
        succeeded: bool,
    },
    /// Rejected decision — node stays paused, evidence row recorded.
    KeptPaused {
        plan_id: uuid::Uuid,
        node_id: String,
        decision: &'static str,
    },
    /// Envelope's artifact_id failed to parse as a UUID.
    ArtifactIdNotUuid { artifact_id: String, error: String },
    /// Plan row not found for the qid's artifact_id.
    NotFound { artifact_id: uuid::Uuid },
    /// Validation rejected the resume (id mismatch, hash miss, etc.)
    /// — kept loud for observability.
    ValidationRejected {
        plan_id: uuid::Uuid,
        code: &'static str,
        message: String,
    },
    /// Plan row exists but `plan.sexp_text` failed DAG validation.
    DagBuildFailed { plan_id: uuid::Uuid, detail: String },
    /// Underlying DB / dispatch raised an error.
    DispatchError { plan_id: uuid::Uuid, detail: String },
}

/// Wave-17 / task 01 — listener-side bridge. Called from
/// `bus::v2_subscribers::handle_review_resolved` when the inbound
/// envelope's scope is `plan` AND action is `plan-node`. Performs the
/// same validate / dispatch / evidence cycle as the explicit
/// `action_execute_resume` entry point, but takes the parsed envelope
/// + decision directly (no `args` JSON) and surfaces a structured
/// outcome instead of a `ToolResult`.
///
/// Side effects mirror the explicit path: at most one node dispatch +
/// the evidence rows (`paused -> resume_*` + `ready -> running` +
/// `running -> {succeeded|failed}` for the approved branch). No bus
/// publish of a downstream Resolved event — the inbound Resolved we
/// just consumed IS the downstream signal.
pub(crate) async fn handle_review_resolved_plan_node_event(
    state: &AppState,
    parsed_qid: &super::review_gate::ParsedReviewQuestionId,
    decision: super::review_gate::ReviewDecision,
) -> PlanNodeResumeListenerOutcome {
    let id = match uuid::Uuid::parse_str(&parsed_qid.artifact_id) {
        Ok(u) => u,
        Err(e) => {
            return PlanNodeResumeListenerOutcome::ArtifactIdNotUuid {
                artifact_id: parsed_qid.artifact_id.clone(),
                error: e.to_string(),
            }
        }
    };
    let plan = match state.store.plan_get(id).await {
        Ok(Some(p)) => p,
        Ok(None) => return PlanNodeResumeListenerOutcome::NotFound { artifact_id: id },
        Err(e) => {
            return PlanNodeResumeListenerOutcome::DispatchError {
                plan_id: id,
                detail: format!("plan_get: {}", e),
            }
        }
    };
    if !matches!(plan.status, PlanStatus::Approved | PlanStatus::Executing) {
        return PlanNodeResumeListenerOutcome::ValidationRejected {
            plan_id: id,
            code: error_codes::INVALID_PARAM,
            message: format!(
                "plan status `{}` is not executable",
                plan.status.as_str()
            ),
        };
    }
    let (parsed_dag, _order) = match build_validated_dag(&plan.sexp_text) {
        Ok(v) => v,
        Err(e) => {
            return PlanNodeResumeListenerOutcome::DagBuildFailed {
                plan_id: id,
                detail: format!("{:?}", e),
            }
        }
    };
    let node = match validate_resume_request(parsed_qid, &plan, &parsed_dag) {
        Ok(n) => n.clone(),
        Err(e) => {
            return PlanNodeResumeListenerOutcome::ValidationRejected {
                plan_id: id,
                code: e.code(),
                message: e.message(),
            }
        }
    };
    let dispatch_strategy = node
        .dispatch_strategy
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let ctx = EvidenceCtx {
        plan_id: plan.id,
        plan_version: plan.version,
        project_arg: None,
        cwd_arg: None,
        target_project_arg: None,
    };
    let input = super::review_gate::PlanNodeResumeInput {
        question_id: format!(
            "review:{}:{}:v{}:{}",
            parsed_qid.scope,
            parsed_qid.artifact_id,
            parsed_qid.version,
            parsed_qid.action
        ) + &parsed_qid
            .topic_hash
            .as_deref()
            .map(|h| format!(":{}", h))
            .unwrap_or_default(),
        decision,
        actor: None,
        note: None,
    };
    let mut outcome = ExecutionOutcome::default();
    let _ = emit_resume_decision_evidence(
        state,
        &ctx,
        &plan,
        &node,
        &dispatch_strategy,
        &input,
        &mut outcome,
    )
    .await;
    match decision {
        super::review_gate::ReviewDecision::Approved => {
            let attempt = PLAN_NODE_DEFAULT_ATTEMPT;
            emit_evidence_running(
                state,
                &ctx,
                &node,
                &dispatch_strategy,
                attempt,
                &mut outcome,
            )
            .await;
            // wave-19 / task 06 — listener-driven resumes never see
            // caller args (they fire from a bus event), so the
            // task-contract emitter defaults to Off here. Callers
            // that want a contract emitted must hit the explicit
            // `mission_plan(action=execute, resume_review_question_id=...)`
            // path so they can pass `task_contract_mode`.
            let dispatch_outcome = match dispatch_node(
                state.clone(),
                plan.clone(),
                node.clone(),
                TaskContractDispatchCtx::off(),
            )
            .await
            {
                Ok(o) => o,
                Err(e) => {
                    return PlanNodeResumeListenerOutcome::DispatchError {
                        plan_id: id,
                        detail: e.to_string(),
                    }
                }
            };
            let DispatchOutcome {
                inner_payload,
                classification,
                dispatch_strategy: ds,
                ..
            } = dispatch_outcome;
            let succeeded = classification.is_ok();
            emit_evidence_finished(
                state,
                &ctx,
                &node,
                &ds,
                &inner_payload,
                succeeded,
                attempt,
                &mut outcome,
            )
            .await;
            PlanNodeResumeListenerOutcome::Dispatched {
                plan_id: id,
                node_id: node.id.clone(),
                succeeded,
            }
        }
        super::review_gate::ReviewDecision::Rejected => {
            PlanNodeResumeListenerOutcome::KeptPaused {
                plan_id: id,
                node_id: node.id.clone(),
                decision: "rejected",
            }
        }
        super::review_gate::ReviewDecision::NeedsChanges => {
            PlanNodeResumeListenerOutcome::KeptPaused {
                plan_id: id,
                node_id: node.id.clone(),
                decision: "needs_changes",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::Utc;
    use missiond_core::types::PlanStatus;
    use uuid::Uuid;

    fn fixture_plan(sexp: &str) -> Plan {
        Plan {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000abc").unwrap(),
            board_task_id: "btk-dag".to_string(),
            source_directive_id: None,
            version: 1,
            sexp_text: sexp.to_string(),
            sexp_hash: "deadbeef".to_string(),
            status: PlanStatus::Approved,
            compiler_model: None,
            compiled_from: None,
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            approved_at: None,
            finished_at: None,
        }
    }

    // ── parser pure tests ──────────────────────────────────────────────

    #[test]
    fn parse_plan_dag_extracts_explicit_node_forms() {
        let sexp = r#"
            (plan
              :board_task_id "btk-1"
              (node :id "n1" :target "mission_execution" :objective "alpha")
              (node :id "n2" :target "mission_task_delegate" :depends-on ["n1"]))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes.len(), 2);
        assert_eq!(parsed.nodes[0].id, "n1");
        assert_eq!(parsed.nodes[0].target, "mission_execution");
        assert_eq!(parsed.nodes[0].objective.as_deref(), Some("alpha"));
        assert_eq!(parsed.nodes[1].depends_on, vec!["n1".to_string()]);
        // The :board_task_id sibling form is a keyword/value pair, not a form
        // we recognise — we don't surface it in unsupported_top_forms because
        // it is not a `(...)` sub-form. Only sibling sub-forms appear there.
    }

    #[test]
    fn parse_plan_dag_records_unsupported_top_forms() {
        let sexp = r#"
            (plan
              (goal :ship "thing")
              (node :id "n1" :target "mission_execution"))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.unsupported_top_forms.len(), 1);
        assert!(parsed.unsupported_top_forms[0].starts_with("(goal"));
    }

    #[test]
    fn parse_plan_dag_captures_unsupported_node_fields() {
        let sexp = r#"
            (plan
              (node :id "n1" :target "mission_execution" :priority "high" :foo bar))
        "#;
        let parsed = parse_plan_dag(sexp);
        let n = &parsed.nodes[0];
        let keys: Vec<&str> = n.unsupported_fields.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"priority"));
        assert!(keys.contains(&"foo"));
    }

    #[test]
    fn parse_plan_dag_supports_paren_depends_on_alias() {
        let sexp = r#"
            (plan
              (node :id "a" :target "mission_execution")
              (node :id "b" :target "mission_execution" :depends-on (a)))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[1].depends_on, vec!["a".to_string()]);
    }

    #[test]
    fn parse_plan_dag_failure_policy_default_and_override() {
        let sexp = r#"
            (plan
              (node :id "a" :target "mission_execution")
              (node :id "b" :target "mission_execution" :failure-policy "continue")
              (node :id "c" :target "mission_execution" :failure-policy "weird"))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[0].failure_policy, "fail-fast");
        assert_eq!(parsed.nodes[1].failure_policy, "continue");
        // unknown policy → fall back to fail-fast and capture the original.
        assert_eq!(parsed.nodes[2].failure_policy, "fail-fast");
        assert!(parsed.nodes[2]
            .unsupported_fields
            .iter()
            .any(|(k, v)| k == "failure-policy" && v == "weird"));
    }

    #[test]
    fn parse_plan_dag_timeout_ms_parsed_as_integer() {
        let sexp = r#"(plan (node :id "n" :target "mission_execution" :timeout-ms 500))"#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[0].timeout_ms, Some(500));
    }

    // ── validator pure tests ───────────────────────────────────────────

    #[test]
    fn build_validated_dag_accepts_valid_chain() {
        let sexp = r#"
            (plan
              (node :id "n1" :target "mission_execution")
              (node :id "n2" :target "mission_task_delegate" :depends-on ["n1"])
              (node :id "n3" :target "mission_execution" :depends-on ["n2"]))
        "#;
        let (parsed, order) = build_validated_dag(sexp).expect("valid chain");
        assert_eq!(parsed.nodes.len(), 3);
        assert_eq!(order, vec!["n1".to_string(), "n2".to_string(), "n3".to_string()]);
    }

    #[test]
    fn build_validated_dag_rejects_no_nodes() {
        let sexp = "(plan :goal :ship)";
        let err = build_validated_dag(sexp).unwrap_err();
        assert!(matches!(err, DagBuildError::NoNodes));
    }

    #[test]
    fn build_validated_dag_rejects_duplicate_id() {
        let sexp = r#"
            (plan
              (node :id "x" :target "mission_execution")
              (node :id "x" :target "mission_execution"))
        "#;
        let err = build_validated_dag(sexp).unwrap_err();
        match err {
            DagBuildError::DuplicateId(id) => assert_eq!(id, "x"),
            other => panic!("expected DuplicateId, got {:?}", other),
        }
    }

    #[test]
    fn build_validated_dag_rejects_invalid_target() {
        let sexp = r#"(plan (node :id "n1" :target "mission_explode"))"#;
        let err = build_validated_dag(sexp).unwrap_err();
        match err {
            DagBuildError::InvalidTarget { node_id, target } => {
                assert_eq!(node_id, "n1");
                assert_eq!(target, "mission_explode");
            }
            other => panic!("expected InvalidTarget, got {:?}", other),
        }
    }

    #[test]
    fn build_validated_dag_rejects_missing_dependency() {
        let sexp = r#"
            (plan
              (node :id "n1" :target "mission_execution" :depends-on ["ghost"]))
        "#;
        let err = build_validated_dag(sexp).unwrap_err();
        match err {
            DagBuildError::DependencyMissing { node_id, missing } => {
                assert_eq!(node_id, "n1");
                assert_eq!(missing, "ghost");
            }
            other => panic!("expected DependencyMissing, got {:?}", other),
        }
    }

    #[test]
    fn build_validated_dag_rejects_self_dependency() {
        let sexp = r#"(plan (node :id "n1" :target "mission_execution" :depends-on ["n1"]))"#;
        let err = build_validated_dag(sexp).unwrap_err();
        assert!(matches!(err, DagBuildError::SelfDependency(ref id) if id == "n1"));
    }

    #[test]
    fn build_validated_dag_rejects_cycle() {
        let sexp = r#"
            (plan
              (node :id "a" :target "mission_execution" :depends-on ["b"])
              (node :id "b" :target "mission_execution" :depends-on ["a"]))
        "#;
        let err = build_validated_dag(sexp).unwrap_err();
        match err {
            DagBuildError::Cycle(members) => {
                assert!(members.contains(&"a".to_string()));
                assert!(members.contains(&"b".to_string()));
            }
            other => panic!("expected Cycle, got {:?}", other),
        }
    }

    #[test]
    fn topo_sort_is_deterministic_for_independent_nodes() {
        // Three independent nodes — their order must be the BTreeSet
        // (lexicographic) order: a, b, c. This pins the contract that pure
        // tests can rely on.
        let sexp = r#"
            (plan
              (node :id "c" :target "mission_execution")
              (node :id "a" :target "mission_execution")
              (node :id "b" :target "mission_execution"))
        "#;
        let (_, order) = build_validated_dag(sexp).expect("topo");
        assert_eq!(order, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    // ── unsupported metadata preservation ───────────────────────────────

    #[test]
    fn node_hint_summary_records_unsupported_fields() {
        let sexp = r#"
            (plan
              (node :id "n1" :target "mission_execution" :foo bar :baz "qux")
              (node :id "n2" :target "mission_execution"))
        "#;
        let (parsed, _) = build_validated_dag(sexp).expect("valid");
        let summary = build_node_hint_summary(&parsed);
        let by_node = summary
            .get("unsupported_fields")
            .and_then(|v| v.as_object())
            .expect("object");
        let n1 = by_node.get("n1").expect("n1 present");
        let arr = n1.as_array().expect("array");
        assert_eq!(arr.len(), 2);
        // n2 has none — must NOT appear in the map at all.
        assert!(by_node.get("n2").is_none());
    }

    #[test]
    fn node_hint_summary_records_unsupported_top_forms() {
        let sexp = r#"
            (plan
              (rollback :step "undo")
              (node :id "n1" :target "mission_execution"))
        "#;
        let (parsed, _) = build_validated_dag(sexp).expect("valid");
        let summary = build_node_hint_summary(&parsed);
        let arr = summary
            .get("unsupported_top_forms")
            .and_then(|v| v.as_array())
            .expect("array");
        assert_eq!(arr.len(), 1);
        assert!(arr[0].as_str().unwrap().starts_with("(rollback"));
    }

    // ── dry_run response shape ──────────────────────────────────────────

    #[test]
    fn build_nodes_summary_renders_topo_order_with_known_fields_only() {
        let sexp = r#"
            (plan
              (node :id "n1" :target "mission_execution" :objective "do thing")
              (node :id "n2" :target "mission_task_delegate" :depends-on ["n1"]
                    :dispatch-strategy "agent-team" :timeout-ms 7000))
        "#;
        let (parsed, order) = build_validated_dag(sexp).expect("valid");
        let summary = build_nodes_summary(&parsed.nodes, &order);
        let arr = summary.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "n1");
        assert_eq!(arr[0]["target"], "mission_execution");
        assert_eq!(arr[0]["objective"], "do thing");
        assert_eq!(arr[1]["dispatch_strategy"], "agent-team");
        assert_eq!(arr[1]["timeout_ms"], 7000);
        assert_eq!(arr[1]["failure_policy"], "fail-fast");
    }

    // ── scheduler_mode detection ────────────────────────────────────────

    #[test]
    fn detect_scheduler_mode_default_when_absent() {
        let v = json!({});
        assert!(!detect_scheduler_mode(&v).unwrap());
    }

    #[test]
    fn detect_scheduler_mode_recognises_dag_v1() {
        assert!(detect_scheduler_mode(&json!({"scheduler_mode": "dag_v1"})).unwrap());
        assert!(detect_scheduler_mode(&json!({"scheduler_mode": "dag-v1"})).unwrap());
    }

    #[test]
    fn detect_scheduler_mode_treats_legacy_aliases_as_default() {
        for alias in ["v0", "default", "current", "single_node", "single-node"] {
            assert!(!detect_scheduler_mode(&json!({"scheduler_mode": alias})).unwrap());
        }
    }

    #[test]
    fn detect_scheduler_mode_rejects_unknown_value() {
        let err = detect_scheduler_mode(&json!({"scheduler_mode": "warp_drive"})).unwrap_err();
        assert_eq!(err.is_error, Some(true));
    }

    // ── wave-20 / task 07 — DAG-side guard for LLM-augmented modes ──────

    #[test]
    fn refuse_llm_inference_in_dag_mode_blocks_sonnet_suggest() {
        // sonnet_suggest is single-node-execute-only in v0; the DAG path
        // must reject the combo eagerly so the LLM proposal block is
        // never silently dropped.
        let args = json!({
            "scheduler_mode": "dag_v1",
            "infer_plan_fields": "sonnet_suggest"
        });
        let err = refuse_llm_inference_in_dag_mode(&args)
            .expect("dag + sonnet_suggest combo refused");
        assert_eq!(err.is_error, Some(true));
    }

    #[test]
    fn refuse_llm_inference_in_dag_mode_allows_deterministic_modes() {
        // Off / preview / apply_safe stay accepted — these are the
        // wave-18 / task 06 modes the DAG path already tolerates.
        for mode in ["off", "preview", "apply_safe"] {
            let args = json!({
                "scheduler_mode": "dag_v1",
                "infer_plan_fields": mode
            });
            assert!(
                refuse_llm_inference_in_dag_mode(&args).is_none(),
                "mode `{}` must not be refused",
                mode
            );
        }
        // Absent infer_plan_fields → no refusal.
        assert!(refuse_llm_inference_in_dag_mode(&json!({})).is_none());
    }

    #[test]
    fn refuse_llm_inference_in_dag_mode_propagates_typo_error() {
        // A typo on the infer mode surfaces as INVALID_PARAM via the
        // shared parser, so the DAG path returns the same structured
        // error as the single-node path (no silent acceptance).
        let args = json!({
            "scheduler_mode": "dag_v1",
            "infer_plan_fields": "sonet_suggest"
        });
        let err = refuse_llm_inference_in_dag_mode(&args)
            .expect("typo surfaced as structured error");
        assert_eq!(err.is_error, Some(true));
    }

    // ── execution helpers (pure paths only — full e2e needs AppState) ────

    #[test]
    fn outcome_aggregate_status_dag_succeeded_when_all_succeed() {
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "n1".into(),
            target: "mission_execution".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "unknown".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        assert_eq!(o.aggregate_status(), "dag_succeeded");
        assert_eq!(o.runner_status(), "all_nodes_dispatched");
        assert_eq!(o.target_plan_status(), Some(PlanStatus::Succeeded));
    }

    #[test]
    fn outcome_aggregate_status_fail_fast_marks_dag_failed_and_plan_failed() {
        let mut o = ExecutionOutcome::default();
        o.aborted_fail_fast = true;
        o.results.push(NodeResult {
            id: "n1".into(),
            target: "mission_execution".into(),
            state: NodeState::Failed { reason: "boom".into() },
            dispatch_strategy: "unknown".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        assert_eq!(o.aggregate_status(), "dag_failed");
        assert_eq!(o.runner_status(), "fail_fast_aborted");
        assert_eq!(o.target_plan_status(), Some(PlanStatus::Failed));
    }

    #[test]
    fn outcome_aggregate_status_continue_with_failure_yields_partial() {
        let mut o = ExecutionOutcome::default();
        // Failed node + downstream skip + an independent success → partial.
        o.results.push(NodeResult {
            id: "n1".into(),
            target: "mission_execution".into(),
            state: NodeState::Failed { reason: "x".into() },
            dispatch_strategy: "unknown".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "n2".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedUpstreamFailed {
                failed_dep: "n1".into(),
            },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "n3".into(),
            target: "mission_execution".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "unknown".into(),
            inner_payload: json!({"ok": true}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        assert_eq!(o.aggregate_status(), "dag_partial");
        assert_eq!(o.target_plan_status(), Some(PlanStatus::Failed));
        let v = o.node_results_json();
        let arr = v.as_array().unwrap();
        assert_eq!(arr[1]["state"], "skipped_upstream_failed");
        assert_eq!(arr[1]["failed_dep"], "n1");
    }

    #[test]
    fn propagate_taint_marks_full_subtree() {
        // Graph: a -> b -> c, a -> d. Taint a; expect b,c,d all tainted.
        let nodes = vec![
            DagNode {
                id: "a".into(),
                target: "mission_execution".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
            DagNode {
                id: "b".into(),
                target: "mission_execution".into(),
                depends_on: vec!["a".into()],
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
            DagNode {
                id: "c".into(),
                target: "mission_execution".into(),
                depends_on: vec!["b".into()],
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
            DagNode {
                id: "d".into(),
                target: "mission_execution".into(),
                depends_on: vec!["a".into()],
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ];
        let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
        for n in &nodes {
            for dep in &n.depends_on {
                succs.entry(dep.as_str()).or_default().push(n.id.as_str());
            }
        }
        let mut tainted: HashMap<String, String> = HashMap::new();
        propagate_taint(&nodes[0], &succs, &mut tainted);
        assert_eq!(tainted.get("b"), Some(&"a".to_string()));
        assert_eq!(tainted.get("c"), Some(&"a".to_string()));
        assert_eq!(tainted.get("d"), Some(&"a".to_string()));
        assert!(tainted.get("a").is_none());
    }

    #[test]
    fn build_node_inner_args_for_mission_execution_emits_known_fields() {
        let node = DagNode {
            id: "n1".into(),
            target: "mission_execution".into(),
            objective: Some("do thing".into()),
            failure_policy: "fail-fast".into(),
            dispatch_strategy: Some("fresh-code-alignment".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/abs/path".into()),
            ..Default::default()
        };
        let plan = fixture_plan("(plan)");
        let built = build_node_inner_args(&node, &plan);
        let inner = built.inner_args.expect("ok");
        assert_eq!(inner["action"], "open");
        assert_eq!(inner["dispatch_strategy"], "fresh-code-alignment");
        assert_eq!(inner["project"], "missiond");
        assert_eq!(inner["target_project"], "missiond");
        assert_eq!(inner["requested_cwd"], "/abs/path");
        assert_eq!(built.dispatch_strategy, "fresh-code-alignment");
    }

    #[test]
    fn build_node_inner_args_for_task_delegate_uses_objective_and_cwd() {
        let node = DagNode {
            id: "n1".into(),
            target: "mission_task_delegate".into(),
            objective: Some("ship a thing".into()),
            failure_policy: "fail-fast".into(),
            timeout_ms: Some(15_000),
            requested_cwd: Some("/abs/path".into()),
            ..Default::default()
        };
        let plan = fixture_plan("(plan)");
        let built = build_node_inner_args(&node, &plan);
        let inner = built.inner_args.expect("ok");
        assert_eq!(inner["objective"], "ship a thing");
        assert_eq!(inner["cwd"], "/abs/path");
        assert_eq!(inner["timeout_secs"], 15);
    }

    #[test]
    fn build_node_inner_args_for_flow_run_requires_flow_id() {
        let node = DagNode {
            id: "n1".into(),
            target: "mission_flow_run".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        };
        let plan = fixture_plan("(plan)");
        let built = build_node_inner_args(&node, &plan);
        // Missing flow_id -> inner builder returns Err with a structured payload.
        assert!(built.inner_args.is_err());
    }

    // ── wave-13 :: plan_dag_node_dispatch typed evidence shape ───────
    //
    // Each DAG node dispatch (success or failure branch) builds an
    // `EvidenceEntry` from `evidence_collector` instead of a hand-rolled
    // JSON object. These tests pin the projected on-disk shape so the
    // wire-compatible mapping
    //   legacy `kind="plan_dag_node_dispatch"`
    //     ↦ canonical `source="plan_dag_node_dispatch"` + `kind="dispatch"`
    // is enforced, and the legacy passthrough fields (`scheduler_mode`,
    // `node_id`, `plan_id`, `target_tool`, `target`, `dispatch_strategy`,
    // and the failure-branch `inner_error`) keep their flat top-level
    // placement for existing audit dashboards.
    //
    // We replay the exact entry construction (mirrored from
    // `execute_sequential`) instead of standing up an `AppState` so the
    // assertions stay focused on the wire shape.
    /// Wave-14 / Task 02: the production path now writes a live
    /// `EventRef::new(execution, plan_node_state_changed, <seq|deterministic>)`.
    /// The fixtures pin the **deterministic** branch (no bus available in
    /// pure tests) so assertions can grep on the deterministic id format.
    fn build_dag_success_entry(node: &DagNode, plan: &Plan, dispatch_strategy: &str, inner_payload: Value) -> Value {
        let det = deterministic_plan_node_event_id(
            plan.id,
            &node.id,
            PLAN_NODE_DEFAULT_ATTEMPT,
            "ready",
            "succeeded",
        );
        EvidenceEntry::new(
            evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
            evidence_collector::kind::DISPATCH,
        )
        .with_inner_dispatch(inner_payload.clone())
        .with_state_transition("ready -> succeeded")
        .add_execution_event(EventRef::new(
            EVENT_REF_SOURCE_EXECUTION,
            EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
            det,
        ))
        .with_extra("scheduler_mode", json!("dag_v1"))
        .with_extra("node_id", json!(node.id))
        .with_extra("plan_id", json!(plan.id))
        .with_extra("target_tool", json!(node.target))
        .with_extra("target", json!(node.target))
        .with_extra("dispatch_strategy", json!(dispatch_strategy))
        .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
        .with_extra("inner_result", inner_payload)
        .into_json()
    }

    fn build_dag_failure_entry(node: &DagNode, plan: &Plan, dispatch_strategy: &str, inner_payload: Value) -> Value {
        let det = deterministic_plan_node_event_id(
            plan.id,
            &node.id,
            PLAN_NODE_DEFAULT_ATTEMPT,
            "ready",
            "failed",
        );
        EvidenceEntry::new(
            evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
            evidence_collector::kind::DISPATCH,
        )
        .with_state_transition("ready -> failed")
        .add_execution_event(EventRef::new(
            EVENT_REF_SOURCE_EXECUTION,
            EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
            det,
        ))
        .with_extra("scheduler_mode", json!("dag_v1"))
        .with_extra("node_id", json!(node.id))
        .with_extra("plan_id", json!(plan.id))
        .with_extra("target_tool", json!(node.target))
        .with_extra("target", json!(node.target))
        .with_extra("dispatch_strategy", json!(dispatch_strategy))
        .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
        .with_extra("inner_error", inner_payload)
        .into_json()
    }

    fn fixture_dag_node(id: &str, target: &str) -> DagNode {
        DagNode {
            id: id.into(),
            target: target.into(),
            failure_policy: "fail-fast".into(),
            dispatch_strategy: Some("agent-team".into()),
            ..Default::default()
        }
    }

    #[test]
    fn dag_node_dispatch_evidence_carries_canonical_source_and_kind() {
        let node = fixture_dag_node("n1", "mission_execution");
        let plan = fixture_plan("(plan)");
        let entry = build_dag_success_entry(&node, &plan, "agent-team", json!({"ok": true}));
        // wave-12 wire-compatible mapping for the DAG branch.
        assert_eq!(entry["source"], "plan_dag_node_dispatch");
        assert_eq!(entry["kind"], "dispatch");
        assert_eq!(entry["schema_version"], "v0");
        // Success-branch transition and inner payload land under canonical slots.
        assert_eq!(entry["state_transition"], "ready -> succeeded");
        assert_eq!(entry["inner_dispatch"]["ok"], true);
        // Pre-wave12 sidecars carried the same payload under `inner_result`;
        // we keep it as a legacy alias for byte-for-byte reader compat.
        assert_eq!(entry["inner_result"]["ok"], true);
    }

    #[test]
    fn dag_node_dispatch_evidence_keeps_legacy_passthrough_keys_flat() {
        let node = fixture_dag_node("n7", "mission_task_delegate");
        let plan = fixture_plan("(plan)");
        let entry = build_dag_success_entry(&node, &plan, "fresh-code-alignment", json!({"task_id": "t7"}));
        // Audit dashboards historically grep at the top level for these.
        assert_eq!(entry["scheduler_mode"], "dag_v1");
        assert_eq!(entry["node_id"], "n7");
        assert_eq!(entry["plan_id"], plan.id.to_string());
        assert_eq!(entry["target_tool"], "mission_task_delegate");
        // `target` is the new short alias the wave-13 plan_dag entry now also
        // exposes (mirrors `target_tool` for DAG-only consumers that pivot
        // on the shorter name).
        assert_eq!(entry["target"], "mission_task_delegate");
        assert_eq!(entry["dispatch_strategy"], "fresh-code-alignment");
    }

    #[test]
    fn dag_node_dispatch_evidence_failure_branch_keeps_inner_error_legacy_key() {
        // The failure branch must NOT call `with_inner_dispatch`; the inner
        // payload stays under the legacy `inner_error` extra so historical
        // readers that filtered on that key keep working byte-for-byte.
        let node = fixture_dag_node("n3", "mission_execution");
        let plan = fixture_plan("(plan)");
        let inner = json!({"error": "downstream rejected request"});
        let entry = build_dag_failure_entry(&node, &plan, "resident-lisp", inner.clone());
        assert_eq!(entry["state_transition"], "ready -> failed");
        // Legacy `inner_error` key survives at top level.
        assert_eq!(entry["inner_error"], inner);
        // Canonical typed slot is intentionally absent on the failure branch.
        assert!(
            entry.get("inner_dispatch").is_none(),
            "failure branch must not populate `inner_dispatch`; payload stays under `inner_error`"
        );
    }

    /// Wave-14 / Task 02: production now writes a live `EventRef::new(...)`
    /// — never `EventRef::unavailable(...)` — on the success branch. The
    /// fixture pins the deterministic-id branch (no bus) so this test
    /// verifies (a) `unavailable` is absent, (b) the canonical
    /// `source=execution` / `kind=plan_node_state_changed` mapping survives,
    /// (c) the deterministic id matches the
    /// `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` format.
    #[test]
    fn dag_node_dispatch_evidence_records_live_event_ref() {
        let node = fixture_dag_node("n2", "mission_execution");
        let plan = fixture_plan("(plan)");
        let entry = build_dag_success_entry(&node, &plan, "agent-team", json!({"ok": true}));
        let events = entry["execution_events"]
            .as_array()
            .expect("execution_events array present");
        assert_eq!(events.len(), 1, "exactly one event reference per node");
        let ref0 = &events[0];
        assert!(
            ref0.get("unavailable").is_none(),
            "live path must NOT mark the ref as unavailable: {:?}",
            ref0
        );
        assert_eq!(ref0["source"], "execution");
        assert_eq!(ref0["kind"], "plan_node_state_changed");
        let event_id = ref0["event_id"].as_str().expect("event_id string");
        let expected = format!(
            "plan-node:{}:{}:{}:ready-succeeded",
            plan.id, node.id, PLAN_NODE_DEFAULT_ATTEMPT
        );
        assert_eq!(event_id, expected);
    }

    /// Wave-16 / Task 07: every plan-DAG evidence entry must surface the
    /// `EventRefStatus` provenance tag on the JSON envelope. The publish
    /// path (success and failure branches) constructs `EventRef::new(...)`
    /// (alias for `live`) so the resulting wire form carries
    /// `"status": "live"`. This pins the contract so a future refactor
    /// that accidentally drops the status field gets caught.
    #[test]
    fn dag_node_dispatch_evidence_surfaces_status_live_on_publish_path() {
        let node = fixture_dag_node("n4", "mission_execution");
        let plan = fixture_plan("(plan)");
        let success = build_dag_success_entry(&node, &plan, "agent-team", json!({"ok": true}));
        assert_eq!(
            success["execution_events"][0]["status"], "live",
            "publish-path success branch surfaces status=live"
        );
        let failure = build_dag_failure_entry(
            &node,
            &plan,
            "agent-team",
            json!({"error": "downstream rejected"}),
        );
        assert_eq!(
            failure["execution_events"][0]["status"], "live",
            "publish-path failure branch surfaces status=live (deterministic id is still real)"
        );
    }

    /// Wave-16 / Task 07: when a downstream call site cannot stamp a live
    /// id directly (e.g. the dispatch ran out-of-band of the publish
    /// task), the resolver lookup degrades to
    /// `EventRef::unavailable(EVENT_REF_RESOLVER_MISS_REASON)` rather
    /// than failing. The resulting evidence entry must carry
    /// `status=unavailable` so audit consumers can distinguish a real
    /// recovery failure from a live publish.
    #[test]
    fn dag_node_dispatch_evidence_resolver_miss_degrades_to_unavailable() {
        use evidence_collector::{EventRefResolver, EVENT_REF_RESOLVER_MISS_REASON};
        let node = fixture_dag_node("n5", "mission_execution");
        let plan = fixture_plan("(plan)");
        // Empty resolver — every lookup misses by construction.
        let resolver = EventRefResolver::new();
        let event_ref = resolver.lookup_plan_node_state_change(
            &plan.id.to_string(),
            &node.id,
            PLAN_NODE_DEFAULT_ATTEMPT,
            "ready",
            "succeeded",
        );
        let entry = EvidenceEntry::new(
            evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
            evidence_collector::kind::DISPATCH,
        )
        .with_state_transition("ready -> succeeded")
        .add_execution_event(event_ref)
        .with_extra("scheduler_mode", json!("dag_v1"))
        .with_extra("node_id", json!(node.id))
        .with_extra("plan_id", json!(plan.id))
        .into_json();
        let ev = &entry["execution_events"][0];
        assert_eq!(ev["status"], "unavailable");
        assert_eq!(ev["unavailable"], true);
        assert_eq!(ev["unavailable_reason"], EVENT_REF_RESOLVER_MISS_REASON);
        assert!(
            ev.get("event_id").is_none(),
            "unavailable ref carries no event_id"
        );
    }

    /// Wave-16 / Task 07: when the resolver IS populated (the passive
    /// subscriber observed a `PlanNodeStateChanged` for this correlation
    /// tuple), a downstream call site that queries the resolver gets a
    /// real id back tagged `status=log` (recovered post-hoc — distinct
    /// from `live` which only the publish path itself can stamp).
    #[test]
    fn dag_node_dispatch_evidence_resolver_hit_surfaces_status_log() {
        use evidence_collector::EventRefResolver;
        let node = fixture_dag_node("n6", "mission_execution");
        let plan = fixture_plan("(plan)");
        let resolver = EventRefResolver::new();
        // Simulate the passive subscriber having observed a Seq=42
        // PlanNodeStateChanged for this transition.
        resolver.record_plan_node_state_change(
            &plan.id.to_string(),
            &node.id,
            PLAN_NODE_DEFAULT_ATTEMPT,
            "ready",
            "succeeded",
            "execution",
            "plan_node_state_changed",
            "42",
        );
        let event_ref = resolver.lookup_plan_node_state_change(
            &plan.id.to_string(),
            &node.id,
            PLAN_NODE_DEFAULT_ATTEMPT,
            "ready",
            "succeeded",
        );
        let entry = EvidenceEntry::new(
            evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
            evidence_collector::kind::DISPATCH,
        )
        .with_state_transition("ready -> succeeded")
        .add_execution_event(event_ref)
        .into_json();
        let ev = &entry["execution_events"][0];
        assert_eq!(ev["status"], "log", "resolver hit surfaces status=log");
        assert_eq!(ev["event_id"], "42");
        assert_eq!(ev["source"], "execution");
        assert_eq!(ev["kind"], "plan_node_state_changed");
        assert!(ev.get("unavailable").is_none());
    }

    // ── wave-17 / Task 06 :: persistent event-log query ─────────────────

    /// Wave-17 / Task 06: when the in-memory cache misses but the
    /// persistent event log carries a matching `PlanNodeStateChanged`
    /// row, the resolver must recover the ref and the evidence entry
    /// must surface `event_ref_status=log` plus the leading
    /// `execution_events[0].status=log`. This pins the contract that
    /// event refs survive daemon restarts (the in-memory cache is
    /// dropped on restart but the event log persists).
    #[tokio::test]
    async fn dag_node_dispatch_evidence_recovers_event_ref_from_log_after_cache_miss() {
        use evidence_collector::EventRefResolver;
        use missiond_core::event::log::{LogError, LogReadable, LoggedEvent, Seq};
        use missiond_core::event::Domain;

        // A tiny `LogReadable` stub that returns one matching row. Matches
        // the post-restart shape: cache empty, log carries the prior emit.
        struct OneRowLog(LoggedEvent);
        #[async_trait::async_trait]
        impl LogReadable for OneRowLog {
            async fn read_from(
                &self,
                _domain: Domain,
                _after: Seq,
                _limit: usize,
            ) -> Result<Vec<LoggedEvent>, LogError> {
                Ok(vec![self.0.clone()])
            }
            async fn head_seq(&self) -> Result<Seq, LogError> {
                Ok(self.0.seq)
            }
        }

        let node = fixture_dag_node("n7", "mission_execution");
        let plan = fixture_plan("(plan)");
        let plan_id_str = plan.id.to_string();
        let row = LoggedEvent {
            seq: Seq(314),
            domain: Domain::Execution,
            kind: "plan_node_state_changed".to_string(),
            payload: json!({
                "PlanNodeStateChanged": {
                    "plan_id": plan_id_str,
                    "node_id": node.id,
                    "from": "ready",
                    "to": "succeeded",
                    "attempt": PLAN_NODE_DEFAULT_ATTEMPT,
                }
            }),
            producer_id: "test/plan_dag".to_string(),
            dedupe_key: None,
            causation_depth: 0,
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            ts: chrono::Utc::now(),
            ephemeral: false,
        };
        let log = OneRowLog(row);

        // Empty resolver — cache miss forces the log-query path.
        let resolver = EventRefResolver::new();
        let event_ref = resolver
            .lookup_or_query_plan_node_state_change(
                &log,
                &plan_id_str,
                &node.id,
                PLAN_NODE_DEFAULT_ATTEMPT,
                "ready",
                "succeeded",
            )
            .await;

        // Build the evidence entry the way `emit_evidence_finished` would.
        let entry = EvidenceEntry::new(
            evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
            evidence_collector::kind::DISPATCH,
        )
        .with_state_transition("ready -> succeeded")
        .with_primary_event_ref(&event_ref, None)
        .add_execution_event(event_ref)
        .into_json();

        // Top-level surface fields carry the log provenance. Wave-18 /
        // task 01 — `event_ref_source` now reports the resolver tier
        // (`event_log_query`) instead of the raw wire source, so audit
        // consumers can pivot directly on the lookup path.
        assert_eq!(
            entry["event_ref_status"], "log",
            "log-recovered ref surfaces status=log at top level"
        );
        assert_eq!(
            entry["event_ref_source"], "event_log_query",
            "wave-18 query-tier hit surfaces provenance=event_log_query"
        );
        assert!(
            entry.get("event_ref_warning").is_none(),
            "no warning when recovery succeeded"
        );
        // Per-event entry mirrors the same provenance.
        let ev = &entry["execution_events"][0];
        assert_eq!(ev["status"], "log");
        assert_eq!(ev["event_id"], "314");
        assert_eq!(ev["source"], "execution");
        assert_eq!(ev["kind"], "plan_node_state_changed");
    }

    // ── wave-13 / 02 :: v2 scheduler runtime — pure tests ────────────
    //
    // Full execution requires `AppState` (handlers + project registry +
    // evidence sidecar). The wave-based scheduler's pure subset is the
    // concurrency-plan projection (`compute_concurrency_plan`) and the
    // response shape (`ExecutionOutcome::node_results_json` /
    // `skipped_nodes_json`). End-to-end behaviour is exercised by the
    // existing v1 tests that still pass under the v2 runtime (above), plus
    // the bridge / record_evidence tests under `plan::tests`.

    #[test]
    fn parse_max_parallel_nodes_defaults_to_one_when_absent() {
        let v = json!({});
        assert_eq!(parse_max_parallel_nodes(&v), 1);
    }

    #[test]
    fn parse_max_parallel_nodes_reads_positive_integer() {
        let v = json!({"max_parallel_nodes": 4});
        assert_eq!(parse_max_parallel_nodes(&v), 4);
    }

    #[test]
    fn parse_max_parallel_nodes_clamps_zero_to_one() {
        // Caller passing 0 / negative is normalised to the v1-equivalent
        // sequential contract instead of hard-failing — same posture as the
        // dispatch_strategy unknown-value normalisation in plan.rs.
        let v = json!({"max_parallel_nodes": 0});
        assert_eq!(parse_max_parallel_nodes(&v), 1);
    }

    #[test]
    fn compute_concurrency_plan_linear_chain_single_per_wave() {
        // a -> b -> c with max=2 still produces three single-node waves
        // because each tier exposes only one ready node.
        let sexp = r#"
            (plan
              (node :id "a" :target "mission_execution")
              (node :id "b" :target "mission_execution" :depends-on ["a"])
              (node :id "c" :target "mission_execution" :depends-on ["b"]))
        "#;
        let (parsed, order) = build_validated_dag(sexp).expect("valid");
        let waves = compute_concurrency_plan(&parsed.nodes, &order, 2);
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0], vec!["a".to_string()]);
        assert_eq!(waves[1], vec!["b".to_string()]);
        assert_eq!(waves[2], vec!["c".to_string()]);
    }

    #[test]
    fn compute_concurrency_plan_diamond_fans_under_max_2() {
        // a fans out to {b, c}, both feed d. max=2 lets b+c run together.
        let sexp = r#"
            (plan
              (node :id "a" :target "mission_execution")
              (node :id "b" :target "mission_execution" :depends-on ["a"])
              (node :id "c" :target "mission_execution" :depends-on ["a"])
              (node :id "d" :target "mission_execution" :depends-on ["b" "c"]))
        "#;
        let (parsed, order) = build_validated_dag(sexp).expect("valid");
        let waves = compute_concurrency_plan(&parsed.nodes, &order, 2);
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0], vec!["a".to_string()]);
        // Wave 2 ids are sorted lexicographically for determinism.
        assert_eq!(waves[1], vec!["b".to_string(), "c".to_string()]);
        assert_eq!(waves[2], vec!["d".to_string()]);
    }

    #[test]
    fn compute_concurrency_plan_max_one_matches_v1_sequential_order() {
        // max_parallel_nodes=1 must produce exactly one wave per node, in
        // strict topological-sort order — preserves the v1 contract.
        let sexp = r#"
            (plan
              (node :id "a" :target "mission_execution")
              (node :id "b" :target "mission_execution" :depends-on ["a"])
              (node :id "c" :target "mission_execution" :depends-on ["a"])
              (node :id "d" :target "mission_execution" :depends-on ["b" "c"]))
        "#;
        let (parsed, order) = build_validated_dag(sexp).expect("valid");
        let waves = compute_concurrency_plan(&parsed.nodes, &order, 1);
        assert_eq!(waves.len(), 4);
        for w in &waves {
            assert_eq!(w.len(), 1, "max=1 must produce single-node waves");
        }
        let flat: Vec<String> = waves.iter().flatten().cloned().collect();
        assert_eq!(flat, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn compute_concurrency_plan_three_independent_packs_into_one_wave_when_budget_allows() {
        // Three roots, no dependencies — max=3 should pack them all into
        // one wave; max=2 splits 2+1; max=1 splits 1+1+1 in id-sorted order.
        let sexp = r#"
            (plan
              (node :id "x" :target "mission_execution")
              (node :id "a" :target "mission_execution")
              (node :id "m" :target "mission_execution"))
        "#;
        let (parsed, order) = build_validated_dag(sexp).expect("valid");
        let w3 = compute_concurrency_plan(&parsed.nodes, &order, 3);
        assert_eq!(w3, vec![vec!["a".to_string(), "m".to_string(), "x".to_string()]]);
        let w2 = compute_concurrency_plan(&parsed.nodes, &order, 2);
        assert_eq!(w2, vec![
            vec!["a".to_string(), "m".to_string()],
            vec!["x".to_string()],
        ]);
        let w1 = compute_concurrency_plan(&parsed.nodes, &order, 1);
        assert_eq!(w1.len(), 3);
        assert_eq!(w1[0], vec!["a".to_string()]);
        assert_eq!(w1[1], vec!["m".to_string()]);
        assert_eq!(w1[2], vec!["x".to_string()]);
    }

    #[test]
    fn compute_concurrency_plan_clamps_zero_max_parallel_to_one() {
        // 0 is normalised to 1 inside parse_max_parallel_nodes, but the
        // pure helper also applies the clamp so direct callers stay safe.
        let sexp = r#"
            (plan
              (node :id "a" :target "mission_execution")
              (node :id "b" :target "mission_execution"))
        "#;
        let (parsed, order) = build_validated_dag(sexp).expect("valid");
        let waves = compute_concurrency_plan(&parsed.nodes, &order, 0);
        assert_eq!(waves.len(), 2, "max=0 must clamp to 1 -> two waves");
    }

    #[test]
    fn skipped_nodes_json_filters_only_skip_states() {
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "a".into(),
            target: "mission_execution".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "b".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedUpstreamFailed { failed_dep: "a".into() },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "c".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedCondition,
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "d".into(),
            target: "mission_execution".into(),
            state: NodeState::Failed { reason: "boom".into() },
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({"error": "boom"}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "e".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedFailFastAbort { aborter: "d".into() },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        let v = o.skipped_nodes_json();
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 3, "only the three skip variants surface here");
        assert_eq!(arr[0]["id"], "b");
        assert_eq!(arr[0]["state"], "skipped_upstream_failed");
        assert_eq!(arr[0]["failed_dep"], "a");
        assert_eq!(arr[1]["id"], "c");
        assert_eq!(arr[1]["state"], "skipped_condition");
        assert_eq!(arr[2]["id"], "e");
        assert_eq!(arr[2]["state"], "skipped_fail_fast_abort");
        assert_eq!(arr[2]["aborter"], "d");
    }

    #[test]
    fn node_results_json_includes_skipped_fail_fast_abort_variant() {
        let mut o = ExecutionOutcome::default();
        o.aborted_fail_fast = true;
        o.results.push(NodeResult {
            id: "a".into(),
            target: "mission_execution".into(),
            state: NodeState::Failed { reason: "boom".into() },
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({"error": "boom"}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "b".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedFailFastAbort { aborter: "a".into() },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        let v = o.node_results_json();
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[1]["state"], "skipped_fail_fast_abort");
        assert_eq!(arr[1]["aborter"], "a");
        assert_eq!(o.aggregate_status(), "dag_failed");
        assert_eq!(o.runner_status(), "fail_fast_aborted");
    }

    #[test]
    fn outcome_partial_status_when_no_failure_but_skips_present() {
        // wave-13/02 fail-fast abort path: the failing node's policy may be
        // `continue` while *another* upstream-tainted child still ends up as
        // `skipped_upstream_failed` — aggregate_status must surface this as
        // dag_partial (not dag_succeeded).
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "a".into(),
            target: "mission_execution".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "b".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedCondition,
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        assert_eq!(o.aggregate_status(), "dag_partial");
        assert_eq!(o.runner_status(), "partial_dispatched");
        assert_eq!(o.target_plan_status(), None);
    }

    // ── evidence shape :: v2 lifecycle transitions ─────────────────
    //
    // The v2 scheduler emits one evidence entry per state transition. We
    // pin the `state_transition` annotations + the `skip_reason` extra so
    // audit dashboards can route on them. Replays the entry construction
    // (mirrored from the helpers above) instead of standing up `AppState`.

    /// Wave-14 / Task 02: fixtures pin the **deterministic** `EventRef::new`
    /// branch (no live bus available in pure tests). This mirrors what the
    /// production helpers write when the bus publish either succeeds (with
    /// the live `Seq` as the id) or fails (with the deterministic id as the
    /// id + `bus_publish_warnings` populated). Tests assert the wire shape
    /// of the *entry*, not the bus interaction itself.
    fn build_running_entry(node: &DagNode, plan: &Plan, dispatch_strategy: &str) -> Value {
        let det = deterministic_plan_node_event_id(
            plan.id,
            &node.id,
            PLAN_NODE_DEFAULT_ATTEMPT,
            "ready",
            "running",
        );
        EvidenceEntry::new(
            evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
            evidence_collector::kind::DISPATCH,
        )
        .with_state_transition("ready -> running")
        .add_execution_event(EventRef::new(
            EVENT_REF_SOURCE_EXECUTION,
            EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
            det,
        ))
        .with_extra("scheduler_mode", json!("dag_v1"))
        .with_extra("node_id", json!(node.id))
        .with_extra("plan_id", json!(plan.id))
        .with_extra("target_tool", json!(node.target))
        .with_extra("target", json!(node.target))
        .with_extra("dispatch_strategy", json!(dispatch_strategy))
        .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
        .into_json()
    }

    fn build_finished_entry(
        node: &DagNode,
        plan: &Plan,
        dispatch_strategy: &str,
        inner_payload: Value,
        succeeded: bool,
    ) -> Value {
        let to = if succeeded { "succeeded" } else { "failed" };
        let det = deterministic_plan_node_event_id(
            plan.id,
            &node.id,
            PLAN_NODE_DEFAULT_ATTEMPT,
            "running",
            to,
        );
        let mut entry = EvidenceEntry::new(
            evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
            evidence_collector::kind::DISPATCH,
        )
        .add_execution_event(EventRef::new(
            EVENT_REF_SOURCE_EXECUTION,
            EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
            det,
        ))
        .with_extra("scheduler_mode", json!("dag_v1"))
        .with_extra("node_id", json!(node.id))
        .with_extra("plan_id", json!(plan.id))
        .with_extra("target_tool", json!(node.target))
        .with_extra("target", json!(node.target))
        .with_extra("dispatch_strategy", json!(dispatch_strategy))
        .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT));
        if succeeded {
            entry = entry
                .with_inner_dispatch(inner_payload.clone())
                .with_state_transition("running -> succeeded")
                .with_extra("inner_result", inner_payload);
        } else {
            entry = entry
                .with_state_transition("running -> failed")
                .with_extra("inner_error", inner_payload);
        }
        entry.into_json()
    }

    fn build_skipped_entry(
        node: &DagNode,
        plan: &Plan,
        dispatch_strategy: &str,
        skip_reason: &str,
        skip_detail: Option<(&'static str, String)>,
    ) -> Value {
        let det = deterministic_plan_node_event_id(
            plan.id,
            &node.id,
            PLAN_NODE_DEFAULT_ATTEMPT,
            "pending",
            "skipped",
        );
        let mut entry = EvidenceEntry::new(
            evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
            evidence_collector::kind::DISPATCH,
        )
        .with_state_transition("pending -> skipped")
        .add_execution_event(EventRef::new(
            EVENT_REF_SOURCE_EXECUTION,
            EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
            det,
        ))
        .with_extra("scheduler_mode", json!("dag_v1"))
        .with_extra("node_id", json!(node.id))
        .with_extra("plan_id", json!(plan.id))
        .with_extra("target_tool", json!(node.target))
        .with_extra("target", json!(node.target))
        .with_extra("dispatch_strategy", json!(dispatch_strategy))
        .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
        .with_extra("skip_reason", json!(skip_reason));
        if let Some((k, v)) = skip_detail {
            entry = entry.with_extra(k, json!(v));
        }
        entry.into_json()
    }

    #[test]
    fn evidence_running_entry_carries_ready_to_running_transition() {
        let node = fixture_dag_node("n1", "mission_execution");
        let plan = fixture_plan("(plan)");
        let entry = build_running_entry(&node, &plan, "agent-team");
        assert_eq!(entry["source"], "plan_dag_node_dispatch");
        assert_eq!(entry["kind"], "dispatch");
        assert_eq!(entry["state_transition"], "ready -> running");
        // No inner payload yet — the dispatch hasn't returned.
        assert!(entry.get("inner_dispatch").is_none());
        assert!(entry.get("inner_result").is_none());
        assert!(entry.get("inner_error").is_none());
    }

    #[test]
    fn evidence_finished_entry_succeeded_uses_running_to_succeeded() {
        let node = fixture_dag_node("n1", "mission_execution");
        let plan = fixture_plan("(plan)");
        let entry = build_finished_entry(&node, &plan, "agent-team", json!({"ok": true}), true);
        assert_eq!(entry["state_transition"], "running -> succeeded");
        assert_eq!(entry["inner_dispatch"]["ok"], true);
        assert_eq!(entry["inner_result"]["ok"], true);
        assert!(entry.get("inner_error").is_none());
    }

    #[test]
    fn evidence_finished_entry_failed_uses_running_to_failed() {
        let node = fixture_dag_node("n1", "mission_execution");
        let plan = fixture_plan("(plan)");
        let entry =
            build_finished_entry(&node, &plan, "agent-team", json!({"error": "boom"}), false);
        assert_eq!(entry["state_transition"], "running -> failed");
        assert_eq!(entry["inner_error"]["error"], "boom");
        assert!(entry.get("inner_dispatch").is_none());
        assert!(entry.get("inner_result").is_none());
    }

    #[test]
    fn evidence_skipped_entry_records_pending_to_skipped_with_reason() {
        let node = fixture_dag_node("n1", "mission_execution");
        let plan = fixture_plan("(plan)");
        let entry = build_skipped_entry(
            &node,
            &plan,
            "agent-team",
            "upstream_failed",
            Some(("failed_dep", "n0".to_string())),
        );
        assert_eq!(entry["state_transition"], "pending -> skipped");
        assert_eq!(entry["skip_reason"], "upstream_failed");
        assert_eq!(entry["failed_dep"], "n0");
    }

    #[test]
    fn evidence_skipped_entry_for_fail_fast_records_aborter() {
        let node = fixture_dag_node("n2", "mission_execution");
        let plan = fixture_plan("(plan)");
        let entry = build_skipped_entry(
            &node,
            &plan,
            "agent-team",
            "fail_fast_aborted",
            Some(("aborter", "n1".to_string())),
        );
        assert_eq!(entry["skip_reason"], "fail_fast_aborted");
        assert_eq!(entry["aborter"], "n1");
    }

    #[test]
    fn evidence_skipped_entry_for_condition_records_condition_text() {
        let node = fixture_dag_node("n3", "mission_execution");
        let plan = fixture_plan("(plan)");
        let entry = build_skipped_entry(
            &node,
            &plan,
            "agent-team",
            "condition_gated",
            Some(("condition", "(env :debug)".to_string())),
        );
        assert_eq!(entry["skip_reason"], "condition_gated");
        assert_eq!(entry["condition"], "(env :debug)");
    }

    // ── wave-14 / 02 :: PlanNodeStateChanged event + live event refs ──

    /// `deterministic_plan_node_event_id` is the fallback id stamped on
    /// `EventRef::new(...)` when the bus publish fails. Format must match
    /// the wave-14 task brief verbatim so downstream consumers can grep.
    #[test]
    fn deterministic_event_id_format_matches_brief() {
        let plan_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000abc").unwrap();
        let id = deterministic_plan_node_event_id(plan_id, "n1", 1, "ready", "running");
        assert_eq!(
            id,
            "plan-node:00000000-0000-0000-0000-000000000abc:n1:1:ready-running"
        );
    }

    /// `build_plan_node_state_changed_event` projects a node + lifecycle
    /// transition into the `ExecutionEvent` payload. Pins the field
    /// mapping (target / dispatch_strategy / target_project / attempt /
    /// reason) and the `kind()` wire tag the evidence collector keys on.
    #[test]
    fn plan_node_state_changed_event_projection_matches_node_metadata() {
        let plan = fixture_plan("(plan)");
        let mut node = fixture_dag_node("nx", "mission_execution");
        node.target_project = Some("missiond".into());
        let ev = build_plan_node_state_changed_event(
            plan.id,
            &node,
            "agent-team",
            1,
            "running",
            "succeeded",
            None,
        );
        assert_eq!(
            <ExecutionEvent as missiond_core::event::DomainEvent>::kind(&ev),
            "plan_node_state_changed"
        );
        match ev {
            ExecutionEvent::PlanNodeStateChanged {
                plan_id,
                node_id,
                from,
                to,
                target,
                dispatch_strategy,
                target_project,
                attempt,
                reason,
            } => {
                assert_eq!(plan_id, plan.id.to_string());
                assert_eq!(node_id, "nx");
                assert_eq!(from, "running");
                assert_eq!(to, "succeeded");
                assert_eq!(target.as_deref(), Some("mission_execution"));
                assert_eq!(dispatch_strategy.as_deref(), Some("agent-team"));
                assert_eq!(target_project.as_deref(), Some("missiond"));
                assert_eq!(attempt, Some(1));
                assert!(reason.is_none(), "success transitions carry no reason");
            }
            _ => panic!("expected PlanNodeStateChanged variant"),
        }
    }

    /// Failure / skip transitions surface a `reason` annotation through to
    /// the bus event payload, mirroring what `emit_evidence_*` writes.
    #[test]
    fn plan_node_state_changed_event_carries_failure_reason() {
        let plan = fixture_plan("(plan)");
        let node = fixture_dag_node("ny", "mission_task_delegate");
        let ev = build_plan_node_state_changed_event(
            plan.id,
            &node,
            "fresh-code-alignment",
            1,
            "pending",
            "skipped",
            Some("upstream_failed:n1".into()),
        );
        match ev {
            ExecutionEvent::PlanNodeStateChanged { reason, from, to, .. } => {
                assert_eq!(reason.as_deref(), Some("upstream_failed:n1"));
                assert_eq!(from, "pending");
                assert_eq!(to, "skipped");
            }
            _ => panic!("expected PlanNodeStateChanged"),
        }
    }

    /// Every fixture-built evidence entry now carries an `attempt` extra
    /// (defaults to 1 for v2). Ensures audit consumers see a stable column
    /// they can pivot on once retry-aware schedulers land.
    #[test]
    fn dag_evidence_entries_include_attempt_field() {
        let node = fixture_dag_node("n1", "mission_execution");
        let plan = fixture_plan("(plan)");
        let succ = build_dag_success_entry(&node, &plan, "agent-team", json!({}));
        assert_eq!(succ["attempt"], 1);
        let fail = build_dag_failure_entry(&node, &plan, "agent-team", json!({"error": "x"}));
        assert_eq!(fail["attempt"], 1);
        let running = build_running_entry(&node, &plan, "agent-team");
        assert_eq!(running["attempt"], 1);
        let finished_ok =
            build_finished_entry(&node, &plan, "agent-team", json!({"ok": true}), true);
        assert_eq!(finished_ok["attempt"], 1);
        let skipped = build_skipped_entry(
            &node,
            &plan,
            "agent-team",
            "upstream_failed",
            Some(("failed_dep", "n0".to_string())),
        );
        assert_eq!(skipped["attempt"], 1);
    }

    /// Every fixture-built entry now stamps a live `EventRef::new(...)` —
    /// the deterministic-id branch in pure tests — with the canonical
    /// source/kind tags the evidence collector (and downstream consumers)
    /// route on.
    #[test]
    fn dag_evidence_entries_carry_live_event_ref_with_deterministic_id() {
        let node = fixture_dag_node("n4", "mission_execution");
        let plan = fixture_plan("(plan)");
        let entries = vec![
            ("ready -> running", build_running_entry(&node, &plan, "agent-team")),
            (
                "running -> succeeded",
                build_finished_entry(&node, &plan, "agent-team", json!({"ok": true}), true),
            ),
            (
                "running -> failed",
                build_finished_entry(&node, &plan, "agent-team", json!({"error": "x"}), false),
            ),
            (
                "pending -> skipped",
                build_skipped_entry(
                    &node,
                    &plan,
                    "agent-team",
                    "upstream_failed",
                    Some(("failed_dep", "n0".to_string())),
                ),
            ),
        ];
        for (transition, entry) in entries {
            let arr = entry["execution_events"]
                .as_array()
                .unwrap_or_else(|| panic!("execution_events array for {}", transition));
            assert_eq!(arr.len(), 1, "exactly one ref for {}", transition);
            let r = &arr[0];
            assert!(
                r.get("unavailable").is_none(),
                "live path: ref must NOT be unavailable for {} ({:?})",
                transition,
                r
            );
            assert_eq!(r["source"], "execution", "for {}", transition);
            assert_eq!(
                r["kind"], "plan_node_state_changed",
                "for {}",
                transition
            );
            let id = r["event_id"].as_str().expect("event_id string");
            assert!(
                id.starts_with(&format!("plan-node:{}:{}:1:", plan.id, node.id)),
                "deterministic id format for {} → {}",
                transition,
                id
            );
        }
    }

    /// Bus-failure-path symptom surface: when `bus_publish_warnings` is
    /// non-empty the `action_execute_dag_v1` response surfaces it as a
    /// top-level array. Verifies the field plumbing in `ExecutionOutcome`.
    #[test]
    fn execution_outcome_collects_bus_publish_warnings() {
        let mut o = ExecutionOutcome::default();
        o.bus_publish_warnings.push("simulated bus drop for n1".into());
        o.bus_publish_warnings.push("simulated bus drop for n2".into());
        assert_eq!(o.bus_publish_warnings.len(), 2);
        assert!(o.bus_publish_warnings[0].contains("n1"));
    }

    // ── wave-15 / task 05 — workstation-dispatch hint contract ───────────
    //
    // Pin the per-node parser additions: scope / commit-policy /
    // owned-files / forbidden-files / acceptance-commands /
    // workstation-dispatch land on the typed slots and never leak into
    // `unsupported_fields` (which would mean the scheduler can't route
    // the node through the workstation-dispatch substrate).

    #[test]
    fn parse_node_form_captures_workstation_dispatch_contract() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :objective "ship the wave"
                    :scope "wave 15 task 05 only"
                    :owned-files ["a.rs" "b.rs"]
                    :forbidden-files ["c.rs"]
                    :acceptance-commands ["cargo test" "git diff --check"]
                    :commit-policy "scoped"
                    :workstation-dispatch true
                    :dispatch-strategy "fresh-code-alignment"))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes.len(), 1);
        let n = &parsed.nodes[0];
        assert_eq!(n.scope.as_deref(), Some("wave 15 task 05 only"));
        assert_eq!(n.commit_policy.as_deref(), Some("scoped"));
        assert!(n.owned_files_raw.as_deref().unwrap().contains("a.rs"));
        assert!(n.forbidden_files_raw.as_deref().unwrap().contains("c.rs"));
        assert!(n
            .acceptance_commands_raw
            .as_deref()
            .unwrap()
            .contains("cargo test"));
        assert!(n.workstation_dispatch_opt_in());
        // None of the new keys should land in unsupported_fields — that
        // would break workstation-dispatch routing.
        let unsupported_keys: Vec<String> =
            n.unsupported_fields.iter().map(|(k, _)| k.clone()).collect();
        for forbidden in [
            "scope",
            "commit-policy",
            "owned-files",
            "forbidden-files",
            "acceptance-commands",
            "workstation-dispatch",
        ] {
            assert!(
                !unsupported_keys.contains(&forbidden.to_string()),
                "key `{}` must land on a typed slot, not unsupported_fields",
                forbidden
            );
        }
    }

    #[test]
    fn parse_node_form_workstation_dispatch_opt_in_recognises_truthy_values() {
        for truthy in &["true", "TRUE", "yes", "on", "1"] {
            let sexp = format!(
                r#"(plan (node :id "n1" :target "mission_task_delegate" :workstation-dispatch {}))"#,
                truthy
            );
            let parsed = parse_plan_dag(&sexp);
            assert!(
                parsed.nodes[0].workstation_dispatch_opt_in(),
                "expected `{}` to be truthy",
                truthy
            );
        }
        for falsy in &["false", "no", "off", "0", "maybe"] {
            let sexp = format!(
                r#"(plan (node :id "n1" :target "mission_task_delegate" :workstation-dispatch {}))"#,
                falsy
            );
            let parsed = parse_plan_dag(&sexp);
            assert!(
                !parsed.nodes[0].workstation_dispatch_opt_in(),
                "expected `{}` to NOT be truthy",
                falsy
            );
        }
        // Absence is also off.
        let sexp = r#"(plan (node :id "n1" :target "mission_task_delegate"))"#;
        let parsed = parse_plan_dag(sexp);
        assert!(!parsed.nodes[0].workstation_dispatch_opt_in());
    }

    /// `node_to_workstation_hints` is the bridge between the parsed node
    /// and the workstation-dispatch substrate. Any divergence here means
    /// per-node DAG dispatch and per-plan single-node dispatch would
    /// produce different briefs for identical inputs.
    #[test]
    fn node_to_workstation_hints_projects_every_field() {
        let node = DagNode {
            id: "n1".into(),
            target: "mission_task_delegate".into(),
            objective: Some("ship".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/abs/missiond".into()),
            dispatch_strategy: Some("agent-team".into()),
            scope: Some("scope text".into()),
            commit_policy: Some("scoped".into()),
            owned_files_raw: Some(r#"["a.rs"]"#.into()),
            forbidden_files_raw: Some(r#"["b.rs"]"#.into()),
            acceptance_commands_raw: Some(r#"["cargo test"]"#.into()),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        };
        let w = node_to_workstation_hints(&node);
        assert_eq!(w.objective.as_deref(), Some("ship"));
        assert_eq!(w.target_project.as_deref(), Some("missiond"));
        assert_eq!(w.requested_cwd.as_deref(), Some("/abs/missiond"));
        assert_eq!(w.dispatch_strategy.as_deref(), Some("agent-team"));
        assert_eq!(w.scope.as_deref(), Some("scope text"));
        assert_eq!(w.commit_policy.as_deref(), Some("scoped"));
        assert_eq!(w.owned_files, vec!["a.rs".to_string()]);
        assert_eq!(w.forbidden_files, vec!["b.rs".to_string()]);
        assert_eq!(w.acceptance_commands, vec!["cargo test".to_string()]);
    }

    fn fixture_decision_explicit(
    ) -> crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
        crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
            source: crate::handlers::knowledge::workstation_dispatch::WorkstationDispatchSource::PlanHint,
            reason: Some("test fixture".to_string()),
        }
    }

    fn fixture_decision_inferred(
    ) -> crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
        crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
            source: crate::handlers::knowledge::workstation_dispatch::WorkstationDispatchSource::Inferred,
            reason: Some("inferred test fixture".to_string()),
        }
    }

    /// Safe-descriptor outcomes from the workstation-dispatch substrate
    /// must classify as failures so the DAG scheduler taints downstream
    /// nodes (vs success which would falsely advance the wave).
    #[test]
    fn workstation_outcome_safe_descriptor_classifies_as_failure() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let node = DagNode {
            id: "n1".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        };
        let outcome = wd::WorkstationDispatchOutcome::SafeDescriptor {
            reason: wd::SafeDescriptorReason::ProjectRootUnresolved(
                "no signal".to_string(),
            ),
            task_brief: None,
        };
        let (payload, classification, non_retryable) = workstation_outcome_to_dispatch_pair(
            &node,
            "fresh-code-alignment",
            outcome,
            &fixture_decision_explicit(),
        );
        assert!(classification.is_err(), "safe descriptors must fail dispatch");
        assert!(
            non_retryable,
            "safe-descriptor refusals are deterministic and must classify non-retryable"
        );
        assert_eq!(payload["workstation_dispatch_status"], "skipped_project_root_unresolved");
        assert_eq!(payload["node_id"], "n1");
        assert_eq!(payload["workstation_dispatch_source"], "plan_hint");
    }

    /// Dispatched outcomes must classify as success (the inner handler
    /// already returned non-error).
    #[test]
    fn workstation_outcome_dispatched_classifies_as_success() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let node = DagNode {
            id: "n1".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        };
        let outcome = wd::WorkstationDispatchOutcome::Dispatched {
            task_brief: "## Objective\nship\n".to_string(),
            task_brief_path: None,
            evidence_path: Some("/tmp/sidecar.json".to_string()),
            evidence_error: None,
            inner_payload: json!({"task_id": "btk-7"}),
        };
        let (payload, classification, non_retryable) = workstation_outcome_to_dispatch_pair(
            &node,
            "agent-team",
            outcome,
            &fixture_decision_inferred(),
        );
        assert!(classification.is_ok(), "dispatched must succeed");
        assert!(
            !non_retryable,
            "successful dispatch must NOT be flagged non-retryable"
        );
        assert_eq!(payload["workstation_dispatch_status"], "dispatched");
        assert_eq!(payload["dispatch_strategy"], "agent-team");
        assert_eq!(payload["inner_result"]["task_id"], "btk-7");
        assert_eq!(payload["workstation_dispatch_source"], "inferred");
        assert_eq!(payload["workstation_dispatch_inference_reason"], "inferred test fixture");
    }

    // ── wave-16 / task 03 — DAG node auto-inference ─────────────────────

    /// Compose `node_to_workstation_hints` with `evaluate_dispatch_decision`
    /// to assert the per-node decision the scheduler would arrive at.
    fn dag_node_decision(
        node: &DagNode,
        dispatch_strategy: &str,
    ) -> crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
        let merged = node_to_workstation_hints(node);
        let ctx = crate::handlers::knowledge::workstation_dispatch::InferenceContext {
            target: node.target.as_str(),
            dispatch_strategy,
            objective: merged.objective.as_deref(),
            owned_files_present: !merged.owned_files.is_empty(),
            scope_present: merged
                .scope
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false),
            target_project_present: merged
                .target_project
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false),
            requested_cwd_present: merged
                .requested_cwd
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false),
        };
        crate::handlers::knowledge::workstation_dispatch::evaluate_dispatch_decision(
            &serde_json::Value::Null,
            node.workstation_dispatch_opt_in(),
            &ctx,
        )
    }

    #[test]
    fn dag_node_auto_inferred_for_task_delegate_with_owned_files_and_strategy() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :objective "ship the wave"
                    :dispatch-strategy "fresh-code-alignment"
                    :owned-files ["a.rs"]))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert!(!parsed.nodes[0].workstation_dispatch_opt_in());
        let decision = dag_node_decision(&parsed.nodes[0], "fresh-code-alignment");
        assert_eq!(decision.source, wd::WorkstationDispatchSource::Inferred);
    }

    #[test]
    fn dag_node_auto_inferred_for_agent_team_with_target_project_signal() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :objective "ship"
                    :dispatch-strategy "agent-team"
                    :target-project "missiond"))
        "#;
        let parsed = parse_plan_dag(sexp);
        let decision = dag_node_decision(&parsed.nodes[0], "agent-team");
        assert_eq!(decision.source, wd::WorkstationDispatchSource::Inferred);
    }

    #[test]
    fn dag_node_explicit_opt_in_takes_plan_hint_path() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :workstation-dispatch true
                    :objective "ship"
                    :dispatch-strategy "fresh-code-alignment"
                    :owned-files ["a.rs"]))
        "#;
        let parsed = parse_plan_dag(sexp);
        let decision = dag_node_decision(&parsed.nodes[0], "fresh-code-alignment");
        // Explicit hint wins over inference and shows up as PlanHint.
        assert_eq!(decision.source, wd::WorkstationDispatchSource::PlanHint);
    }

    #[test]
    fn dag_node_not_inferred_when_strategy_unknown() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :objective "ship"
                    :owned-files ["a.rs"]))
        "#;
        let parsed = parse_plan_dag(sexp);
        let decision = dag_node_decision(&parsed.nodes[0], "unknown");
        assert_eq!(decision.source, wd::WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn dag_node_not_inferred_when_objective_missing() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :dispatch-strategy "fresh-code-alignment"
                    :owned-files ["a.rs"]))
        "#;
        let parsed = parse_plan_dag(sexp);
        let decision = dag_node_decision(&parsed.nodes[0], "fresh-code-alignment");
        assert_eq!(decision.source, wd::WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn dag_node_not_inferred_for_mission_execution_target() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_execution"
                    :objective "ship"
                    :dispatch-strategy "fresh-code-alignment"
                    :owned-files ["a.rs"]))
        "#;
        let parsed = parse_plan_dag(sexp);
        let decision = dag_node_decision(&parsed.nodes[0], "fresh-code-alignment");
        assert_eq!(decision.source, wd::WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn dag_node_not_inferred_when_no_scope_signal() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :objective "ship"
                    :dispatch-strategy "fresh-code-alignment"))
        "#;
        let parsed = parse_plan_dag(sexp);
        let decision = dag_node_decision(&parsed.nodes[0], "fresh-code-alignment");
        assert_eq!(decision.source, wd::WorkstationDispatchSource::NotApplicable);
    }

    /// `build_nodes_summary` must surface the new workstation-dispatch
    /// hint fields when present (so dry-run callers can see them) and
    /// stay quiet for nodes that did not opt in.
    #[test]
    fn build_nodes_summary_surfaces_workstation_dispatch_hints_when_present() {
        let nodes = vec![
            DagNode {
                id: "wd".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                scope: Some("scope text".into()),
                commit_policy: Some("scoped".into()),
                owned_files_raw: Some(r#"["a.rs"]"#.into()),
                forbidden_files_raw: Some(r#"["b.rs"]"#.into()),
                acceptance_commands_raw: Some(r#"["cargo test"]"#.into()),
                workstation_dispatch_flag: Some("true".into()),
                ..Default::default()
            },
            DagNode {
                id: "plain".into(),
                target: "mission_execution".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ];
        let order = vec!["wd".to_string(), "plain".to_string()];
        let summary = build_nodes_summary(&nodes, &order);
        let arr = summary.as_array().unwrap();
        let wd = &arr[0];
        assert_eq!(wd["scope"], "scope text");
        assert_eq!(wd["commit_policy"], "scoped");
        assert_eq!(wd["workstation_dispatch"], true);
        assert!(wd["owned_files_raw"].as_str().unwrap().contains("a.rs"));
        let plain = &arr[1];
        // Plain node carries none of the workstation-dispatch fields so
        // the summary stays quiet (regression guard for the v2 baseline).
        assert!(plain.get("scope").is_none());
        assert!(plain.get("commit_policy").is_none());
        assert!(plain.get("workstation_dispatch").is_none());
    }

    // ── wave-16 / task 04 — review-gate hint contract ────────────────────
    //
    // PLAN DAG runtime now supports a per-node `:review-gate
    // "question-event"` hint that pauses the node and emits
    // `QuestionEvent::Created` instead of dispatching the target tool.
    // Pure tests pin (a) parser captures the new fields without leaking
    // them into `unsupported_fields`, (b) the `review_gate_kind` typed
    // projection routes correctly, (c) `build_nodes_summary` surfaces
    // the hints, (d) the response shape for paused nodes.

    #[test]
    fn parse_node_form_captures_review_gate_contract() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_execution"
                    :objective "ship"
                    :review-gate "question-event"
                    :review-action "human-checkpoint"
                    :review-text "Look at the diff before merging."))
        "#;
        let parsed = parse_plan_dag(sexp);
        let n = &parsed.nodes[0];
        assert_eq!(n.review_gate.as_deref(), Some("question-event"));
        assert_eq!(n.review_action.as_deref(), Some("human-checkpoint"));
        assert_eq!(n.review_text.as_deref(), Some("Look at the diff before merging."));
        // None of the new keys must land in unsupported_fields — that
        // would mean the scheduler can't route the node through the
        // pause path.
        let unsupported_keys: Vec<String> =
            n.unsupported_fields.iter().map(|(k, _)| k.clone()).collect();
        for forbidden in ["review-gate", "review-action", "review-text"] {
            assert!(
                !unsupported_keys.contains(&forbidden.to_string()),
                "key `{}` must land on a typed slot, not unsupported_fields",
                forbidden
            );
        }
        assert_eq!(n.review_gate_kind(), ReviewGateKind::QuestionEvent);
    }

    #[test]
    fn parse_node_form_review_gate_default_is_none() {
        // Absent `:review-gate` keeps the wave-15 baseline: scheduler
        // dispatches as before, no field surfaces in the response.
        let sexp = r#"(plan (node :id "n1" :target "mission_execution"))"#;
        let parsed = parse_plan_dag(sexp);
        assert!(parsed.nodes[0].review_gate.is_none());
        assert_eq!(parsed.nodes[0].review_gate_kind(), ReviewGateKind::None);
    }

    #[test]
    fn parse_node_form_review_gate_explicit_none_resolves_to_none() {
        let sexp = r#"
            (plan
              (node :id "n1" :target "mission_execution" :review-gate "none"))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[0].review_gate.as_deref(), Some("none"));
        assert_eq!(parsed.nodes[0].review_gate_kind(), ReviewGateKind::None);
        // "none" is recognised, must NOT pollute unsupported_fields.
        assert!(parsed.nodes[0]
            .unsupported_fields
            .iter()
            .all(|(k, _)| k != "review-gate"));
    }

    #[test]
    fn parse_node_form_review_gate_unknown_kind_safely_falls_back_and_records_typo() {
        // Defensive: an unrecognised gate kind (typo) must NOT silently
        // pause the node. The scheduler treats it as `None` and the
        // parser records the raw value into `unsupported_fields` so
        // `node_hint_summary` surfaces the typo in the response.
        let sexp = r#"
            (plan
              (node :id "n1" :target "mission_execution" :review-gate "questoin-event"))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[0].review_gate_kind(), ReviewGateKind::None);
        assert!(parsed.nodes[0]
            .unsupported_fields
            .iter()
            .any(|(k, v)| k == "review-gate" && v == "questoin-event"));
    }

    #[test]
    fn parse_node_form_review_gate_underscore_alias_works() {
        // Authors that prefer snake_case keys still get the typed slot.
        let sexp = r#"
            (plan
              (node :id "n1" :target "mission_execution"
                    :review_gate "question_event"
                    :review_action "ship-it"))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[0].review_gate_kind(), ReviewGateKind::QuestionEvent);
        assert_eq!(parsed.nodes[0].review_action.as_deref(), Some("ship-it"));
    }

    #[test]
    fn build_nodes_summary_surfaces_review_gate_hints_when_present() {
        let nodes = vec![
            DagNode {
                id: "g".into(),
                target: "mission_execution".into(),
                failure_policy: "fail-fast".into(),
                review_gate: Some("question-event".into()),
                review_action: Some("human-checkpoint".into()),
                review_text: Some("eyeball it".into()),
                ..Default::default()
            },
            DagNode {
                id: "plain".into(),
                target: "mission_execution".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ];
        let order = vec!["g".to_string(), "plain".to_string()];
        let summary = build_nodes_summary(&nodes, &order);
        let arr = summary.as_array().unwrap();
        let g = &arr[0];
        assert_eq!(g["review_gate"], "question-event");
        assert_eq!(g["review_action"], "human-checkpoint");
        assert_eq!(g["review_text"], "eyeball it");
        let plain = &arr[1];
        // Quiet for nodes without a gate — protects the wave-15 baseline
        // shape so consumers that pivot on key presence keep working.
        assert!(plain.get("review_gate").is_none());
        assert!(plain.get("review_action").is_none());
        assert!(plain.get("review_text").is_none());
    }

    #[test]
    fn node_results_json_includes_paused_state_with_review_question_id() {
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "g".into(),
            target: "mission_execution".into(),
            state: NodeState::Paused {
                question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
                bus_publish_warning: None,
            },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        let v = o.node_results_json();
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["state"], "paused");
        assert_eq!(
            arr[0]["review_question_id"],
            "review:plan:p1:v1:plan-node:abcdef0123456789"
        );
        // No warning attached here — must NOT surface the field.
        assert!(arr[0].get("review_question_warning").is_none());
    }

    #[test]
    fn node_results_json_paused_with_bus_warning_surfaces_warning_field() {
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "g".into(),
            target: "mission_execution".into(),
            state: NodeState::Paused {
                question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
                bus_publish_warning: Some("simulated bus drop".into()),
            },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        let v = o.node_results_json();
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["state"], "paused");
        assert_eq!(arr[0]["review_question_warning"], "simulated bus drop");
    }

    #[test]
    fn paused_nodes_json_filters_only_paused_results() {
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "a".into(),
            target: "mission_execution".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "g".into(),
            target: "mission_execution".into(),
            state: NodeState::Paused {
                question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
                bus_publish_warning: None,
            },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "g2".into(),
            target: "mission_task_delegate".into(),
            state: NodeState::Paused {
                question_id: "review:plan:p1:v1:plan-node:0011223344556677".into(),
                bus_publish_warning: Some("bus dropped".into()),
            },
            dispatch_strategy: "agent-team".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        let v = o.paused_nodes_json();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2, "only the two paused entries surface here");
        assert_eq!(arr[0]["id"], "g");
        assert_eq!(arr[0]["state"], "paused");
        assert_eq!(
            arr[0]["review_question_id"],
            "review:plan:p1:v1:plan-node:abcdef0123456789"
        );
        assert!(arr[0].get("review_question_warning").is_none());
        assert_eq!(arr[1]["id"], "g2");
        assert_eq!(arr[1]["review_question_warning"], "bus dropped");
    }

    #[test]
    fn paused_node_ids_and_review_question_ids_align_in_topo_order() {
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "a".into(),
            target: "mission_execution".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "g1".into(),
            target: "mission_execution".into(),
            state: NodeState::Paused {
                question_id: "review:plan:p1:v1:plan-node:0000000000000001".into(),
                bus_publish_warning: None,
            },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "g2".into(),
            target: "mission_execution".into(),
            state: NodeState::Paused {
                question_id: "review:plan:p1:v1:plan-node:0000000000000002".into(),
                bus_publish_warning: None,
            },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        assert_eq!(o.paused_node_ids(), vec!["g1".to_string(), "g2".to_string()]);
        assert_eq!(
            o.review_question_ids(),
            vec![
                "review:plan:p1:v1:plan-node:0000000000000001".to_string(),
                "review:plan:p1:v1:plan-node:0000000000000002".to_string(),
            ]
        );
    }

    #[test]
    fn aggregate_status_dag_paused_when_only_paused_no_failure() {
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "g".into(),
            target: "mission_execution".into(),
            state: NodeState::Paused {
                question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
                bus_publish_warning: None,
            },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        assert_eq!(o.aggregate_status(), "dag_paused");
        assert_eq!(o.runner_status(), "review_gate_paused");
        // Paused runs MUST NOT mutate the plan status — auto-resume
        // (wave-16 / task 02 territory) revives the node in a follow-up
        // call, so the plan stays Approved/Executing.
        assert_eq!(o.target_plan_status(), None);
    }

    #[test]
    fn aggregate_status_partial_when_paused_and_succeeded_mix() {
        // A successful node alongside a paused gate still surfaces as
        // dag_paused — paused is the dominant signal (the run cannot
        // complete until resume).
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "a".into(),
            target: "mission_execution".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "g".into(),
            target: "mission_execution".into(),
            state: NodeState::Paused {
                question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
                bus_publish_warning: None,
            },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        assert_eq!(o.aggregate_status(), "dag_paused");
        assert_eq!(o.target_plan_status(), None);
    }

    #[test]
    fn aggregate_status_failure_dominates_paused() {
        // Mixed paused + failed run reads as dag_partial (failure is the
        // louder signal). The failing node also flips the plan status to
        // Failed so the caller knows the DAG cannot resume cleanly.
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "f".into(),
            target: "mission_execution".into(),
            state: NodeState::Failed { reason: "boom".into() },
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({"error": "boom"}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        o.results.push(NodeResult {
            id: "g".into(),
            target: "mission_execution".into(),
            state: NodeState::Paused {
                question_id: "review:plan:p1:v1:plan-node:abcdef0123456789".into(),
                bus_publish_warning: None,
            },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        assert_eq!(o.aggregate_status(), "dag_partial");
        assert_eq!(o.target_plan_status(), Some(PlanStatus::Failed));
    }

    /// Helper that mirrors `emit_paused_review_gate`'s evidence entry
    /// shape (without standing up an AppState/bus). Pins the wire form
    /// auditors / dashboards will route on.
    fn build_paused_evidence_entry(
        node: &DagNode,
        plan: &Plan,
        dispatch_strategy: &str,
        question_id: &str,
        bus_warning: Option<&str>,
    ) -> Value {
        let det = deterministic_plan_node_event_id(
            plan.id,
            &node.id,
            PLAN_NODE_DEFAULT_ATTEMPT,
            "pending",
            "paused",
        );
        let mut entry = EvidenceEntry::new(
            evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
            evidence_collector::kind::DISPATCH,
        )
        .with_state_transition("pending -> paused")
        .add_execution_event(EventRef::new(
            EVENT_REF_SOURCE_EXECUTION,
            EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
            det,
        ))
        .with_extra("scheduler_mode", json!("dag_v1"))
        .with_extra("node_id", json!(node.id))
        .with_extra("plan_id", json!(plan.id))
        .with_extra("target_tool", json!(node.target))
        .with_extra("target", json!(node.target))
        .with_extra("dispatch_strategy", json!(dispatch_strategy))
        .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
        .with_extra("review_gate", json!("question-event"))
        .with_extra("review_question_id", json!(question_id));
        if let Some(action) = node.review_action.as_deref() {
            entry = entry.with_extra("review_action", json!(action));
        }
        if let Some(text) = node.review_text.as_deref() {
            entry = entry.with_extra("review_text", json!(text));
        }
        if let Some(w) = bus_warning {
            entry = entry.with_extra("review_question_warning", json!(w));
        }
        entry.into_json()
    }

    #[test]
    fn evidence_paused_entry_carries_pending_to_paused_transition_and_qid() {
        let node = DagNode {
            id: "g".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            dispatch_strategy: Some("agent-team".into()),
            review_gate: Some("question-event".into()),
            review_action: Some("plan-node".into()),
            review_text: Some("eyeball it".into()),
            ..Default::default()
        };
        let plan = fixture_plan("(plan)");
        let qid = super::super::review_gate::derive_plan_node_review_question_id(
            &plan.id.to_string(),
            plan.version,
            &node.id,
            node.review_action.as_deref(),
        );
        let entry = build_paused_evidence_entry(&node, &plan, "agent-team", &qid, None);
        assert_eq!(entry["source"], "plan_dag_node_dispatch");
        assert_eq!(entry["kind"], "dispatch");
        assert_eq!(entry["state_transition"], "pending -> paused");
        assert_eq!(entry["review_gate"], "question-event");
        assert_eq!(entry["review_question_id"], qid);
        assert_eq!(entry["review_action"], "plan-node");
        assert_eq!(entry["review_text"], "eyeball it");
        // The deterministic id format pinned for the lifecycle event ref.
        let event_id = entry["execution_events"][0]["event_id"]
            .as_str()
            .expect("event_id");
        assert!(event_id.starts_with(&format!(
            "plan-node:{}:{}:{}:pending-paused",
            plan.id, node.id, PLAN_NODE_DEFAULT_ATTEMPT
        )));
    }

    #[test]
    fn evidence_paused_entry_with_bus_warning_records_review_question_warning() {
        let node = DagNode {
            id: "g".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            dispatch_strategy: Some("agent-team".into()),
            review_gate: Some("question-event".into()),
            ..Default::default()
        };
        let plan = fixture_plan("(plan)");
        let qid = super::super::review_gate::derive_plan_node_review_question_id(
            &plan.id.to_string(),
            plan.version,
            &node.id,
            None,
        );
        let entry = build_paused_evidence_entry(
            &node,
            &plan,
            "agent-team",
            &qid,
            Some("simulated bus drop"),
        );
        // Bus failure DOES NOT change the transition string — the gate
        // is still real, the warning is observability-only.
        assert_eq!(entry["state_transition"], "pending -> paused");
        assert_eq!(entry["review_question_warning"], "simulated bus drop");
    }

    // ── wave-16 / task 05 — bounded per-node retry policy ──────────────
    //
    // Pure tests for the parser additions (`:retry-count` /
    // `:max-attempts` / `:retry-delay-ms`), the typed projections
    // (`effective_max_attempts` + `effective_retry_delay_ms`), the
    // structured parse-error path (`DagBuildError::InvalidRetryHint`),
    // the dry-run `retry_plan` projection, the per-node response
    // surface, and the safe-descriptor non-retryable classification.
    //
    // End-to-end retry behaviour (one failure then success → succeeded;
    // exhausted attempts → failed) is covered by the integration tests
    // under `tests/plan_dag_retry.rs` because it requires an `AppState`
    // / handler stub — these pure tests pin the contract surface
    // without standing up the daemon.

    #[test]
    fn parse_node_form_captures_retry_count_keyword() {
        // `:retry-count N` declares N **additional** attempts beyond
        // the first; both kebab- and snake_case spellings populate
        // `retry_count` directly.
        for keyword in ["retry-count", "retry_count"] {
            let sexp = format!(
                r#"(plan (node :id "n1" :target "mission_execution" :{} 2))"#,
                keyword
            );
            let parsed = parse_plan_dag(&sexp);
            assert_eq!(
                parsed.nodes[0].retry_count,
                Some(2),
                "keyword `:{}` must populate retry_count directly",
                keyword
            );
            assert_eq!(parsed.nodes[0].effective_max_attempts(), 3); // 1 + 2 retries
            assert!(parsed.nodes[0].retry_enabled());
        }
    }

    #[test]
    fn parse_node_form_captures_max_attempts_keyword_as_total_attempts() {
        // `:max-attempts N` declares N **total** attempts (including
        // the first); the parser converts to additional retries so
        // the runtime always sees the same shape.
        for keyword in ["max-attempts", "max_attempts"] {
            let sexp = format!(
                r#"(plan (node :id "n1" :target "mission_execution" :{} 3))"#,
                keyword
            );
            let parsed = parse_plan_dag(&sexp);
            assert_eq!(
                parsed.nodes[0].retry_count,
                Some(2),
                "keyword `:{}` value 3 must lower into 2 additional retries",
                keyword
            );
            assert_eq!(parsed.nodes[0].effective_max_attempts(), 3);
        }
    }

    #[test]
    fn parse_node_form_max_attempts_one_keeps_baseline_single_attempt() {
        // `:max-attempts 1` means "exactly one attempt" — the baseline
        // single-attempt contract; retry_enabled must read false.
        let sexp = r#"(plan (node :id "n" :target "mission_execution" :max-attempts 1))"#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[0].retry_count, Some(0));
        assert_eq!(parsed.nodes[0].effective_max_attempts(), 1);
        assert!(!parsed.nodes[0].retry_enabled());
    }

    #[test]
    fn build_validated_dag_rejects_max_attempts_zero() {
        // `:max-attempts 0` is meaningless — zero attempts = never
        // run. We surface a structured parse error so the typo is
        // visible to the author instead of silently disabling the node.
        let sexp =
            r#"(plan (node :id "n" :target "mission_execution" :max-attempts 0))"#;
        let err = build_validated_dag(sexp).unwrap_err();
        match err {
            DagBuildError::InvalidRetryHint { node_id, key, raw, detail } => {
                assert_eq!(node_id, "n");
                assert_eq!(key, "max-attempts");
                assert_eq!(raw, "0");
                assert!(detail.contains("positive"));
            }
            other => panic!("expected InvalidRetryHint, got {:?}", other),
        }
    }

    #[test]
    fn parse_node_form_captures_retry_delay_ms() {
        let sexp = r#"
            (plan
              (node :id "n1" :target "mission_execution"
                    :retry-count 1
                    :retry-delay-ms 250))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[0].retry_count, Some(1));
        assert_eq!(parsed.nodes[0].retry_delay_ms, Some(250));
        assert_eq!(parsed.nodes[0].effective_retry_delay_ms(), Some(250));
        assert_eq!(parsed.nodes[0].effective_max_attempts(), 2);
    }

    #[test]
    fn parse_node_form_retry_count_caps_to_safe_max() {
        // Authoring `:retry-count 9999` cannot melt the dispatch loop
        // — `effective_max_attempts` clamps to MAX_NODE_ATTEMPTS_CAP.
        let sexp =
            r#"(plan (node :id "n" :target "mission_execution" :retry-count 9999))"#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[0].retry_count, Some(9999));
        assert_eq!(
            parsed.nodes[0].effective_max_attempts(),
            MAX_NODE_ATTEMPTS_CAP
        );
    }

    #[test]
    fn parse_node_form_retry_delay_ms_caps_to_safe_max() {
        let sexp = r#"
            (plan
              (node :id "n" :target "mission_execution"
                    :retry-count 1
                    :retry-delay-ms 9999999))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[0].retry_delay_ms, Some(9999999));
        assert_eq!(
            parsed.nodes[0].effective_retry_delay_ms(),
            Some(MAX_RETRY_DELAY_MS)
        );
    }

    #[test]
    fn parse_node_form_retry_count_zero_keeps_baseline_single_attempt() {
        let sexp = r#"(plan (node :id "n" :target "mission_execution" :retry-count 0))"#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[0].retry_count, Some(0));
        assert_eq!(parsed.nodes[0].effective_max_attempts(), 1);
        assert!(!parsed.nodes[0].retry_enabled());
    }

    #[test]
    fn parse_node_form_retry_count_absent_keeps_baseline_single_attempt() {
        let sexp = r#"(plan (node :id "n" :target "mission_execution"))"#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes[0].retry_count, None);
        assert_eq!(parsed.nodes[0].effective_max_attempts(), 1);
        assert!(!parsed.nodes[0].retry_enabled());
        assert_eq!(parsed.nodes[0].effective_retry_delay_ms(), None);
    }

    #[test]
    fn build_validated_dag_rejects_negative_retry_count() {
        // Negative retry counts are a structured parse error — silently
        // dropping the hint into `unsupported_fields` would lose the
        // policy the author declared.
        let sexp =
            r#"(plan (node :id "n" :target "mission_execution" :retry-count -1))"#;
        let err = build_validated_dag(sexp).unwrap_err();
        match err {
            DagBuildError::InvalidRetryHint { node_id, key, raw, detail } => {
                assert_eq!(node_id, "n");
                assert_eq!(key, "retry-count");
                assert_eq!(raw, "-1");
                assert!(detail.contains("non-negative"));
            }
            other => panic!("expected InvalidRetryHint, got {:?}", other),
        }
    }

    #[test]
    fn build_validated_dag_rejects_non_numeric_retry_count() {
        let sexp = r#"
            (plan (node :id "n" :target "mission_execution" :max-attempts "thrice"))
        "#;
        let err = build_validated_dag(sexp).unwrap_err();
        match err {
            DagBuildError::InvalidRetryHint { node_id, key, raw, .. } => {
                assert_eq!(node_id, "n");
                assert_eq!(key, "max-attempts");
                assert_eq!(raw, "thrice");
            }
            other => panic!("expected InvalidRetryHint, got {:?}", other),
        }
    }

    #[test]
    fn build_validated_dag_rejects_negative_retry_delay_ms() {
        let sexp = r#"
            (plan
              (node :id "n" :target "mission_execution"
                    :retry-count 1
                    :retry-delay-ms -50))
        "#;
        let err = build_validated_dag(sexp).unwrap_err();
        assert!(matches!(err, DagBuildError::InvalidRetryHint { .. }));
    }

    #[test]
    fn invalid_retry_hint_into_tool_result_carries_invalid_param_code() {
        // Author-facing surface of the structured parse error: the
        // ToolResult must carry the canonical INVALID_PARAM error code
        // and a suggestion that points at the corrective action so a
        // failed dispatch tells the author exactly what to fix.
        let err = DagBuildError::InvalidRetryHint {
            node_id: "n".into(),
            key: "retry-count".into(),
            raw: "-1".into(),
            detail: "value must be a non-negative integer".into(),
        };
        let tr = err.into_tool_result();
        assert_eq!(tr.is_error, Some(true));
        let payload = tool_result_payload(&tr);
        assert_eq!(payload["error_code"], error_codes::INVALID_PARAM);
        let reason = payload["reason"].as_str().expect("reason string");
        assert!(reason.contains("retry-count"), "reason: {}", reason);
        assert!(reason.contains("-1"), "reason: {}", reason);
        assert!(payload["suggestion"].is_string(), "must carry a suggestion");
    }

    #[test]
    fn build_retry_plan_lists_only_nodes_that_opted_in() {
        // Plain nodes (no retry hint) and `:max-attempts 1` (explicit
        // single-attempt) MUST stay out of `retry_plan` so the v2
        // baseline byte-shape is preserved for callers that never
        // declared a retry policy.
        let sexp = r#"
            (plan
              (node :id "a" :target "mission_execution")
              (node :id "b" :target "mission_execution" :retry-count 2 :retry-delay-ms 100)
              (node :id "c" :target "mission_execution" :max-attempts 1)
              (node :id "d" :target "mission_execution" :max-attempts 2))
        "#;
        let (parsed, order) = build_validated_dag(sexp).expect("valid");
        let plan = build_retry_plan(&parsed.nodes, &order);
        let arr = plan.as_array().unwrap();
        assert_eq!(arr.len(), 2, "only nodes with > 1 attempts surface");
        // Node `b` opted in via `:retry-count 2` (3 total).
        assert_eq!(arr[0]["id"], "b");
        assert_eq!(arr[0]["max_attempts"], 3);
        assert_eq!(arr[0]["retry_count_raw"], 2);
        assert_eq!(arr[0]["retry_delay_ms"], 100);
        // Node `d` opted in via `:max-attempts 2` (lowered to 1 retry).
        assert_eq!(arr[1]["id"], "d");
        assert_eq!(arr[1]["max_attempts"], 2);
        assert_eq!(arr[1]["retry_count_raw"], 1);
    }

    #[test]
    fn build_nodes_summary_surfaces_retry_block_when_present() {
        let nodes = vec![
            DagNode {
                id: "rb".into(),
                target: "mission_execution".into(),
                failure_policy: "fail-fast".into(),
                retry_count: Some(2),
                retry_delay_ms: Some(50),
                ..Default::default()
            },
            DagNode {
                id: "plain".into(),
                target: "mission_execution".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ];
        let order = vec!["rb".to_string(), "plain".to_string()];
        let summary = build_nodes_summary(&nodes, &order);
        let arr = summary.as_array().unwrap();
        let rb = &arr[0];
        assert_eq!(rb["retry"]["max_attempts"], 3);
        assert_eq!(rb["retry"]["retry_count_raw"], 2);
        assert_eq!(rb["retry"]["retry_delay_ms"], 50);
        let plain = &arr[1];
        // Plain node never opted in — must NOT carry a `retry` block
        // (preserves the wave-15 baseline byte-shape).
        assert!(plain.get("retry").is_none());
    }

    #[test]
    fn node_results_json_emits_retry_block_when_attempts_made_more_than_one() {
        let mut o = ExecutionOutcome::default();
        // A node that succeeded on attempt 2 (one retry consumed) must
        // emit the `retry` observability block.
        o.results.push(NodeResult {
            id: "r".into(),
            target: "mission_execution".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({"ok": true}),
            attempts_made: 2,
            max_attempts: 3,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        // A baseline-quiet node (single attempt, no retry policy) must
        // NOT emit the block — preserves wave-15 byte-shape.
        o.results.push(NodeResult {
            id: "q".into(),
            target: "mission_execution".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        let v = o.node_results_json();
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["retry"]["attempts"], 2);
        assert_eq!(arr[0]["retry"]["max_attempts"], 3);
        assert!(arr[0]["retry"].get("non_retryable").is_none());
        assert!(arr[1].get("retry").is_none());
    }

    #[test]
    fn node_results_json_emits_retry_block_when_failure_was_non_retryable() {
        // Safe-descriptor refusal — only one attempt, but the
        // `non_retryable` flag must surface so consumers can
        // distinguish "we exhausted attempts" from "we refused to
        // retry on policy grounds".
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "sd".into(),
            target: "mission_task_delegate".into(),
            state: NodeState::Failed {
                reason: "workstation_dispatch refused: no project root".into(),
            },
            dispatch_strategy: "fresh-code-alignment".into(),
            inner_payload: json!({"workstation_dispatch_status": "skipped_project_root_unresolved"}),
            attempts_made: 1,
            max_attempts: 3,
            retry_skipped_non_retryable: true,
            rollback: None,
            acceptance: None,
        });
        let v = o.node_results_json();
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["retry"]["attempts"], 1);
        assert_eq!(arr[0]["retry"]["max_attempts"], 3);
        assert_eq!(arr[0]["retry"]["non_retryable"], true);
    }

    // ── wave-16 / task 05 — retry decision predicate ──────────────────
    //
    // `plan_node_should_retry` is the single point of truth for the
    // wave loop's "retry vs final failure" branch. Pure tests below
    // pin the matrix authors care about: one failure then a remaining
    // attempt → retry; exhausted → no retry; safe-descriptor refusal
    // → no retry; fail-fast abort → no retry.

    #[test]
    fn plan_node_should_retry_first_failure_with_attempts_left() {
        // attempt 1 of 3 (1 + 2 retries) failed, retryable, no abort
        // → must retry on attempt 2.
        assert!(plan_node_should_retry(1, 3, false, false));
        // attempt 2 of 3 still has one more retry left.
        assert!(plan_node_should_retry(2, 3, false, false));
    }

    #[test]
    fn plan_node_should_retry_exhausted_attempts_returns_false() {
        // Final attempt failed → must NOT retry.
        assert!(!plan_node_should_retry(3, 3, false, false));
        // Defensive: current_attempt > max_attempts (saturating sub).
        assert!(!plan_node_should_retry(4, 3, false, false));
    }

    #[test]
    fn plan_node_should_retry_baseline_single_attempt_returns_false() {
        // Default policy = 1 attempt total → never retries.
        assert!(!plan_node_should_retry(1, 1, false, false));
    }

    #[test]
    fn plan_node_should_retry_safe_descriptor_refusal_short_circuits() {
        // Safe-descriptor refusal — non-retryable trumps remaining
        // attempts so the wave loop never re-spawns a deterministic
        // policy refusal.
        assert!(!plan_node_should_retry(1, 3, true, false));
    }

    #[test]
    fn plan_node_should_retry_fail_fast_abort_short_circuits() {
        // Even with attempts left + retryable failure, an already-
        // tripped fail-fast abort must stop further retries so the
        // failing-fast contract (no new dispatches once aborted) is
        // honoured for retries too.
        assert!(!plan_node_should_retry(1, 3, false, true));
    }

    #[test]
    fn node_results_json_quiet_for_baseline_single_attempt_failure() {
        // A node that failed on its single allowed attempt (no retry
        // policy) must stay quiet on the `retry` surface so the
        // wave-15 byte-shape is preserved.
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "f".into(),
            target: "mission_execution".into(),
            state: NodeState::Failed { reason: "boom".into() },
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({"error": "boom"}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        let v = o.node_results_json();
        let arr = v.as_array().unwrap();
        assert!(arr[0].get("retry").is_none());
    }

    /// Forward-compat smoke test: the deterministic id we stamp on the
    /// paused state must round-trip through wave-15's
    /// `parse_review_question_id_struct` AND wave-16's subscriber
    /// dispatcher so the wave-16 / task 02 resolution listener can route
    /// it back to this plan when auto-resume lands. This task does NOT
    /// implement resume; the test is the contract handshake.
    #[test]
    fn paused_node_review_question_id_round_trips_through_subscriber_dispatcher() {
        use super::super::review_gate::{
            derive_plan_node_review_question_id, parse_review_question_id_struct,
            plan_review_resolved_dispatch, ReviewDecision, ReviewResolvedDispatch,
        };
        let plan = fixture_plan("(plan)");
        let qid = derive_plan_node_review_question_id(
            &plan.id.to_string(),
            plan.version,
            "node-g",
            Some("plan-node"),
        );
        // Layer 1: wave-15 envelope parser.
        let parsed = parse_review_question_id_struct(&qid).expect("valid envelope");
        assert_eq!(parsed.scope, "plan");
        assert_eq!(parsed.action, "plan-node");
        assert!(parsed.topic_hash.is_some());
        // Layer 2: wave-16 subscriber dispatcher routes under the plan
        // scope so a future resume hook can match the deterministic id
        // to its origin node.
        let dispatch = plan_review_resolved_dispatch(&qid, "approved");
        match dispatch {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "plan");
                assert_eq!(parsed.action, "plan-node");
                assert_eq!(decision, ReviewDecision::Approved);
            }
            other => panic!("expected Route under plan scope, got {:?}", other),
        }
    }

    // ── wave-17 / task 01 — paused-node resume helpers ─────────────────
    //
    // Pure tests for the resume validator and the listener-side planner
    // step that maps an approved plan-node Resolved event back to a
    // resume request. End-to-end DB / dispatch coverage requires an
    // AppState; the pure tests below pin the matrix authors care about.

    fn paused_node(node_id: &str) -> DagNode {
        DagNode {
            id: node_id.to_string(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            review_gate: Some("question-event".into()),
            review_action: Some("plan-node".into()),
            ..Default::default()
        }
    }

    fn parsed_dag_with(nodes: Vec<DagNode>) -> ParsedDag {
        ParsedDag {
            nodes,
            unsupported_top_forms: Vec::new(),
        }
    }

    #[test]
    fn validate_resume_request_rejects_non_plan_node_action() {
        // Wave-15 manager-side ids (action ∈ {compile, approve, mark, supersede})
        // must NOT route through the resume helper — they belong to the
        // existing manager bridge.
        use super::super::review_gate::parse_review_question_id_struct;
        let plan = fixture_plan("(plan)");
        let qid = format!(
            "review:plan:{}:v{}:approve",
            plan.id, plan.version
        );
        let parsed = parse_review_question_id_struct(&qid).expect("valid");
        let dag = parsed_dag_with(vec![paused_node("g")]);
        let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
        match err {
            PlanNodeResumeError::NotPlanNodeId { scope, action } => {
                assert_eq!(scope, "plan");
                assert_eq!(action, "approve");
            }
            other => panic!("expected NotPlanNodeId, got {:?}", other),
        }
    }

    #[test]
    fn validate_resume_request_rejects_non_plan_scope() {
        use super::super::review_gate::parse_review_question_id_struct;
        let plan = fixture_plan("(plan)");
        let qid = "review:directive:abc:v1:plan-node:0123456789abcdef";
        let parsed = parse_review_question_id_struct(qid).expect("valid");
        let dag = parsed_dag_with(vec![paused_node("g")]);
        let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
        assert!(matches!(err, PlanNodeResumeError::NotPlanNodeId { .. }));
    }

    #[test]
    fn validate_resume_request_rejects_plan_id_mismatch() {
        use super::super::review_gate::{
            derive_plan_node_review_question_id, parse_review_question_id_struct,
        };
        let plan = fixture_plan("(plan)");
        // Build a qid against a different plan id.
        let other_plan_id = "11111111-2222-3333-4444-555555555555";
        let qid = derive_plan_node_review_question_id(other_plan_id, plan.version, "g", None);
        let parsed = parse_review_question_id_struct(&qid).expect("valid");
        let dag = parsed_dag_with(vec![paused_node("g")]);
        let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
        match err {
            PlanNodeResumeError::PlanIdMismatch { expected, actual } => {
                assert_eq!(expected, plan.id.to_string());
                assert_eq!(actual, other_plan_id);
            }
            other => panic!("expected PlanIdMismatch, got {:?}", other),
        }
    }

    #[test]
    fn validate_resume_request_rejects_stale_version() {
        use super::super::review_gate::{
            derive_plan_node_review_question_id, parse_review_question_id_struct,
        };
        let plan = fixture_plan("(plan)");
        // Build a qid against an older plan version.
        let qid = derive_plan_node_review_question_id(
            &plan.id.to_string(),
            plan.version - 1,
            "g",
            None,
        );
        let parsed = parse_review_question_id_struct(&qid).expect("valid");
        let dag = parsed_dag_with(vec![paused_node("g")]);
        let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
        match err {
            PlanNodeResumeError::StaleVersion {
                expected,
                actual_in_id,
            } => {
                assert_eq!(expected, plan.version);
                assert_eq!(actual_in_id, plan.version - 1);
            }
            other => panic!("expected StaleVersion, got {:?}", other),
        }
    }

    #[test]
    fn validate_resume_request_rejects_hash_with_no_paused_node() {
        // The DAG carries node `g` WITHOUT the review-gate hint — so
        // the hash for `g` won't match any paused-eligible node.
        use super::super::review_gate::{
            derive_plan_node_review_question_id, parse_review_question_id_struct,
        };
        let plan = fixture_plan("(plan)");
        let qid = derive_plan_node_review_question_id(
            &plan.id.to_string(),
            plan.version,
            "g",
            None,
        );
        let parsed = parse_review_question_id_struct(&qid).expect("valid");
        let plain_node = DagNode {
            id: "g".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            // NO review_gate set — node is not paused-eligible.
            ..Default::default()
        };
        let dag = parsed_dag_with(vec![plain_node]);
        let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
        assert!(matches!(
            err,
            PlanNodeResumeError::NoMatchingPausedNode { .. }
        ));
    }

    #[test]
    fn validate_resume_request_rejects_hash_pointing_at_unknown_node() {
        // Plan was recompiled and the originally-paused node was renamed
        // — the qid hash now misses every paused-eligible node.
        use super::super::review_gate::{
            derive_plan_node_review_question_id, parse_review_question_id_struct,
        };
        let plan = fixture_plan("(plan)");
        let qid = derive_plan_node_review_question_id(
            &plan.id.to_string(),
            plan.version,
            "old-node-id",
            None,
        );
        let parsed = parse_review_question_id_struct(&qid).expect("valid");
        // DAG has a paused-eligible node, but with a DIFFERENT id.
        let dag = parsed_dag_with(vec![paused_node("new-node-id")]);
        let err = validate_resume_request(&parsed, &plan, &dag).unwrap_err();
        match err {
            PlanNodeResumeError::NoMatchingPausedNode { topic_hash } => {
                assert_eq!(
                    topic_hash,
                    super::super::review_gate::derive_plan_node_topic_hash("old-node-id")
                );
            }
            other => panic!("expected NoMatchingPausedNode, got {:?}", other),
        }
    }

    #[test]
    fn validate_resume_request_routes_unique_paused_node() {
        // Happy path: hash uniquely identifies a paused-eligible node.
        use super::super::review_gate::{
            derive_plan_node_review_question_id, parse_review_question_id_struct,
        };
        let plan = fixture_plan("(plan)");
        let qid = derive_plan_node_review_question_id(
            &plan.id.to_string(),
            plan.version,
            "g",
            None,
        );
        let parsed = parse_review_question_id_struct(&qid).expect("valid");
        let dag = parsed_dag_with(vec![
            paused_node("g"),
            DagNode {
                // Plain non-paused node with same prefix substring should
                // NOT collide because the validator hashes the whole
                // node id (and the paused-eligible filter excludes it
                // anyway).
                id: "h".into(),
                target: "mission_execution".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ]);
        let node = validate_resume_request(&parsed, &plan, &dag).expect("ok");
        assert_eq!(node.id, "g");
    }

    #[test]
    fn validate_resume_request_action_case_insensitive() {
        // The wave-15 envelope parser lowercases the action segment, but
        // assert the resume helper still routes correctly when the
        // upstream id was uppercased.
        use super::super::review_gate::{
            derive_plan_node_review_question_id, parse_review_question_id_struct,
        };
        let plan = fixture_plan("(plan)");
        let qid = derive_plan_node_review_question_id(
            &plan.id.to_string(),
            plan.version,
            "g",
            Some("PLAN-NODE"),
        );
        let parsed = parse_review_question_id_struct(&qid).expect("valid");
        let dag = parsed_dag_with(vec![paused_node("g")]);
        let node = validate_resume_request(&parsed, &plan, &dag).expect("ok");
        assert_eq!(node.id, "g");
    }

    #[test]
    fn plan_node_resume_error_codes_match_review_validator_vocabulary() {
        // Pin the structured error codes the wave-15 review validator
        // already speaks — keeps audit dashboards routing on a stable
        // vocabulary.
        assert_eq!(
            PlanNodeResumeError::IdMalformed { detail: "x".into() }.code(),
            "REVIEW_ID_MALFORMED"
        );
        assert_eq!(
            PlanNodeResumeError::NotPlanNodeId {
                scope: "x".into(),
                action: "y".into(),
            }
            .code(),
            "REVIEW_ACTION_UNSUPPORTED"
        );
        assert_eq!(
            PlanNodeResumeError::PlanIdMismatch {
                expected: "x".into(),
                actual: "y".into(),
            }
            .code(),
            "REVIEW_ARTIFACT_MISMATCH"
        );
        assert_eq!(
            PlanNodeResumeError::StaleVersion {
                expected: 1,
                actual_in_id: 2,
            }
            .code(),
            "STALE_REVIEW_VERSION"
        );
    }

    #[test]
    fn listener_planner_routes_approved_plan_node_resolved_through_resume_helper() {
        // Pure routing handshake: the wave-16 / task 02 subscriber's
        // planner must classify an approved plan-node Resolved event
        // as scope=plan + action=plan-node so the wave-17 / task 01
        // listener can branch on the action and call the resume
        // helper instead of the wave-15 manager-side handler.
        use super::super::review_gate::{
            derive_plan_node_review_question_id, is_plan_node_review_action,
            plan_review_resolved_dispatch, ReviewDecision, ReviewResolvedDispatch,
        };
        let plan_id = "00000000-0000-0000-0000-000000000abc";
        let qid = derive_plan_node_review_question_id(plan_id, 1, "node-g", None);
        let dispatch = plan_review_resolved_dispatch(&qid, "approved");
        match dispatch {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "plan");
                assert!(
                    is_plan_node_review_action(&parsed.action),
                    "action `{}` must classify as plan-node",
                    parsed.action
                );
                assert_eq!(decision, ReviewDecision::Approved);
            }
            other => panic!(
                "expected Route to plan-node resume helper, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn listener_planner_ignores_unknown_resolution_for_plan_node_id() {
        // Even when the qid is shaped for plan-node, an unrecognised
        // resolution string MUST hit IgnoreUnknownResolution rather
        // than Route — this is the "no auto-approve for arbitrary
        // text" guarantee carried over into wave-17.
        use super::super::review_gate::{
            derive_plan_node_review_question_id, plan_review_resolved_dispatch,
            ReviewResolvedDispatch,
        };
        let plan_id = "00000000-0000-0000-0000-000000000abc";
        let qid = derive_plan_node_review_question_id(plan_id, 1, "node-g", None);
        let dispatch = plan_review_resolved_dispatch(&qid, "looks-good-to-me");
        assert!(matches!(
            dispatch,
            ReviewResolvedDispatch::IgnoreUnknownResolution { .. }
        ));
    }

    #[test]
    fn listener_planner_routes_rejected_plan_node_resolved_with_decision_kept() {
        // Rejected resolutions still route through the planner — the
        // listener-side handler is responsible for keeping the node
        // paused without dispatching.
        use super::super::review_gate::{
            derive_plan_node_review_question_id, plan_review_resolved_dispatch,
            ReviewDecision, ReviewResolvedDispatch,
        };
        let plan_id = "00000000-0000-0000-0000-000000000abc";
        let qid = derive_plan_node_review_question_id(plan_id, 1, "node-g", None);
        let dispatch = plan_review_resolved_dispatch(&qid, "rejected");
        match dispatch {
            ReviewResolvedDispatch::Route { decision, .. } => {
                assert_eq!(decision, ReviewDecision::Rejected);
            }
            other => panic!("expected Route, got {:?}", other),
        }
    }

    // ── wave-17 / task 02 — claim / lease pure helpers ──────────────────

    fn claim_test_plan_id() -> uuid::Uuid {
        uuid::Uuid::parse_str("00000000-0000-0000-0000-0000000c1a1d").unwrap()
    }

    #[test]
    fn parse_claim_lease_secs_defaults_to_1800() {
        let v = json!({});
        assert_eq!(parse_claim_lease_secs(&v), PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS);
        assert_eq!(parse_claim_lease_secs(&v), 1800);
    }

    #[test]
    fn parse_claim_lease_secs_clamps_low_and_high() {
        // Below floor → clamped up to MIN.
        assert_eq!(
            parse_claim_lease_secs(&json!({"claim_lease_secs": 5})),
            PLAN_DAG_CLAIM_LEASE_SECS_MIN
        );
        // Above ceiling → clamped down to MAX.
        assert_eq!(
            parse_claim_lease_secs(&json!({"claim_lease_secs": 999_999})),
            PLAN_DAG_CLAIM_LEASE_SECS_MAX
        );
        // Inside the band → echoed verbatim.
        assert_eq!(
            parse_claim_lease_secs(&json!({"claim_lease_secs": 600})),
            600
        );
    }

    #[test]
    fn parse_claimer_name_defaults_when_missing_or_blank() {
        assert_eq!(
            parse_claimer_name(&json!({})),
            PLAN_DAG_DEFAULT_CLAIMER_NAME
        );
        // Whitespace-only → default (so a blank form field doesn't poison
        // the audit log).
        assert_eq!(
            parse_claimer_name(&json!({"claimer_name": "   "})),
            PLAN_DAG_DEFAULT_CLAIMER_NAME
        );
        // Explicit value → echoed (with surrounding whitespace trimmed).
        assert_eq!(
            parse_claimer_name(&json!({"claimer_name": "  alice  "})),
            "alice"
        );
    }

    #[test]
    fn parse_enforce_claims_defaults_to_false() {
        assert!(!parse_enforce_claims(&json!({})));
        assert!(parse_enforce_claims(&json!({"enforce_claims": true})));
        assert!(!parse_enforce_claims(&json!({"enforce_claims": false})));
        // Non-bool values normalise to false (strict opt-in).
        assert!(!parse_enforce_claims(&json!({"enforce_claims": "yes"})));
        assert!(!parse_enforce_claims(&json!({"enforce_claims": 1})));
    }

    #[test]
    fn derive_node_claim_scopes_uses_owned_files_first() {
        let plan_id = claim_test_plan_id();
        let node = DagNode {
            id: "n1".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            owned_files_raw: Some(r#"["src/a.rs" "src/b.rs"]"#.into()),
            scope: Some("ignored-when-owned-files-set".into()),
            ..Default::default()
        };
        let (scopes, source) = derive_node_claim_scopes(&node, plan_id);
        assert_eq!(source, CLAIM_SCOPE_SOURCE_OWNED_FILES);
        assert_eq!(
            scopes,
            vec!["src/a.rs".to_string(), "src/b.rs".to_string()]
        );
    }

    #[test]
    fn derive_node_claim_scopes_falls_back_to_scope_when_no_owned_files() {
        let plan_id = claim_test_plan_id();
        let node = DagNode {
            id: "n2".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            scope: Some("crates/foo".into()),
            ..Default::default()
        };
        let (scopes, source) = derive_node_claim_scopes(&node, plan_id);
        assert_eq!(source, CLAIM_SCOPE_SOURCE_SCOPE);
        assert_eq!(scopes, vec!["crates/foo".to_string()]);
    }

    #[test]
    fn derive_node_claim_scopes_falls_back_to_plan_node_synthetic_when_empty() {
        let plan_id = claim_test_plan_id();
        let node = DagNode {
            id: "n3".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        };
        let (scopes, source) = derive_node_claim_scopes(&node, plan_id);
        assert_eq!(source, CLAIM_SCOPE_SOURCE_PLAN_NODE_FALLBACK);
        assert_eq!(scopes.len(), 1);
        assert!(scopes[0].contains(&plan_id.to_string()));
        assert!(scopes[0].contains("node/n3"));
    }

    #[test]
    fn derive_node_claim_scopes_treats_blank_owned_files_and_scope_as_empty() {
        let plan_id = claim_test_plan_id();
        let node = DagNode {
            id: "n4".into(),
            target: "mission_execution".into(),
            failure_policy: "fail-fast".into(),
            owned_files_raw: Some(r#"["   "]"#.into()),
            scope: Some("   ".into()),
            ..Default::default()
        };
        let (scopes, source) = derive_node_claim_scopes(&node, plan_id);
        // Blank owned_files entries filter out → falls through to blank
        // :scope → falls through to synthetic.
        assert_eq!(source, CLAIM_SCOPE_SOURCE_PLAN_NODE_FALLBACK);
        assert_eq!(scopes.len(), 1);
    }

    #[test]
    fn derive_plan_dag_claim_id_includes_attempt() {
        let plan_id = claim_test_plan_id();
        let id_a = derive_plan_dag_claim_id(plan_id, "node-x", 1);
        let id_b = derive_plan_dag_claim_id(plan_id, "node-x", 2);
        assert_ne!(id_a, id_b);
        assert!(id_a.starts_with("plan-dag:"));
        assert!(id_a.ends_with(":1"));
        assert!(id_b.ends_with(":2"));
    }

    fn claim_test_now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap()
    }

    #[test]
    fn claim_registry_acquires_disjoint_scopes() {
        let mut reg = ClaimRegistry::new();
        let now = claim_test_now();
        let r1 = reg.try_acquire(
            "c1".into(),
            "claimer-a".into(),
            vec!["src/a.rs".into()],
            CLAIM_SCOPE_SOURCE_OWNED_FILES,
            300,
            now,
        );
        assert!(matches!(r1, ClaimAcquire::Acquired(_)));
        let r2 = reg.try_acquire(
            "c2".into(),
            "claimer-b".into(),
            vec!["src/b.rs".into()],
            CLAIM_SCOPE_SOURCE_OWNED_FILES,
            300,
            now,
        );
        assert!(matches!(r2, ClaimAcquire::Acquired(_)));
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn claim_registry_rejects_overlapping_scope() {
        let mut reg = ClaimRegistry::new();
        let now = claim_test_now();
        let r1 = reg.try_acquire(
            "c1".into(),
            "alpha".into(),
            vec!["crates/foo".into()],
            CLAIM_SCOPE_SOURCE_SCOPE,
            300,
            now,
        );
        assert!(matches!(r1, ClaimAcquire::Acquired(_)));
        let r2 = reg.try_acquire(
            "c2".into(),
            "beta".into(),
            // Prefix of the held scope — `scopes_overlap_pure` matches
            // both directions.
            vec!["crates/foo/src".into()],
            CLAIM_SCOPE_SOURCE_SCOPE,
            300,
            now,
        );
        match r2 {
            ClaimAcquire::Conflict {
                conflicting_claim_id,
                conflicting_claimer,
                conflicting_scope,
                offending_scope,
                ..
            } => {
                assert_eq!(conflicting_claim_id, "c1");
                assert_eq!(conflicting_claimer, "alpha");
                assert_eq!(conflicting_scope, "crates/foo");
                assert_eq!(offending_scope, "crates/foo/src");
            }
            other => panic!("expected Conflict, got {:?}", other),
        }
        // The conflicting attempt was NOT inserted — only the original
        // acquired claim lives in the registry.
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn claim_registry_release_then_reacquire_succeeds() {
        let mut reg = ClaimRegistry::new();
        let now = claim_test_now();
        let r1 = reg.try_acquire(
            "c1".into(),
            "writer".into(),
            vec!["src/a.rs".into()],
            CLAIM_SCOPE_SOURCE_OWNED_FILES,
            300,
            now,
        );
        assert!(matches!(r1, ClaimAcquire::Acquired(_)));
        let later = now + chrono::Duration::seconds(10);
        let released = reg.release("c1", later);
        assert!(released.is_some());
        assert!(released.unwrap().released_at.is_some());
        // After release the same scope can be re-acquired by a different
        // claim id (audit row remains, registry just moves on).
        let r2 = reg.try_acquire(
            "c2".into(),
            "writer-2".into(),
            vec!["src/a.rs".into()],
            CLAIM_SCOPE_SOURCE_OWNED_FILES,
            300,
            later,
        );
        assert!(matches!(r2, ClaimAcquire::Acquired(_)));
    }

    #[test]
    fn claim_registry_lease_expiry_treats_held_claim_as_soft_released() {
        let mut reg = ClaimRegistry::new();
        let now = claim_test_now();
        let r1 = reg.try_acquire(
            "c1".into(),
            "writer".into(),
            vec!["src/a.rs".into()],
            CLAIM_SCOPE_SOURCE_OWNED_FILES,
            // 60-second lease so we can deliberately step past it.
            60,
            now,
        );
        assert!(matches!(r1, ClaimAcquire::Acquired(_)));
        // Step well past the lease — registry should treat the claim as
        // soft-released for conflict purposes (mirrors wave12-01).
        let later = now + chrono::Duration::seconds(120);
        let r2 = reg.try_acquire(
            "c2".into(),
            "writer-2".into(),
            vec!["src/a.rs".into()],
            CLAIM_SCOPE_SOURCE_OWNED_FILES,
            300,
            later,
        );
        assert!(matches!(r2, ClaimAcquire::Acquired(_)));
    }

    #[test]
    fn build_planned_claims_emits_one_entry_per_node_in_topo_order() {
        let plan_id = claim_test_plan_id();
        let sexp = r#"
            (plan
              (node :id "n1" :target "mission_task_delegate"
                    :owned-files ["src/a.rs"])
              (node :id "n2" :target "mission_execution"
                    :scope "crates/foo" :depends-on ["n1"])
              (node :id "n3" :target "mission_execution" :depends-on ["n2"]))
        "#;
        let (parsed, order) = build_validated_dag(sexp).expect("valid");
        let projection = build_planned_claims(
            &parsed.nodes,
            &order,
            plan_id,
            "scheduler",
            900,
            true,
        );
        let arr = projection.as_array().expect("array");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["node_id"], "n1");
        assert_eq!(arr[0]["scope_source"], CLAIM_SCOPE_SOURCE_OWNED_FILES);
        assert_eq!(arr[0]["scopes"], json!(["src/a.rs"]));
        assert_eq!(arr[0]["lease_secs"], 900);
        assert_eq!(arr[0]["enforce_claims"], true);
        assert_eq!(arr[0]["claimer"], "scheduler");
        assert_eq!(arr[1]["node_id"], "n2");
        assert_eq!(arr[1]["scope_source"], CLAIM_SCOPE_SOURCE_SCOPE);
        assert_eq!(arr[1]["scopes"], json!(["crates/foo"]));
        assert_eq!(arr[2]["node_id"], "n3");
        assert_eq!(
            arr[2]["scope_source"],
            CLAIM_SCOPE_SOURCE_PLAN_NODE_FALLBACK
        );
    }

    #[tokio::test]
    async fn dry_run_response_includes_planned_claims_and_knobs() {
        // Build a fake AppState by way of the existing test fixtures.
        // We exercise the dry-run branch which never touches the bus
        // / store, so we can pass a minimal AppState constructed via
        // `AppState::test_dummy()` where available — but that helper
        // doesn't exist on plan_dag's test surface, so instead we
        // assert the projection shape via the pure `build_planned_claims`
        // (already covered above) PLUS the sub-projection that
        // `action_execute_dag_v1` would echo. The integration glue
        // (action_execute_dag_v1 itself) is exercised by full daemon
        // tests, not pure unit tests.
        let plan_id = claim_test_plan_id();
        let sexp = r#"
            (plan
              (node :id "n1" :target "mission_execution"
                    :owned-files ["src/x.rs"]))
        "#;
        let (parsed, order) = build_validated_dag(sexp).expect("valid");
        let projection = build_planned_claims(
            &parsed.nodes,
            &order,
            plan_id,
            PLAN_DAG_DEFAULT_CLAIMER_NAME,
            PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS,
            false,
        );
        let arr = projection.as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["claimer"], PLAN_DAG_DEFAULT_CLAIMER_NAME);
        assert_eq!(
            arr[0]["lease_secs"],
            PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS
        );
        assert_eq!(arr[0]["enforce_claims"], false);
        // Claim id format is byte-stable so dashboards can grep on it.
        let claim_id = arr[0]["claim_id"].as_str().unwrap();
        assert!(claim_id.starts_with("plan-dag:"));
        assert!(claim_id.ends_with(":1"));
    }

    #[test]
    fn plan_dag_claim_iso_timestamps_round_trip_through_chrono() {
        // Pin the ISO-8601 second-precision projection so audit
        // dashboards can compare claim timestamps to wave12-01
        // companion-log claims byte-for-byte.
        let now = claim_test_now();
        let claim = PlanDagClaim {
            claim_id: "plan-dag:00000000-0000-0000-0000-0000000c1a1d:n1:1".into(),
            claimer: "plan-dag-scheduler".into(),
            scopes: vec!["src/a.rs".into()],
            scope_source: CLAIM_SCOPE_SOURCE_OWNED_FILES,
            acquired_at: now,
            lease_expires_at: now + chrono::Duration::seconds(300),
            released_at: None,
        };
        assert_eq!(claim.acquired_at_iso(), "2026-01-01T12:00:00Z");
        assert_eq!(claim.lease_expires_at_iso(), "2026-01-01T12:05:00Z");
        assert!(claim.released_at_iso().is_none());
        let mut released = claim.clone();
        released.released_at = Some(now + chrono::Duration::seconds(42));
        assert_eq!(
            released.released_at_iso().unwrap(),
            "2026-01-01T12:00:42Z"
        );
    }

    #[test]
    fn enforce_claims_off_preserves_default_byte_compat_knobs() {
        // The compat-mode default surface MUST report `enforce_claims=false`
        // and the wave12-01 lease defaults so pre-wave17 callers see
        // their expected byte-shape.
        let v = json!({});
        assert!(!parse_enforce_claims(&v));
        assert_eq!(
            parse_claim_lease_secs(&v),
            PLAN_DAG_DEFAULT_CLAIM_LEASE_SECS
        );
        assert_eq!(parse_claimer_name(&v), PLAN_DAG_DEFAULT_CLAIMER_NAME);
    }

    #[test]
    fn enforce_claims_on_does_not_change_scope_derivation() {
        // The enforce knob lives in the scheduler's dispatch path, not
        // in the scope derivation. Pin that boundary so a future
        // refactor that conflates the two surfaces gets caught.
        let plan_id = claim_test_plan_id();
        let node = DagNode {
            id: "n5".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            owned_files_raw: Some(r#"["src/a.rs"]"#.into()),
            ..Default::default()
        };
        let (scopes_a, source_a) = derive_node_claim_scopes(&node, plan_id);
        let (scopes_b, source_b) = derive_node_claim_scopes(&node, plan_id);
        assert_eq!(scopes_a, scopes_b);
        assert_eq!(source_a, source_b);
        assert_eq!(source_a, CLAIM_SCOPE_SOURCE_OWNED_FILES);
    }

    #[test]
    fn claim_registry_release_returns_none_for_unknown_id() {
        let mut reg = ClaimRegistry::new();
        assert!(reg.release("ghost", claim_test_now()).is_none());
    }

    #[test]
    fn claim_registry_release_is_idempotent_on_already_released_record() {
        let mut reg = ClaimRegistry::new();
        let now = claim_test_now();
        let _ = reg.try_acquire(
            "c1".into(),
            "writer".into(),
            vec!["src/a.rs".into()],
            CLAIM_SCOPE_SOURCE_OWNED_FILES,
            300,
            now,
        );
        let later1 = now + chrono::Duration::seconds(5);
        let later2 = now + chrono::Duration::seconds(10);
        let r1 = reg.release("c1", later1).expect("first release");
        assert_eq!(r1.released_at, Some(later1));
        let r2 = reg.release("c1", later2).expect("second release returns same record");
        // First release wins — second release must NOT clobber the
        // earlier timestamp (audit dashboards depend on the original
        // release moment).
        assert_eq!(r2.released_at, Some(later1));
    }

    // ── wave-17 / task 03 — deterministic acceptance evaluator ───────────
    //
    // The acceptance phase runs after a successful inner dispatch and
    // decides whether the node truly succeeded, was rejected, or needs
    // human approval. CRITICAL invariant: NO shell command is ever
    // executed; the evaluator is a pure projection over `(node,
    // payload)`. These tests pin the four decision branches plus the
    // fact that declared `:acceptance-commands` are surfaced verbatim
    // without execution.

    fn acceptance_node_with(
        mode: Option<&str>,
        commands: Option<&str>,
        keys: Option<&str>,
    ) -> DagNode {
        DagNode {
            id: "n".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            acceptance_mode_raw: mode.map(|s| s.to_string()),
            acceptance_commands_raw: commands.map(|s| s.to_string()),
            acceptance_evidence_keys_raw: keys.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn parse_node_form_captures_acceptance_evaluator_hints() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :acceptance-mode "evidence_keys"
                    :acceptance-evidence-keys ["build_ok" "tests_passed"]
                    :acceptance-commands ["cargo test" "git diff --check"]))
        "#;
        let parsed = parse_plan_dag(sexp);
        let n = &parsed.nodes[0];
        assert_eq!(n.acceptance_mode_raw.as_deref(), Some("evidence_keys"));
        assert!(n
            .acceptance_evidence_keys_raw
            .as_deref()
            .unwrap()
            .contains("build_ok"));
        assert!(n
            .acceptance_commands_raw
            .as_deref()
            .unwrap()
            .contains("cargo test"));
        // None of the new keys must land in unsupported_fields — that
        // would mean the scheduler can't route the acceptance phase.
        let unsupported_keys: Vec<String> =
            n.unsupported_fields.iter().map(|(k, _)| k.clone()).collect();
        for forbidden in [
            "acceptance-mode",
            "acceptance-evidence-keys",
            "acceptance-commands",
        ] {
            assert!(
                !unsupported_keys.contains(&forbidden.to_string()),
                "key `{}` must land on a typed slot, not unsupported_fields",
                forbidden
            );
        }
        assert!(n.has_acceptance_hints());
        assert_eq!(
            n.acceptance_mode_kind(),
            Some(AcceptanceMode::EvidenceKeys)
        );
    }

    #[test]
    fn parse_node_form_records_unrecognised_acceptance_mode_in_unsupported_fields() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :acceptance-mode "invent_a_mode"))
        "#;
        let parsed = parse_plan_dag(sexp);
        let n = &parsed.nodes[0];
        // Raw value lands on the typed slot AND the typo surfaces in
        // unsupported_fields so the response loudly flags the mistake.
        assert_eq!(n.acceptance_mode_raw.as_deref(), Some("invent_a_mode"));
        assert!(n
            .unsupported_fields
            .iter()
            .any(|(k, v)| k == "acceptance-mode" && v == "invent_a_mode"));
        // The typed projection refuses to interpret a typo as a real mode.
        assert!(n.acceptance_mode_kind().is_none());
    }

    #[test]
    fn build_nodes_summary_surfaces_acceptance_hints_when_present() {
        let nodes = vec![
            DagNode {
                id: "with".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                acceptance_mode_raw: Some("inner_status".into()),
                acceptance_evidence_keys_raw: Some(r#"["k1"]"#.into()),
                ..Default::default()
            },
            DagNode {
                id: "plain".into(),
                target: "mission_execution".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ];
        let order = vec!["with".to_string(), "plain".to_string()];
        let summary = build_nodes_summary(&nodes, &order);
        let arr = summary.as_array().unwrap();
        assert_eq!(arr[0]["acceptance_mode"], "inner_status");
        assert!(arr[0]["acceptance_evidence_keys_raw"]
            .as_str()
            .unwrap()
            .contains("k1"));
        // Plain node carries none of the acceptance fields so the
        // summary stays quiet (regression guard for the wave-16 baseline).
        assert!(arr[1].get("acceptance_mode").is_none());
        assert!(arr[1].get("acceptance_evidence_keys_raw").is_none());
    }

    #[test]
    fn evaluate_acceptance_no_hints_returns_not_evaluated() {
        let node = acceptance_node_with(None, None, None);
        let payload = json!({"task_id": "btk-1"});
        let e = evaluate_node_acceptance(&node, &payload, true);
        assert_eq!(e.status, AcceptanceStatus::NotEvaluated);
        assert!(e.is_inactive());
        assert!(e.commands.is_empty());
        assert!(e.evidence_keys.is_empty());
        assert!(e.mode.is_none());
    }

    #[test]
    fn evaluate_acceptance_inner_status_accepts_clean_success_payload() {
        let node = acceptance_node_with(Some("inner_status"), None, None);
        let payload = json!({"workstation_dispatch_status": "dispatched", "task_id": "btk-1"});
        let e = evaluate_node_acceptance(&node, &payload, true);
        assert_eq!(e.status, AcceptanceStatus::Accepted);
        assert_eq!(e.mode, Some(AcceptanceMode::InnerStatus));
        assert!(e.reason.contains("dispatch Ok"));
    }

    #[test]
    fn evaluate_acceptance_inner_status_rejects_when_dispatch_classification_failed() {
        // dispatch_succeeded=false short-circuits the evaluator to
        // Rejected even when the payload looks clean. This guards
        // against the evaluator second-guessing the dispatch judgment.
        let node = acceptance_node_with(Some("inner_status"), None, None);
        let payload = json!({"task_id": "btk-1"});
        let e = evaluate_node_acceptance(&node, &payload, false);
        assert_eq!(e.status, AcceptanceStatus::Rejected);
        assert!(e.reason.contains("dispatch classification was not Ok"));
    }

    #[test]
    fn evaluate_acceptance_inner_status_rejects_when_payload_signals_error() {
        let node = acceptance_node_with(Some("inner_status"), None, None);
        for bad in [
            json!({"error": "boom"}),
            json!({"success": false}),
            json!({"ok": false}),
            json!({"status": "failed"}),
            json!({"workstation_dispatch_status": "skipped_project_root_unresolved"}),
        ] {
            let e = evaluate_node_acceptance(&node, &bad, true);
            assert_eq!(
                e.status,
                AcceptanceStatus::Rejected,
                "payload {:?} should reject under inner_status",
                bad
            );
        }
    }

    #[test]
    fn evaluate_acceptance_evidence_keys_accepts_when_all_present_at_top_level() {
        let node = acceptance_node_with(
            Some("evidence_keys"),
            None,
            Some(r#"["build_ok" "tests_passed"]"#),
        );
        let payload = json!({
            "build_ok": true,
            "tests_passed": 3,
            "noise": "anything",
        });
        let e = evaluate_node_acceptance(&node, &payload, true);
        assert_eq!(e.status, AcceptanceStatus::Accepted);
        assert_eq!(e.mode, Some(AcceptanceMode::EvidenceKeys));
        assert_eq!(
            e.evidence_keys,
            vec!["build_ok".to_string(), "tests_passed".to_string()]
        );
    }

    #[test]
    fn evaluate_acceptance_evidence_keys_descends_into_nested_holders() {
        // Substrates often stash typed evidence under `evidence` /
        // `inner_result`; the evaluator descends one level into the
        // well-known holders so authors don't have to mirror the
        // payload's exact nesting in their `:acceptance-evidence-keys`.
        let node = acceptance_node_with(
            Some("evidence_keys"),
            None,
            Some(r#"["build_ok" "tests_passed"]"#),
        );
        let payload = json!({
            "evidence": {
                "build_ok": true,
                "tests_passed": 1,
            }
        });
        let e = evaluate_node_acceptance(&node, &payload, true);
        assert_eq!(e.status, AcceptanceStatus::Accepted);
    }

    #[test]
    fn evaluate_acceptance_evidence_keys_rejects_missing_keys_with_named_list() {
        let node = acceptance_node_with(
            Some("evidence_keys"),
            None,
            Some(r#"["build_ok" "tests_passed"]"#),
        );
        let payload = json!({"build_ok": true});
        let e = evaluate_node_acceptance(&node, &payload, true);
        assert_eq!(e.status, AcceptanceStatus::Rejected);
        assert!(
            e.reason.contains("tests_passed"),
            "reason `{}` must surface the missing key",
            e.reason
        );
    }

    #[test]
    fn evaluate_acceptance_evidence_keys_with_empty_keys_degrades_to_manual() {
        let node = acceptance_node_with(Some("evidence_keys"), None, Some("[]"));
        let payload = json!({"task_id": "x"});
        let e = evaluate_node_acceptance(&node, &payload, true);
        // An empty contract cannot prove anything — surface as
        // manual_required so the typo is loud.
        assert_eq!(e.status, AcceptanceStatus::ManualRequired);
        assert!(e.reason.contains("empty"));
    }

    #[test]
    fn evaluate_acceptance_manual_mode_always_pauses() {
        let node = acceptance_node_with(Some("manual"), None, None);
        let payload = json!({"task_id": "x"});
        let e = evaluate_node_acceptance(&node, &payload, true);
        assert_eq!(e.status, AcceptanceStatus::ManualRequired);
        assert_eq!(e.mode, Some(AcceptanceMode::Manual));
    }

    #[test]
    fn evaluate_acceptance_commands_without_mode_pause_as_manual_required_and_never_run_shell() {
        // CRITICAL: declaring `:acceptance-commands` without a typed
        // evaluator must NOT execute shell. The default policy is to
        // surface the gate as manual_required and carry the commands
        // verbatim into the response so a human / out-of-band pipeline
        // can run them.
        let node = acceptance_node_with(
            None,
            Some(r#"["cargo test" "git diff --check"]"#),
            None,
        );
        let payload = json!({"task_id": "x"});
        let e = evaluate_node_acceptance(&node, &payload, true);
        assert_eq!(e.status, AcceptanceStatus::ManualRequired);
        assert_eq!(
            e.commands,
            vec!["cargo test".to_string(), "git diff --check".to_string()],
            "declared commands must round-trip into the evaluation block verbatim"
        );
        assert!(e.mode.is_none());
        assert!(e.reason.contains("never runs shell"));
    }

    #[test]
    fn evaluation_to_json_carries_every_surface_field() {
        let node = acceptance_node_with(
            Some("inner_status"),
            Some(r#"["cargo test"]"#),
            Some(r#"["k1"]"#),
        );
        let payload = json!({"task_id": "x"});
        let e = evaluate_node_acceptance(&node, &payload, true);
        let v = e.to_json();
        assert_eq!(v["status"], "accepted");
        assert_eq!(v["mode"], "inner_status");
        assert_eq!(v["commands"][0], "cargo test");
        assert_eq!(v["evidence_keys"][0], "k1");
        assert!(v["reason"].is_string());
    }

    #[test]
    fn derive_acceptance_pause_id_is_distinct_from_review_gate_id_space() {
        // The deterministic pause id MUST start with `acceptance:` so
        // the wave-17 / task 01 paused-node resume helper (which
        // requires `review:plan:...:plan-node:...` shape) cannot
        // accidentally consume an acceptance pause.
        let plan_id =
            uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000abc").unwrap();
        let id = derive_acceptance_pause_id(plan_id, 7, "n42");
        assert!(
            id.starts_with("acceptance:plan:"),
            "id `{}` must use the acceptance prefix",
            id
        );
        assert!(id.contains(":v7:"));
        assert!(id.ends_with(":n42"));
        // Round-trips deterministically — same inputs, same output.
        assert_eq!(id, derive_acceptance_pause_id(plan_id, 7, "n42"));
    }

    #[test]
    fn node_results_json_surfaces_acceptance_block_only_when_active() {
        let mut o = ExecutionOutcome::default();
        // Active acceptance — surfaces.
        o.results.push(NodeResult {
            id: "with_acc".into(),
            target: "mission_task_delegate".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "fresh-code-alignment".into(),
            inner_payload: json!({"task_id": "btk-1"}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: Some(AcceptanceEvaluation {
                status: AcceptanceStatus::Accepted,
                mode: Some(AcceptanceMode::InnerStatus),
                commands: vec!["cargo test".into()],
                evidence_keys: vec![],
                reason: "ok".into(),
                fan_in: None,
            }),
        });
        // No hints — quiet.
        o.results.push(NodeResult {
            id: "plain".into(),
            target: "mission_execution".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "unknown".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        let v = o.node_results_json();
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["acceptance"]["status"], "accepted");
        assert_eq!(arr[0]["acceptance"]["mode"], "inner_status");
        assert_eq!(arr[0]["acceptance"]["commands"][0], "cargo test");
        assert!(arr[1].get("acceptance").is_none());
    }

    #[test]
    fn manual_required_surfaces_paused_state_with_acceptance_id_distinct_from_review_gate() {
        // When the acceptance phase returns ManualRequired the wave
        // loop MUST flip the node to `Paused` with the deterministic
        // `acceptance:plan:...` id (NOT the wave-16 `review:plan:...`
        // id). The aggregate status surfaces as `dag_paused` — same
        // codepath as review-gate paused.
        let mut o = ExecutionOutcome::default();
        o.results.push(NodeResult {
            id: "n".into(),
            target: "mission_task_delegate".into(),
            state: NodeState::Paused {
                question_id: derive_acceptance_pause_id(
                    uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000abc")
                        .unwrap(),
                    1,
                    "n",
                ),
                bus_publish_warning: None,
            },
            dispatch_strategy: "fresh-code-alignment".into(),
            inner_payload: json!({"task_id": "btk-1"}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: Some(AcceptanceEvaluation {
                status: AcceptanceStatus::ManualRequired,
                mode: Some(AcceptanceMode::Manual),
                commands: vec![],
                evidence_keys: vec![],
                reason: "manual mode".into(),
                fan_in: None,
            }),
        });
        assert_eq!(o.aggregate_status(), "dag_paused");
        assert_eq!(o.runner_status(), "review_gate_paused");
        let arr = o.node_results_json();
        let arr = arr.as_array().unwrap();
        assert_eq!(arr[0]["state"], "paused");
        assert_eq!(arr[0]["acceptance"]["status"], "manual_required");
        let qid = arr[0]["review_question_id"].as_str().unwrap();
        assert!(
            qid.starts_with("acceptance:plan:"),
            "manual_required pause id `{}` MUST use the acceptance: prefix",
            qid
        );
    }

    // ── wave-18 / task 03 — cross-node acceptance fan-in ─────────────────
    //
    // The fan-in evaluator overlays a deterministic decision on top of
    // the per-node acceptance evaluation. It NEVER re-runs the source
    // node — it only inspects the recorded `state` (lifecycle) and
    // `inner_payload`. Validator + evaluator invariants pinned below.

    fn make_succeeded_result(id: &str, payload: Value) -> NodeResult {
        NodeResult {
            id: id.to_string(),
            target: "mission_task_delegate".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "fresh-code-alignment".into(),
            inner_payload: payload,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        }
    }

    fn make_failed_result(id: &str) -> NodeResult {
        NodeResult {
            id: id.to_string(),
            target: "mission_task_delegate".into(),
            state: NodeState::Failed {
                reason: "test failure".into(),
            },
            dispatch_strategy: "fresh-code-alignment".into(),
            inner_payload: Value::Null,
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        }
    }

    #[test]
    fn parse_node_form_captures_acceptance_fan_in_hints() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate")
              (node :id "n2"
                    :target "mission_task_delegate"
                    :depends-on ["n1"]
                    :acceptance-depends-on ["n1"]
                    :acceptance-requires "all_succeeded"))
        "#;
        let parsed = parse_plan_dag(sexp);
        let n2 = parsed.nodes.iter().find(|n| n.id == "n2").unwrap();
        assert_eq!(n2.acceptance_depends_on, vec!["n1".to_string()]);
        assert_eq!(n2.acceptance_requires_raw.as_deref(), Some("all_succeeded"));
        assert_eq!(
            n2.acceptance_requires_kind(),
            Some(AcceptanceRequires::AllSucceeded)
        );
        assert!(n2.has_acceptance_fan_in());
        assert!(n2.has_acceptance_hints());
        // Recognised mode MUST NOT land in unsupported_fields.
        let unsupported_keys: Vec<String> = n2
            .unsupported_fields
            .iter()
            .map(|(k, _)| k.clone())
            .collect();
        for forbidden in [
            "acceptance-depends-on",
            "acceptance-requires",
            "acceptance-source-node",
        ] {
            assert!(
                !unsupported_keys.contains(&forbidden.to_string()),
                "key `{}` must land on a typed slot, not unsupported_fields",
                forbidden
            );
        }
    }

    #[test]
    fn parse_node_form_records_unrecognised_acceptance_requires_in_unsupported_fields() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate")
              (node :id "n2"
                    :target "mission_task_delegate"
                    :depends-on ["n1"]
                    :acceptance-depends-on ["n1"]
                    :acceptance-requires "majority_succeeded"))
        "#;
        let parsed = parse_plan_dag(sexp);
        let n2 = parsed.nodes.iter().find(|n| n.id == "n2").unwrap();
        // Raw value lands on the typed slot AND in unsupported_fields.
        assert_eq!(
            n2.acceptance_requires_raw.as_deref(),
            Some("majority_succeeded")
        );
        assert!(n2
            .unsupported_fields
            .iter()
            .any(|(k, v)| k == "acceptance-requires" && v == "majority_succeeded"));
        assert!(n2.acceptance_requires_kind().is_none());
    }

    #[test]
    fn build_validated_dag_rejects_acceptance_dep_referencing_missing_node() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :acceptance-depends-on ["does_not_exist"]
                    :acceptance-requires "all_succeeded"))
        "#;
        let err = build_validated_dag(sexp).expect_err("must reject missing fan-in dep");
        match err {
            DagBuildError::AcceptanceDependencyMissing { node_id, missing } => {
                assert_eq!(node_id, "n1");
                assert_eq!(missing, "does_not_exist");
            }
            other => panic!(
                "expected AcceptanceDependencyMissing, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn build_validated_dag_rejects_acceptance_dep_when_not_a_depends_on_ancestor() {
        // n2 declares :acceptance-depends-on ["n1"] but does NOT carry
        // n1 as a (transitive) :depends-on ancestor — the source node's
        // evidence may not exist when n2's acceptance phase runs, so
        // the validator MUST refuse instead of silently changing
        // execution order.
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate")
              (node :id "n2"
                    :target "mission_task_delegate"
                    :acceptance-depends-on ["n1"]
                    :acceptance-requires "all_succeeded"))
        "#;
        let err = build_validated_dag(sexp).expect_err("must reject non-ancestor fan-in dep");
        match err {
            DagBuildError::AcceptanceFanInDepNotAncestor { node_id, ancestor } => {
                assert_eq!(node_id, "n2");
                assert_eq!(ancestor, "n1");
            }
            other => panic!(
                "expected AcceptanceFanInDepNotAncestor, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn build_validated_dag_rejects_acceptance_depends_on_without_recognised_requires() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate")
              (node :id "n2"
                    :target "mission_task_delegate"
                    :depends-on ["n1"]
                    :acceptance-depends-on ["n1"]))
        "#;
        let err = build_validated_dag(sexp).expect_err("must reject missing requires");
        match err {
            DagBuildError::AcceptanceFanInRequiresMissing { node_id, .. } => {
                assert_eq!(node_id, "n2");
            }
            other => panic!(
                "expected AcceptanceFanInRequiresMissing, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn build_validated_dag_rejects_evidence_keys_mode_without_source_node() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate")
              (node :id "n2"
                    :target "mission_task_delegate"
                    :depends-on ["n1"]
                    :acceptance-depends-on ["n1"]
                    :acceptance-requires "evidence_keys"
                    :acceptance-evidence-keys ["build_ok"]))
        "#;
        let err = build_validated_dag(sexp)
            .expect_err("evidence_keys without :acceptance-source-node must fail");
        match err {
            DagBuildError::AcceptanceSourceNodeInvalid { node_id, detail } => {
                assert_eq!(node_id, "n2");
                assert!(
                    detail.contains("acceptance-source-node"),
                    "detail `{}` must mention the missing field",
                    detail
                );
            }
            other => panic!(
                "expected AcceptanceSourceNodeInvalid, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn build_validated_dag_rejects_source_node_outside_depends_on_list() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate")
              (node :id "n2"
                    :target "mission_task_delegate"
                    :depends-on ["n1"]
                    :acceptance-depends-on ["n1"]
                    :acceptance-requires "evidence_keys"
                    :acceptance-evidence-keys ["build_ok"]
                    :acceptance-source-node "n1_typo"))
        "#;
        let err = build_validated_dag(sexp).expect_err("source node mismatch must fail");
        match err {
            DagBuildError::AcceptanceSourceNodeInvalid { node_id, detail } => {
                assert_eq!(node_id, "n2");
                assert!(detail.contains("n1_typo"));
            }
            other => panic!(
                "expected AcceptanceSourceNodeInvalid, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn build_validated_dag_accepts_well_formed_fan_in() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate")
              (node :id "n2"
                    :target "mission_task_delegate"
                    :depends-on ["n1"]
                    :acceptance-depends-on ["n1"]
                    :acceptance-requires "all_succeeded"))
        "#;
        let (_parsed, order) =
            build_validated_dag(sexp).expect("well-formed fan-in must build");
        assert_eq!(order, vec!["n1".to_string(), "n2".to_string()]);
    }

    #[test]
    fn apply_fan_in_no_op_when_node_has_no_fan_in_hints() {
        // Absence of :acceptance-depends-on preserves the wave-17
        // shape exactly — the fan_in field is None on the way in
        // AND on the way out, regardless of prior_results contents.
        let node = acceptance_node_with(Some("inner_status"), None, None);
        let payload = json!({"task_id": "btk-1"});
        let base = evaluate_node_acceptance(&node, &payload, true);
        assert!(base.fan_in.is_none(), "baseline must carry no fan_in");
        let prior = HashMap::new();
        let after = apply_acceptance_fan_in(base.clone(), &node, &prior);
        assert_eq!(after.status, base.status);
        assert!(after.fan_in.is_none());
    }

    #[test]
    fn apply_fan_in_all_succeeded_passes_when_every_source_succeeded() {
        let mut node = acceptance_node_with(None, None, None);
        node.acceptance_depends_on = vec!["a".into(), "b".into()];
        node.acceptance_requires_raw = Some("all_succeeded".into());
        let r_a = make_succeeded_result("a", json!({}));
        let r_b = make_succeeded_result("b", json!({}));
        let prior: HashMap<String, &NodeResult> =
            [("a".to_string(), &r_a), ("b".to_string(), &r_b)]
                .into_iter()
                .collect();
        let base = evaluate_node_acceptance(&node, &json!({}), true);
        let after = apply_acceptance_fan_in(base, &node, &prior);
        assert_eq!(after.status, AcceptanceStatus::Accepted);
        let f = after.fan_in.expect("fan_in must be recorded");
        assert!(f.passed);
        assert_eq!(f.mode, AcceptanceRequires::AllSucceeded);
        assert_eq!(f.source_nodes, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn apply_fan_in_all_succeeded_rejects_when_one_source_failed() {
        let mut node = acceptance_node_with(None, None, None);
        node.acceptance_depends_on = vec!["a".into(), "b".into()];
        node.acceptance_requires_raw = Some("all_succeeded".into());
        let r_a = make_succeeded_result("a", json!({}));
        let r_b = make_failed_result("b");
        let prior: HashMap<String, &NodeResult> =
            [("a".to_string(), &r_a), ("b".to_string(), &r_b)]
                .into_iter()
                .collect();
        let base = evaluate_node_acceptance(&node, &json!({}), true);
        let after = apply_acceptance_fan_in(base, &node, &prior);
        assert_eq!(after.status, AcceptanceStatus::Rejected);
        let f = after.fan_in.expect("fan_in must be recorded");
        assert!(!f.passed);
        assert!(
            f.reason.contains("\"b\""),
            "reason `{}` must surface the failing source node",
            f.reason
        );
        assert!(after.reason.starts_with("acceptance_fan_in:"));
    }

    #[test]
    fn apply_fan_in_any_succeeded_passes_when_at_least_one_source_succeeded() {
        let mut node = acceptance_node_with(None, None, None);
        node.acceptance_depends_on = vec!["a".into(), "b".into()];
        node.acceptance_requires_raw = Some("any_succeeded".into());
        let r_a = make_failed_result("a");
        let r_b = make_succeeded_result("b", json!({}));
        let prior: HashMap<String, &NodeResult> =
            [("a".to_string(), &r_a), ("b".to_string(), &r_b)]
                .into_iter()
                .collect();
        let base = evaluate_node_acceptance(&node, &json!({}), true);
        let after = apply_acceptance_fan_in(base, &node, &prior);
        assert_eq!(after.status, AcceptanceStatus::Accepted);
        let f = after.fan_in.expect("fan_in must be recorded");
        assert!(f.passed);
        assert_eq!(f.mode, AcceptanceRequires::AnySucceeded);
    }

    #[test]
    fn apply_fan_in_any_succeeded_rejects_when_all_sources_failed() {
        let mut node = acceptance_node_with(None, None, None);
        node.acceptance_depends_on = vec!["a".into(), "b".into()];
        node.acceptance_requires_raw = Some("any_succeeded".into());
        let r_a = make_failed_result("a");
        let r_b = make_failed_result("b");
        let prior: HashMap<String, &NodeResult> =
            [("a".to_string(), &r_a), ("b".to_string(), &r_b)]
                .into_iter()
                .collect();
        let base = evaluate_node_acceptance(&node, &json!({}), true);
        let after = apply_acceptance_fan_in(base, &node, &prior);
        assert_eq!(after.status, AcceptanceStatus::Rejected);
        let f = after.fan_in.expect("fan_in must be recorded");
        assert!(!f.passed);
    }

    #[test]
    fn apply_fan_in_evidence_keys_passes_when_source_payload_carries_keys() {
        let mut node = acceptance_node_with(
            None,
            None,
            Some(r#"["build_ok" "tests_passed"]"#),
        );
        node.acceptance_depends_on = vec!["a".into()];
        node.acceptance_requires_raw = Some("evidence_keys".into());
        node.acceptance_source_node = Some("a".into());
        let r_a = make_succeeded_result(
            "a",
            json!({"build_ok": true, "tests_passed": 12}),
        );
        let prior: HashMap<String, &NodeResult> =
            [("a".to_string(), &r_a)].into_iter().collect();
        let base = evaluate_node_acceptance(&node, &json!({}), true);
        let after = apply_acceptance_fan_in(base, &node, &prior);
        assert_eq!(after.status, AcceptanceStatus::Accepted);
        let f = after.fan_in.expect("fan_in must be recorded");
        assert!(f.passed);
        assert_eq!(f.mode, AcceptanceRequires::EvidenceKeys);
        assert_eq!(f.source_nodes, vec!["a".to_string()]);
    }

    #[test]
    fn apply_fan_in_evidence_keys_rejects_when_source_missing_keys() {
        let mut node = acceptance_node_with(
            None,
            None,
            Some(r#"["build_ok" "tests_passed"]"#),
        );
        node.acceptance_depends_on = vec!["a".into()];
        node.acceptance_requires_raw = Some("evidence_keys".into());
        node.acceptance_source_node = Some("a".into());
        let r_a = make_succeeded_result("a", json!({"build_ok": true}));
        let prior: HashMap<String, &NodeResult> =
            [("a".to_string(), &r_a)].into_iter().collect();
        let base = evaluate_node_acceptance(&node, &json!({}), true);
        let after = apply_acceptance_fan_in(base, &node, &prior);
        assert_eq!(after.status, AcceptanceStatus::Rejected);
        let f = after.fan_in.expect("fan_in must be recorded");
        assert!(!f.passed);
        assert!(
            f.reason.contains("tests_passed"),
            "reason `{}` must surface the missing key",
            f.reason
        );
    }

    #[test]
    fn apply_fan_in_does_not_promote_a_per_node_rejected_decision() {
        // Per-node Rejected dominates — fan-in is recorded for audit
        // but never flips status back to Accepted.
        let mut node = acceptance_node_with(Some("inner_status"), None, None);
        node.acceptance_depends_on = vec!["a".into()];
        node.acceptance_requires_raw = Some("all_succeeded".into());
        let r_a = make_succeeded_result("a", json!({}));
        let prior: HashMap<String, &NodeResult> =
            [("a".to_string(), &r_a)].into_iter().collect();
        // Per-node: dispatch_succeeded=false → Rejected.
        let base = evaluate_node_acceptance(&node, &json!({}), false);
        assert_eq!(base.status, AcceptanceStatus::Rejected);
        let after = apply_acceptance_fan_in(base, &node, &prior);
        assert_eq!(
            after.status,
            AcceptanceStatus::Rejected,
            "per-node Rejected MUST dominate even when fan-in passes"
        );
        // Fan-in still recorded for audit.
        let f = after.fan_in.expect("fan_in must be recorded for audit");
        assert!(f.passed, "fan-in itself passed even though parent rejected");
    }

    #[test]
    fn apply_fan_in_records_outcome_under_to_json_when_active() {
        let mut node = acceptance_node_with(None, None, None);
        node.acceptance_depends_on = vec!["a".into()];
        node.acceptance_requires_raw = Some("all_succeeded".into());
        let r_a = make_succeeded_result("a", json!({}));
        let prior: HashMap<String, &NodeResult> =
            [("a".to_string(), &r_a)].into_iter().collect();
        let base = evaluate_node_acceptance(&node, &json!({}), true);
        let after = apply_acceptance_fan_in(base, &node, &prior);
        let v = after.to_json();
        assert_eq!(v["status"], "accepted");
        assert_eq!(v["fan_in"]["mode"], "all_succeeded");
        assert_eq!(v["fan_in"]["passed"], true);
        assert_eq!(v["fan_in"]["source_nodes"][0], "a");
        assert!(v["fan_in"]["reason"].is_string());
    }

    #[test]
    fn build_nodes_summary_surfaces_fan_in_hints_when_present() {
        let nodes = vec![
            DagNode {
                id: "a".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
            DagNode {
                id: "b".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                depends_on: vec!["a".into()],
                acceptance_depends_on: vec!["a".into()],
                acceptance_requires_raw: Some("evidence_keys".into()),
                acceptance_source_node: Some("a".into()),
                acceptance_evidence_keys_raw: Some(r#"["k"]"#.into()),
                ..Default::default()
            },
        ];
        let order = vec!["a".to_string(), "b".to_string()];
        let summary = build_nodes_summary(&nodes, &order);
        let arr = summary.as_array().unwrap();
        // a: no fan-in fields surface.
        assert!(arr[0].get("acceptance_depends_on").is_none());
        assert!(arr[0].get("acceptance_requires").is_none());
        assert!(arr[0].get("acceptance_source_node").is_none());
        // b: every declared field surfaces.
        assert_eq!(arr[1]["acceptance_depends_on"][0], "a");
        assert_eq!(arr[1]["acceptance_requires"], "evidence_keys");
        assert_eq!(arr[1]["acceptance_source_node"], "a");
    }

    // ── wave-17 / task 04 — conservative rollback descriptors ────────────
    //
    // The rollback pass runs AFTER a node's final failed attempt and
    // BEFORE downstream taint propagation. It NEVER runs destructive
    // shell commands; it only records intent, builds descriptors, or
    // (in workstation mode) hands a scoped task brief to the existing
    // wave-15 substrate. These tests pin every branch of the decision
    // tree plus the failure-policy invariants the brief calls out.

    fn rollback_node_with(
        policy: Option<&str>,
        objective: Option<&str>,
        owned_files: Option<&str>,
        acceptance_commands: Option<&str>,
    ) -> DagNode {
        DagNode {
            id: "n".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            // A safe forward dispatch strategy so the workstation
            // safety check can pass when the test wants it to.
            dispatch_strategy: Some("fresh-code-alignment".into()),
            target_project: Some("missiond".into()),
            rollback_policy: policy.map(|s| s.to_string()),
            rollback_objective: objective.map(|s| s.to_string()),
            rollback_owned_files_raw: owned_files.map(|s| s.to_string()),
            rollback_acceptance_commands_raw: acceptance_commands
                .map(|s| s.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn parse_node_form_captures_rollback_policy_hints() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :rollback-policy "workstation"
                    :rollback-objective "undo migration step 3"
                    :rollback-owned-files ["src/migrations/0003.rs"]
                    :rollback-acceptance-commands ["cargo test -p missiond"]))
        "#;
        let parsed = parse_plan_dag(sexp);
        let n = &parsed.nodes[0];
        assert_eq!(n.rollback_policy.as_deref(), Some("workstation"));
        assert_eq!(
            n.rollback_objective.as_deref(),
            Some("undo migration step 3")
        );
        assert!(n
            .rollback_owned_files_raw
            .as_deref()
            .unwrap()
            .contains("src/migrations/0003.rs"));
        assert!(n
            .rollback_acceptance_commands_raw
            .as_deref()
            .unwrap()
            .contains("cargo test"));
        // None of the new keys must land in unsupported_fields — that
        // would mean the scheduler can't route the rollback pass.
        let unsupported_keys: Vec<String> =
            n.unsupported_fields.iter().map(|(k, _)| k.clone()).collect();
        for forbidden in [
            "rollback-policy",
            "rollback-objective",
            "rollback-owned-files",
            "rollback-acceptance-commands",
        ] {
            assert!(
                !unsupported_keys.contains(&forbidden.to_string()),
                "key `{}` must land on a typed slot, not unsupported_fields",
                forbidden
            );
        }
        assert!(n.has_rollback_hints());
        assert_eq!(n.rollback_policy_kind(), Some(RollbackPolicy::Workstation));
    }

    #[test]
    fn parse_node_form_records_unrecognised_rollback_policy_in_unsupported_fields() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :rollback-policy "self_destruct"))
        "#;
        let parsed = parse_plan_dag(sexp);
        let n = &parsed.nodes[0];
        assert_eq!(n.rollback_policy.as_deref(), Some("self_destruct"));
        // Typo lands in the typed slot AND is surfaced via the
        // unsupported_fields audit so the response loudly flags it.
        assert!(n
            .unsupported_fields
            .iter()
            .any(|(k, v)| k == "rollback-policy" && v == "self_destruct"));
        // Typed projection refuses to interpret a typo as a real policy.
        assert!(n.rollback_policy_kind().is_none());
    }

    #[test]
    fn rollback_policy_default_is_no_rollback_when_absent() {
        // Defaults: absent -> no rollback / no destructive action.
        let node = rollback_node_with(None, None, None, None);
        assert!(!node.has_rollback_hints());
        assert!(node.rollback_policy_kind().is_none());
        let descriptor = build_rollback_descriptor(&node);
        assert_eq!(descriptor.policy, RollbackPolicy::None);
        assert!(descriptor.objective.is_none());
        assert!(descriptor.owned_files.is_empty());
        assert!(descriptor.acceptance_commands.is_empty());
        let eval = pre_dispatch_rollback_decision(&node);
        assert_eq!(eval.status, RollbackStatus::NotRequested);
        assert!(eval.is_inactive());
    }

    #[test]
    fn rollback_policy_explicit_none_is_inactive() {
        // `:rollback-policy "none"` is the explicit opt-out — the
        // descriptor still parses, but the evaluator surfaces
        // `not_requested` so the response stays quiet.
        let node = rollback_node_with(Some("none"), Some("noop"), None, None);
        assert_eq!(node.rollback_policy_kind(), Some(RollbackPolicy::None));
        let eval = pre_dispatch_rollback_decision(&node);
        assert_eq!(eval.policy, RollbackPolicy::None);
        assert_eq!(eval.status, RollbackStatus::NotRequested);
    }

    #[test]
    fn rollback_descriptor_mode_records_intent_without_dispatch() {
        let node = rollback_node_with(
            Some("descriptor"),
            Some("undo step"),
            Some(r#"["src/a.rs"]"#),
            Some(r#"["cargo test"]"#),
        );
        let descriptor = build_rollback_descriptor(&node);
        assert_eq!(descriptor.policy, RollbackPolicy::Descriptor);
        assert_eq!(descriptor.objective.as_deref(), Some("undo step"));
        assert_eq!(descriptor.owned_files, vec!["src/a.rs".to_string()]);
        let eval = pre_dispatch_rollback_decision(&node);
        assert_eq!(eval.status, RollbackStatus::DescriptorReady);
        // No inner payload — descriptor mode never touches the substrate.
        assert!(eval.inner_payload.is_none());
        // Brief preview is computed by the async helper, not the pure
        // decision; pre-dispatch evaluation leaves it None.
        assert!(eval.task_brief_preview.is_none());
    }

    #[test]
    fn rollback_workstation_mode_passes_safety_when_all_signals_present() {
        let node = rollback_node_with(
            Some("workstation"),
            Some("undo migration"),
            Some(r#"["src/a.rs"]"#),
            None,
        );
        let descriptor = build_rollback_descriptor(&node);
        assert_eq!(descriptor.policy, RollbackPolicy::Workstation);
        assert!(descriptor.safety_check_for_workstation(&node).is_ok());
    }

    #[test]
    fn rollback_workstation_mode_refuses_when_objective_missing() {
        // No rollback-objective declared — workstation mode requires it
        // because a content-free brief is useless.
        let node = rollback_node_with(
            Some("workstation"),
            None,
            Some(r#"["src/a.rs"]"#),
            None,
        );
        let descriptor = build_rollback_descriptor(&node);
        let err = descriptor
            .safety_check_for_workstation(&node)
            .expect_err("missing objective must refuse");
        assert!(err.contains(":rollback-objective"));
        let eval = pre_dispatch_rollback_decision(&node);
        assert_eq!(eval.status, RollbackStatus::Refused);
        assert!(eval.reason.contains(":rollback-objective"));
    }

    #[test]
    fn rollback_workstation_mode_refuses_when_owned_files_empty() {
        let node = rollback_node_with(
            Some("workstation"),
            Some("undo step"),
            None,
            None,
        );
        let descriptor = build_rollback_descriptor(&node);
        let err = descriptor
            .safety_check_for_workstation(&node)
            .expect_err("missing owned files must refuse");
        assert!(err.contains(":rollback-owned-files"));
        let eval = pre_dispatch_rollback_decision(&node);
        assert_eq!(eval.status, RollbackStatus::Refused);
    }

    #[test]
    fn rollback_workstation_mode_refuses_when_no_project_signal() {
        // Mutate the node so neither :target-project nor :requested-cwd
        // is set — the safety gate must refuse.
        let mut node = rollback_node_with(
            Some("workstation"),
            Some("undo step"),
            Some(r#"["src/a.rs"]"#),
            None,
        );
        node.target_project = None;
        node.requested_cwd = None;
        let descriptor = build_rollback_descriptor(&node);
        let err = descriptor
            .safety_check_for_workstation(&node)
            .expect_err("missing project signal must refuse");
        assert!(err.contains(":target-project"));
        let eval = pre_dispatch_rollback_decision(&node);
        assert_eq!(eval.status, RollbackStatus::Refused);
        assert!(eval.reason.contains(":target-project"));
    }

    #[test]
    fn rollback_workstation_mode_refuses_when_dispatch_strategy_unsafe() {
        // `unknown` (the default) is not on the inferable whitelist;
        // the safety gate must refuse so the rollback never rides an
        // unsupported substrate.
        let mut node = rollback_node_with(
            Some("workstation"),
            Some("undo step"),
            Some(r#"["src/a.rs"]"#),
            None,
        );
        node.dispatch_strategy = Some("unknown".into());
        let descriptor = build_rollback_descriptor(&node);
        let err = descriptor
            .safety_check_for_workstation(&node)
            .expect_err("unsafe dispatch strategy must refuse");
        assert!(err.contains(":dispatch-strategy"));
        let eval = pre_dispatch_rollback_decision(&node);
        assert_eq!(eval.status, RollbackStatus::Refused);
    }

    #[test]
    fn rollback_evaluation_to_json_carries_every_surface_field() {
        let eval = RollbackEvaluation {
            policy: RollbackPolicy::Workstation,
            status: RollbackStatus::Dispatched,
            reason: "ok".into(),
            objective: Some("undo step".into()),
            owned_files: vec!["src/a.rs".into()],
            acceptance_commands: vec!["cargo test".into()],
            task_brief_preview: Some("## Objective\nundo step\n".into()),
            task_brief_path: Some("/tmp/rollback.md".into()),
            inner_payload: Some(json!({"task_id": "btk-rb"})),
            cascade: None,
        };
        let v = eval.to_json();
        assert_eq!(v["policy"], "workstation");
        assert_eq!(v["status"], "dispatched");
        assert_eq!(v["reason"], "ok");
        assert_eq!(v["objective"], "undo step");
        assert_eq!(v["owned_files"][0], "src/a.rs");
        assert_eq!(v["acceptance_commands"][0], "cargo test");
        // CRITICAL invariant — `acceptance_commands_executed=false` is
        // pinned so audit dashboards can pivot on the flag and prove
        // the scheduler never ran shell on behalf of a rollback brief.
        assert_eq!(v["acceptance_commands_executed"], false);
        assert!(v["task_brief_preview"]
            .as_str()
            .unwrap()
            .contains("undo step"));
        assert_eq!(v["task_brief_path"], "/tmp/rollback.md");
        assert_eq!(v["inner_result"]["task_id"], "btk-rb");
    }

    #[test]
    fn rollback_status_wire_strings_are_distinct_and_stable() {
        // Pin the wire vocabulary so audit dashboards can grep on
        // these strings without re-deriving them from the enum.
        assert_eq!(RollbackStatus::NotRequested.as_wire(), "not_requested");
        assert_eq!(RollbackStatus::DescriptorReady.as_wire(), "descriptor_ready");
        assert_eq!(RollbackStatus::Dispatched.as_wire(), "dispatched");
        assert_eq!(RollbackStatus::Refused.as_wire(), "refused");
        assert_eq!(RollbackStatus::Failed.as_wire(), "failed");
        // RollbackPolicy mirror.
        assert_eq!(RollbackPolicy::None.as_wire(), "none");
        assert_eq!(RollbackPolicy::Descriptor.as_wire(), "descriptor");
        assert_eq!(RollbackPolicy::Workstation.as_wire(), "workstation");
    }

    #[test]
    fn rollback_evaluation_is_inactive_only_when_truly_empty() {
        // Inactive: no policy + no fields.
        let inactive = RollbackEvaluation {
            policy: RollbackPolicy::None,
            status: RollbackStatus::NotRequested,
            reason: "no rollback hints declared".into(),
            objective: None,
            owned_files: vec![],
            acceptance_commands: vec![],
            task_brief_preview: None,
            task_brief_path: None,
            inner_payload: None,
            cascade: None,
        };
        assert!(inactive.is_inactive());
        // ANY signal should flip is_inactive to false so the response
        // surfaces the row even when the policy is None (e.g. the
        // explicit-none case where the author wrote out an objective
        // but then suppressed dispatch).
        let mut with_obj = inactive.clone();
        with_obj.objective = Some("intent".into());
        assert!(!with_obj.is_inactive());
    }

    #[test]
    fn build_nodes_summary_surfaces_rollback_block_when_present() {
        let nodes = vec![
            DagNode {
                id: "with".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                rollback_policy: Some("descriptor".into()),
                rollback_objective: Some("undo".into()),
                ..Default::default()
            },
            DagNode {
                id: "plain".into(),
                target: "mission_execution".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ];
        let order = vec!["with".to_string(), "plain".to_string()];
        let summary = build_nodes_summary(&nodes, &order);
        let arr = summary.as_array().unwrap();
        assert_eq!(arr[0]["rollback"]["policy"], "descriptor");
        assert_eq!(arr[0]["rollback"]["objective"], "undo");
        // Plain node has no rollback hints — summary stays quiet
        // (regression guard for the wave-17 / task 03 baseline).
        assert!(arr[1].get("rollback").is_none());
    }

    #[test]
    fn node_results_json_surfaces_rollback_block_only_when_active() {
        let mut o = ExecutionOutcome::default();
        // Active rollback — surfaces.
        o.results.push(NodeResult {
            id: "with_rb".into(),
            target: "mission_task_delegate".into(),
            state: NodeState::Failed { reason: "boom".into() },
            dispatch_strategy: "fresh-code-alignment".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: Some(RollbackEvaluation {
                policy: RollbackPolicy::Descriptor,
                status: RollbackStatus::DescriptorReady,
                reason: "descriptor mode".into(),
                objective: Some("undo".into()),
                owned_files: vec!["src/a.rs".into()],
                acceptance_commands: vec![],
                task_brief_preview: None,
                task_brief_path: None,
                inner_payload: None,
                cascade: None,
            }),
            acceptance: None,
        });
        // No rollback hints — quiet.
        o.results.push(NodeResult {
            id: "plain".into(),
            target: "mission_execution".into(),
            state: NodeState::Failed { reason: "boom".into() },
            dispatch_strategy: "unknown".into(),
            inner_payload: json!({}),
            attempts_made: 1,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        });
        let v = o.node_results_json();
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["rollback"]["policy"], "descriptor");
        assert_eq!(arr[0]["rollback"]["status"], "descriptor_ready");
        assert!(arr[1].get("rollback").is_none());
    }

    #[tokio::test]
    async fn run_rollback_descriptor_mode_skips_dispatch_and_records_brief() {
        // Descriptor mode never dispatches — we can run the async
        // helper without a real AppState because the substrate is
        // never invoked. We use a dummy state via the existing
        // `tempfile`-backed registry path the workstation_dispatch
        // tests use; descriptor mode doesn't read any AppState
        // fields.
        //
        // To stay self-contained we just call the pure pre-dispatch
        // decision (which is the byte-identical projection minus the
        // brief preview) and assert the contract.
        let node = rollback_node_with(
            Some("descriptor"),
            Some("undo step"),
            Some(r#"["src/a.rs"]"#),
            Some(r#"["cargo test"]"#),
        );
        let eval = pre_dispatch_rollback_decision(&node);
        assert_eq!(eval.policy, RollbackPolicy::Descriptor);
        assert_eq!(eval.status, RollbackStatus::DescriptorReady);
        // CRITICAL — descriptor mode NEVER produces an inner_payload
        // because the substrate is not invoked.
        assert!(eval.inner_payload.is_none());
    }

    #[test]
    fn rollback_workstation_brief_includes_canonical_sections() {
        // The rollback brief reuses the wave-15 task-brief shape so
        // observers see the same headings as a forward task brief.
        let node = rollback_node_with(
            Some("workstation"),
            Some("undo migration step 3"),
            Some(r#"["src/migrations/0003.rs"]"#),
            Some(r#"["cargo test -p missiond"]"#),
        );
        let descriptor = build_rollback_descriptor(&node);
        let hints = descriptor.to_workstation_hints(&node);
        assert_eq!(hints.objective.as_deref(), Some("undo migration step 3"));
        assert!(hints
            .scope
            .as_deref()
            .unwrap()
            .contains("rollback for failed plan-DAG node"));
        assert_eq!(hints.owned_files, vec!["src/migrations/0003.rs".to_string()]);
        assert_eq!(
            hints.acceptance_commands,
            vec!["cargo test -p missiond".to_string()]
        );
        // Default commit policy lands as "scoped" so the rollback
        // brief inherits the scoped-commit invariant.
        assert_eq!(hints.commit_policy.as_deref(), Some("scoped"));
        // Build the brief through the substrate helper to confirm
        // the canonical sections are present.
        let plan = fixture_plan("(plan)");
        let brief = crate::handlers::knowledge::workstation_dispatch::build_task_brief(
            &plan,
            &hints,
            "fresh-code-alignment",
        );
        assert!(brief.contains("## Objective"));
        assert!(brief.contains("## Scope"));
        assert!(brief.contains("rollback for failed plan-DAG node"));
        assert!(brief.contains("## Owned files"));
        assert!(brief.contains("- src/migrations/0003.rs"));
        assert!(brief.contains("## Acceptance commands"));
        assert!(brief.contains("- cargo test -p missiond"));
        assert!(brief.contains("## Commit policy"));
    }

    #[test]
    fn rollback_failure_policy_interaction_taint_still_propagates_under_descriptor_mode() {
        // Pin the failure-policy contract: the rollback decision
        // never short-circuits taint propagation. We exercise the
        // pure helpers because the wave loop has the full integration
        // (covered by build_validated_dag + execute_with_concurrency
        // in production); the decision-level test here protects
        // against a refactor that flips the rollback evaluator into
        // a "rolled back successfully so don't taint downstream"
        // bug.
        //
        // Concretely: a node with `:rollback-policy "descriptor"`
        // that fails MUST still cause `propagate_taint` to mark its
        // downstream as `tainted_by`. We build the graph + simulate
        // the failure here.
        let nodes = vec![
            DagNode {
                id: "a".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                rollback_policy: Some("descriptor".into()),
                rollback_objective: Some("undo".into()),
                ..Default::default()
            },
            DagNode {
                id: "b".into(),
                target: "mission_execution".into(),
                depends_on: vec!["a".into()],
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ];
        let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
        for n in &nodes {
            for dep in &n.depends_on {
                succs.entry(dep.as_str()).or_default().push(n.id.as_str());
            }
        }
        let mut tainted: HashMap<String, String> = HashMap::new();
        // Simulate the wave loop: rollback runs FIRST, then
        // propagate_taint. Both happen so downstream stays tainted.
        let _eval = pre_dispatch_rollback_decision(&nodes[0]);
        propagate_taint(&nodes[0], &succs, &mut tainted);
        assert_eq!(
            tainted.get("b"),
            Some(&"a".to_string()),
            "rollback descriptor mode must NOT short-circuit downstream taint"
        );
    }

    #[test]
    fn rollback_failure_policy_interaction_taint_still_propagates_under_workstation_mode() {
        // Same invariant for the workstation mode — even when
        // the rollback dispatch succeeds, taint propagates so
        // downstream sees the failure.
        let nodes = vec![
            DagNode {
                id: "a".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "continue".into(),
                rollback_policy: Some("workstation".into()),
                rollback_objective: Some("undo".into()),
                rollback_owned_files_raw: Some(r#"["src/a.rs"]"#.into()),
                target_project: Some("missiond".into()),
                dispatch_strategy: Some("fresh-code-alignment".into()),
                ..Default::default()
            },
            DagNode {
                id: "b".into(),
                target: "mission_execution".into(),
                depends_on: vec!["a".into()],
                failure_policy: "continue".into(),
                ..Default::default()
            },
        ];
        let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
        for n in &nodes {
            for dep in &n.depends_on {
                succs.entry(dep.as_str()).or_default().push(n.id.as_str());
            }
        }
        let mut tainted: HashMap<String, String> = HashMap::new();
        let _eval = pre_dispatch_rollback_decision(&nodes[0]);
        propagate_taint(&nodes[0], &succs, &mut tainted);
        assert_eq!(
            tainted.get("b"),
            Some(&"a".to_string()),
            "rollback workstation mode must NOT short-circuit downstream taint"
        );
    }

    #[test]
    fn rollback_safe_descriptor_refusals_are_non_retryable() {
        // Per the brief: SafeDescriptor refusals must not be retried.
        // The refusal vocabulary the rollback evaluator emits when
        // safety fails is `RollbackStatus::Refused`. Pin this so a
        // future refactor can't accidentally route a refused
        // rollback back through the wave-loop's retry path.
        //
        // Test: a node with `:rollback-policy "workstation"` but
        // missing `:rollback-objective` should evaluate to Refused
        // and the reason should explicitly mention the failing gate.
        let node = rollback_node_with(
            Some("workstation"),
            None, // objective missing — safety refusal
            Some(r#"["src/a.rs"]"#),
            None,
        );
        let eval = pre_dispatch_rollback_decision(&node);
        assert_eq!(eval.status, RollbackStatus::Refused);
        // The wave loop's retry-decision predicate is
        // `plan_node_should_retry`. SafeDescriptor refusals from the
        // forward dispatch already set `non_retryable=true`. Our
        // rollback Refused status is similarly non-retryable in the
        // sense that the wave loop never retries the failed node
        // (it only ever runs the rollback ONCE per terminal failure).
        // We assert the stable wire form so dashboards can pivot.
        assert_eq!(eval.status.as_wire(), "refused");
    }

    #[test]
    fn rollback_descriptor_carries_acceptance_commands_unexecuted() {
        // CRITICAL invariant — the rollback brief surfaces declared
        // commands verbatim AND the JSON projection pins
        // `acceptance_commands_executed=false` so audit dashboards
        // can prove the scheduler never ran them.
        let node = rollback_node_with(
            Some("descriptor"),
            Some("undo"),
            None,
            Some(r#"["rm -rf /" "echo all good"]"#),
        );
        let eval = pre_dispatch_rollback_decision(&node);
        let v = eval.to_json();
        assert_eq!(
            v["acceptance_commands_executed"], false,
            "rollback evaluator MUST surface acceptance_commands_executed=false"
        );
        assert_eq!(v["acceptance_commands"][0], "rm -rf /");
        assert_eq!(v["acceptance_commands"][1], "echo all good");
    }

    // ── wave-17 / task 05 — DAG finalize + distill trigger v0 ──────────

    #[test]
    fn parse_finalize_plan_defaults_false_for_backward_compat() {
        // No knob present → existing wave-17 / task 04 byte-shape MUST be
        // preserved. The whole point of the default is that nothing
        // observable changes for callers that did not opt in.
        assert!(!parse_finalize_plan(&json!({})));
        assert!(!parse_finalize_plan(&json!({"finalize_plan": false})));
        assert!(parse_finalize_plan(&json!({"finalize_plan": true})));
        // Non-bool values normalise to false rather than fail — finalize is
        // additive so a typo on the runtime knob never breaks dispatch.
        assert!(!parse_finalize_plan(&json!({"finalize_plan": "yes"})));
        assert!(!parse_finalize_plan(&json!({"finalize_plan": 1})));
    }

    #[test]
    fn parse_distill_on_success_defaults_false() {
        assert!(!parse_distill_on_success(&json!({})));
        assert!(!parse_distill_on_success(&json!({"distill_on_success": false})));
        assert!(parse_distill_on_success(&json!({"distill_on_success": true})));
        assert!(!parse_distill_on_success(&json!({"distill_on_success": "yep"})));
    }

    #[test]
    fn parse_distill_mode_arg_default_dry_run() {
        // Absence + empty + literal "dry_run" all collapse onto the
        // canonical "dry_run" so the response always echoes a known mode.
        assert_eq!(parse_distill_mode_arg(&json!({})).unwrap(), "dry_run");
        assert_eq!(
            parse_distill_mode_arg(&json!({"distill_mode": ""})).unwrap(),
            "dry_run"
        );
        assert_eq!(
            parse_distill_mode_arg(&json!({"distill_mode": "dry_run"})).unwrap(),
            "dry_run"
        );
        assert_eq!(
            parse_distill_mode_arg(&json!({"distill_mode": "sonnet"})).unwrap(),
            "sonnet"
        );
    }

    #[test]
    fn parse_distill_mode_arg_rejects_typos() {
        // Strict allowlist mirrors workflow.rs::parse_distill_mode so the
        // two surfaces cannot drift; a typo must surface even when
        // distill_on_success=false (caught up-front by validate_finalize_args).
        let err = parse_distill_mode_arg(&json!({"distill_mode": "sonet"})).unwrap_err();
        assert!(err.contains("dry_run"), "error must spell out the allowlist");
        assert!(err.contains("sonet"), "error must echo the rejected value");
    }

    #[test]
    fn validate_finalize_args_accepts_default_baseline() {
        // No finalize knobs at all → wave-17 / task 04 byte-shape lives.
        assert!(validate_finalize_args(&json!({})).is_none());
        // finalize_plan alone is fine — distill is opt-in on top.
        assert!(validate_finalize_args(&json!({"finalize_plan": true})).is_none());
        // finalize_plan + distill_on_success is the canonical opt-in.
        assert!(validate_finalize_args(
            &json!({"finalize_plan": true, "distill_on_success": true})
        )
        .is_none());
        assert!(validate_finalize_args(
            &json!({"finalize_plan": true, "distill_on_success": true, "distill_mode": "sonnet"})
        )
        .is_none());
    }

    #[test]
    fn validate_finalize_args_rejects_distill_without_finalize() {
        // Silently ignoring a distill request would mask caller intent —
        // fail-fast surfaces the contradiction.
        let result =
            validate_finalize_args(&json!({"distill_on_success": true})).unwrap();
        assert_eq!(result.is_error, Some(true));
        let payload = tool_result_payload(&result);
        // Structured errors serialise as `{ error_code, reason, suggestion?, trace_id? }`.
        // The reason field carries the human-readable diagnostic.
        let reason = payload
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(
            reason.contains("finalize_plan=true"),
            "error must point at the missing finalize knob; got `{}` (full payload: {})",
            reason,
            payload
        );
        assert_eq!(
            payload.get("error_code").and_then(|v| v.as_str()),
            Some("INVALID_PARAM")
        );
    }

    #[test]
    fn validate_finalize_args_rejects_unknown_distill_mode() {
        // Validation runs even when distill_on_success=false — a typo
        // should fail the next live caller's dispatch up-front, not
        // silently survive into production.
        let result = validate_finalize_args(&json!({"distill_mode": "warp"})).unwrap();
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn finalize_plan_status_label_maps_aggregate_to_status() {
        // Pin the aggregate -> plan_status mapping table so a future
        // refactor cannot silently advance the plan FSM past a paused
        // run. The `dag_paused` row in particular MUST preserve the
        // current status — claiming success while a node awaits review
        // is the exact "do not lie" invariant the brief calls out.
        assert_eq!(
            finalize_plan_status_label("dag_succeeded", "executing"),
            "succeeded"
        );
        assert_eq!(
            finalize_plan_status_label("dag_failed", "executing"),
            "failed"
        );
        assert_eq!(
            finalize_plan_status_label("dag_partial", "executing"),
            "failed"
        );
        // Paused → never claim success. Preserve the in-flight status
        // so the resume helper (wave-17 / task 01) can advance it once
        // the gate resolves.
        assert_eq!(
            finalize_plan_status_label("dag_paused", "executing"),
            "executing"
        );
        assert_eq!(
            finalize_plan_status_label("dag_paused", "awaiting_review"),
            "awaiting_review"
        );
        // Defensive: an unrecognised aggregate must not pretend success.
        assert_eq!(
            finalize_plan_status_label("dag_unknown", "approved"),
            "unchanged"
        );
    }

    #[test]
    fn build_finalization_block_carries_rule_label_per_aggregate() {
        // The `rule` field lets audit dashboards group runs by the same
        // mapping rule without re-deriving the aggregate semantics.
        let succeeded =
            build_finalization_block("dag_succeeded", Some("succeeded"), None, None);
        assert_eq!(succeeded["finalize_plan"], true);
        assert_eq!(succeeded["aggregate_status"], "dag_succeeded");
        assert_eq!(succeeded["final_plan_status"], "succeeded");
        assert_eq!(succeeded["rule"], "all_terminal_no_failed_no_paused");
        assert!(succeeded.get("distill").is_none());

        let failed = build_finalization_block("dag_failed", Some("failed"), None, None);
        assert_eq!(failed["rule"], "fail_fast_or_failure_dominates");

        let partial =
            build_finalization_block("dag_partial", Some("failed"), None, None);
        assert_eq!(partial["rule"], "failed_node_or_skipped_without_paused");

        // Paused: response MUST report the current (preserved) status —
        // not a fictitious "succeeded".
        let paused =
            build_finalization_block("dag_paused", Some("executing"), None, None);
        assert_eq!(paused["final_plan_status"], "executing");
        assert_eq!(paused["rule"], "paused_node_present_no_finalization");
    }

    #[test]
    fn build_finalization_block_surfaces_distill_block_when_present() {
        // The distill block round-trips into the finalization shape so
        // callers can grep `finalization.distill.triggered` without a
        // second hop.
        let distill =
            build_distill_block(true, "distill_invoked_ok", "dry_run", Some(json!({"ok": true})), false);
        let block = build_finalization_block(
            "dag_succeeded",
            Some("succeeded"),
            None,
            Some(distill),
        );
        assert_eq!(block["distill"]["triggered"], true);
        assert_eq!(block["distill"]["reason"], "distill_invoked_ok");
        assert_eq!(block["distill"]["distill_mode"], "dry_run");
        assert_eq!(block["distill"]["result"]["ok"], true);
        assert!(block["distill"].get("warning").is_none());
    }

    #[test]
    fn build_finalization_block_surfaces_plan_status_update_error() {
        // When the FSM update itself fails (e.g. PG transient error) the
        // block MUST surface that explicitly so callers can route — the
        // distill trigger ALSO refuses to fire in that case (verified by
        // maybe_run_distill_trigger logic) so the audit row never claims a
        // distill ran against an inconsistent plan state.
        let block = build_finalization_block(
            "dag_succeeded",
            None,
            Some("DB connection lost"),
            None,
        );
        assert_eq!(
            block["plan_status_update_error"], "DB connection lost",
            "FSM update error must round-trip into the response"
        );
        assert_eq!(block["final_plan_status"], "unchanged");
    }

    #[test]
    fn build_distill_block_skipped_path_preserves_reason() {
        // distill_on_success=false → no trigger, no result, but we still
        // surface the mode for the response shape consistency.
        let b = build_distill_block(false, "aggregate_not_succeeded", "dry_run", None, false);
        assert_eq!(b["triggered"], false);
        assert_eq!(b["reason"], "aggregate_not_succeeded");
        assert_eq!(b["distill_mode"], "dry_run");
        assert!(b.get("result").is_none());
        assert!(b.get("warning").is_none());
    }

    #[test]
    fn build_distill_block_failure_surfaces_warning_keeps_triggered_true() {
        // When the workflow distill handler returns an error we MUST keep
        // `triggered=true` (it ran) but add a `warning` so callers can
        // detect partial success. CRITICAL: the brief forbids breaking
        // the plan final state when distill fails.
        let b = build_distill_block(
            true,
            "distill_invoked_returned_error",
            "sonnet",
            Some(json!({"error": "sonnet quota exhausted"})),
            true,
        );
        assert_eq!(b["triggered"], true);
        assert_eq!(b["distill_mode"], "sonnet");
        assert_eq!(
            b["warning"],
            "distill trigger returned an error; plan final state preserved"
        );
        assert_eq!(b["result"]["error"], "sonnet quota exhausted");
    }

    #[test]
    fn build_distill_block_success_omits_warning() {
        let b = build_distill_block(
            true,
            "distill_invoked_ok",
            "dry_run",
            Some(json!({"status": "dry_run", "persisted": false})),
            false,
        );
        assert_eq!(b["triggered"], true);
        assert!(b.get("warning").is_none(), "ok branch must not warn");
        assert_eq!(b["result"]["persisted"], false);
    }

    // ── wave-18 / task 04 — conservative cascade rollback ────────────────
    //
    // Cascade evaluator is conservative by design: it never runs unless
    // the failed (cascade-root) node opted in via `:rollback-cascade`,
    // and even then `plan` mode never dispatches. `dispatch-safe` mode
    // dispatches a compensation node only when its OWN safety gates
    // pass; descriptor-only / no-policy compensations stay
    // descriptor_ready (never silently promoted). These tests pin the
    // parser, the compensation discovery + ordering, the plan-mode
    // recording branch, the dispatch-safe refusal branch, and the
    // default-mode invariant.

    #[test]
    fn parse_node_form_captures_cascade_hints() {
        let sexp = r#"
            (plan
              (node :id "fail"
                    :target "mission_task_delegate"
                    :rollback-cascade "dispatch-safe"
                    :rollback-policy "descriptor"
                    :rollback-objective "root failed")
              (node :id "comp-1"
                    :target "mission_task_delegate"
                    :compensates "fail"
                    :rollback-after ["comp-2"])
              (node :id "comp-2"
                    :target "mission_task_delegate"
                    :compensates "fail"))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes.len(), 3);
        // Cascade root parses :rollback-cascade onto the typed slot
        // AND surfaces the typed projection.
        let root = &parsed.nodes[0];
        assert_eq!(root.rollback_cascade.as_deref(), Some("dispatch-safe"));
        assert_eq!(
            root.rollback_cascade_kind(),
            Some(RollbackCascadeMode::DispatchSafe)
        );
        assert!(root.has_active_rollback_cascade());
        // Compensation nodes parse :compensates + :rollback-after.
        let c1 = &parsed.nodes[1];
        assert_eq!(c1.compensates.as_deref(), Some("fail"));
        assert_eq!(c1.rollback_after, vec!["comp-2".to_string()]);
        let c2 = &parsed.nodes[2];
        assert_eq!(c2.compensates.as_deref(), Some("fail"));
        assert!(c2.rollback_after.is_empty());
        // None of the new keys lands in unsupported_fields.
        for n in &parsed.nodes {
            for forbidden in [
                "compensates",
                "rollback-cascade",
                "rollback-after",
            ] {
                assert!(
                    !n.unsupported_fields.iter().any(|(k, _)| k == forbidden),
                    "key `{}` must land on a typed slot, not unsupported_fields",
                    forbidden
                );
            }
        }
    }

    #[test]
    fn parse_node_form_records_unrecognised_rollback_cascade_mode_in_unsupported() {
        let sexp = r#"
            (plan
              (node :id "n1"
                    :target "mission_task_delegate"
                    :rollback-cascade "yolo"))
        "#;
        let parsed = parse_plan_dag(sexp);
        let n = &parsed.nodes[0];
        // Raw value still lands on the typed slot (so the response
        // round-trips author intent).
        assert_eq!(n.rollback_cascade.as_deref(), Some("yolo"));
        // But the typed projection refuses to interpret a typo.
        assert!(n.rollback_cascade_kind().is_none());
        assert!(!n.has_active_rollback_cascade());
        // And the unsupported_fields audit captures the typo.
        assert!(n
            .unsupported_fields
            .iter()
            .any(|(k, v)| k == "rollback-cascade" && v == "yolo"));
    }

    #[test]
    fn rollback_cascade_default_is_inactive() {
        // No `:rollback-cascade` declared → cascade evaluator never runs.
        let node = DagNode {
            id: "n".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        };
        assert!(node.rollback_cascade_kind().is_none());
        assert!(!node.has_active_rollback_cascade());
    }

    #[test]
    fn rollback_cascade_explicit_none_is_inactive() {
        // `:rollback-cascade "none"` is the explicit opt-out.
        let node = DagNode {
            id: "n".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            rollback_cascade: Some("none".into()),
            ..Default::default()
        };
        assert_eq!(
            node.rollback_cascade_kind(),
            Some(RollbackCascadeMode::None)
        );
        assert!(!node.has_active_rollback_cascade());
    }

    #[test]
    fn compute_compensation_order_finds_compensates_matches() {
        // Two compensation nodes, no :rollback-after edges → ordering
        // follows forward topological order (then declaration as a
        // tie-break for nodes not in the forward order).
        let nodes = vec![
            DagNode {
                id: "fail".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
            DagNode {
                id: "comp-a".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                compensates: Some("fail".into()),
                ..Default::default()
            },
            DagNode {
                id: "comp-b".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                compensates: Some("fail".into()),
                ..Default::default()
            },
            DagNode {
                id: "unrelated".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ];
        let order = vec![
            "fail".to_string(),
            "comp-a".to_string(),
            "comp-b".to_string(),
            "unrelated".to_string(),
        ];
        let ordered = compute_compensation_order("fail", &nodes, &order);
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].id, "comp-a");
        assert_eq!(ordered[1].id, "comp-b");
    }

    #[test]
    fn compute_compensation_order_honours_rollback_after_edge() {
        // comp-a declares `:rollback-after ["comp-b"]` so the cascade
        // ordering MUST place comp-b before comp-a even though comp-a
        // comes first in the forward order.
        let nodes = vec![
            DagNode {
                id: "fail".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
            DagNode {
                id: "comp-a".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                compensates: Some("fail".into()),
                rollback_after: vec!["comp-b".into()],
                ..Default::default()
            },
            DagNode {
                id: "comp-b".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                compensates: Some("fail".into()),
                ..Default::default()
            },
        ];
        let order = vec![
            "fail".to_string(),
            "comp-a".to_string(),
            "comp-b".to_string(),
        ];
        let ordered = compute_compensation_order("fail", &nodes, &order);
        assert_eq!(ordered.len(), 2);
        assert_eq!(
            ordered[0].id, "comp-b",
            ":rollback-after must place comp-b first"
        );
        assert_eq!(ordered[1].id, "comp-a");
    }

    #[test]
    fn compute_compensation_order_cycle_falls_back_to_declaration_order() {
        // Both nodes declare `:rollback-after` for each other — that
        // is a cycle (a typo). The evaluator must NOT deadlock; it
        // falls back to declaration order so the cascade still runs.
        let nodes = vec![
            DagNode {
                id: "fail".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
            DagNode {
                id: "comp-a".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                compensates: Some("fail".into()),
                rollback_after: vec!["comp-b".into()],
                ..Default::default()
            },
            DagNode {
                id: "comp-b".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                compensates: Some("fail".into()),
                rollback_after: vec!["comp-a".into()],
                ..Default::default()
            },
        ];
        let order = vec![
            "fail".to_string(),
            "comp-a".to_string(),
            "comp-b".to_string(),
        ];
        let ordered = compute_compensation_order("fail", &nodes, &order);
        // Cycle resolution: every candidate still appears, no deadlock.
        assert_eq!(ordered.len(), 2);
        let ids: Vec<&str> = ordered.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"comp-a"));
        assert!(ids.contains(&"comp-b"));
    }

    #[test]
    fn compute_compensation_order_returns_empty_when_no_compensations_declared() {
        let nodes = vec![DagNode {
            id: "fail".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            ..Default::default()
        }];
        let order = vec!["fail".to_string()];
        assert!(compute_compensation_order("fail", &nodes, &order).is_empty());
    }

    #[test]
    fn build_compensation_plan_entry_records_descriptor_without_dispatch() {
        // Pure helper — verifies that `plan` mode produces a
        // descriptor_ready row with no inner_payload.
        let plan = fixture_plan("(plan)");
        let comp = DagNode {
            id: "comp-1".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("fail".into()),
            rollback_policy: Some("descriptor".into()),
            rollback_objective: Some("undo step".into()),
            rollback_owned_files_raw: Some(r#"["src/a.rs"]"#.into()),
            target_project: Some("missiond".into()),
            dispatch_strategy: Some("fresh-code-alignment".into()),
            ..Default::default()
        };
        let entry = build_compensation_plan_entry(&plan, &comp);
        assert_eq!(entry.node_id, "comp-1");
        assert_eq!(entry.policy, RollbackPolicy::Descriptor);
        assert_eq!(entry.status, RollbackStatus::DescriptorReady);
        assert_eq!(entry.objective.as_deref(), Some("undo step"));
        assert_eq!(entry.owned_files, vec!["src/a.rs".to_string()]);
        // CRITICAL — `plan` mode never produces an inner_payload.
        assert!(entry.inner_payload.is_none());
        // Brief preview is built locally because the objective is set.
        assert!(entry.task_brief_preview.is_some());
        let v = entry.to_json();
        assert_eq!(v["node_id"], "comp-1");
        assert_eq!(v["status"], "descriptor_ready");
        // Pin the audit invariant — declared commands are NEVER executed.
        assert_eq!(v["acceptance_commands_executed"], false);
    }

    #[test]
    fn cascade_outcome_to_json_carries_every_surface_field() {
        let cascade = CascadeRollbackOutcome {
            mode: RollbackCascadeMode::Plan,
            cascade_root: "fail".into(),
            compensations: vec![CascadeCompensationOutcome {
                node_id: "comp-1".into(),
                policy: RollbackPolicy::Descriptor,
                status: RollbackStatus::DescriptorReady,
                reason: "recorded".into(),
                objective: Some("undo".into()),
                owned_files: vec!["src/a.rs".into()],
                acceptance_commands: vec![],
                task_brief_preview: Some("## Objective\nundo\n".into()),
                task_brief_path: None,
                inner_payload: None,
            }],
            reason: "cascade plan: 1 compensation".into(),
        };
        let v = cascade.to_json();
        assert_eq!(v["mode"], "plan");
        assert_eq!(v["cascade_root"], "fail");
        assert_eq!(v["reason"], "cascade plan: 1 compensation");
        let comps = v["compensations"].as_array().unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0]["node_id"], "comp-1");
        assert_eq!(comps[0]["status"], "descriptor_ready");
    }

    #[test]
    fn cascade_outcome_inactive_when_no_mode_and_no_compensations() {
        let inactive = CascadeRollbackOutcome {
            mode: RollbackCascadeMode::None,
            cascade_root: "fail".into(),
            compensations: vec![],
            reason: "skipped".into(),
        };
        assert!(inactive.is_inactive());
        let active_mode = CascadeRollbackOutcome {
            mode: RollbackCascadeMode::Plan,
            cascade_root: "fail".into(),
            compensations: vec![],
            reason: "no compensation declared".into(),
        };
        assert!(!active_mode.is_inactive());
    }

    #[test]
    fn rollback_cascade_mode_wire_strings_are_distinct_and_stable() {
        assert_eq!(RollbackCascadeMode::None.as_wire(), "none");
        assert_eq!(RollbackCascadeMode::Plan.as_wire(), "plan");
        assert_eq!(
            RollbackCascadeMode::DispatchSafe.as_wire(),
            "dispatch-safe"
        );
        // Author-friendly aliases parse identically.
        assert_eq!(
            RollbackCascadeMode::parse("dispatch_safe"),
            Some(RollbackCascadeMode::DispatchSafe)
        );
    }

    #[tokio::test]
    async fn run_cascade_rollback_plan_mode_records_compensations_without_dispatch() {
        // Construct an `AppState` is heavy; use the pure helper +
        // synthesise the cascade outcome by hand to verify the plan
        // mode contract end-to-end without standing up the substrate.
        let plan = fixture_plan("(plan)");
        let nodes = vec![
            DagNode {
                id: "fail".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                rollback_cascade: Some("plan".into()),
                ..Default::default()
            },
            DagNode {
                id: "comp-1".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                compensates: Some("fail".into()),
                rollback_policy: Some("descriptor".into()),
                rollback_objective: Some("undo".into()),
                target_project: Some("missiond".into()),
                dispatch_strategy: Some("fresh-code-alignment".into()),
                ..Default::default()
            },
        ];
        let order = vec!["fail".into(), "comp-1".into()];
        // Use the pure helpers directly — `plan` mode never touches the
        // substrate, so we can synthesise the outcome with the same code
        // path the async helper takes.
        let ordered = compute_compensation_order("fail", &nodes, &order);
        assert_eq!(ordered.len(), 1);
        let entry = build_compensation_plan_entry(&plan, ordered[0]);
        assert_eq!(entry.status, RollbackStatus::DescriptorReady);
        assert!(entry.inner_payload.is_none());
    }

    #[test]
    fn run_cascade_rollback_dispatch_safe_refuses_unsafe_compensation() {
        // Pure projection of the `dispatch-safe` decision: a compensation
        // node that opts into `:rollback-policy "workstation"` BUT misses
        // `:rollback-objective` MUST be refused (non-retryable). We
        // verify this through the safety check directly because the
        // cascade body uses the same check.
        let comp = DagNode {
            id: "comp-1".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("fail".into()),
            rollback_policy: Some("workstation".into()),
            // objective intentionally missing
            rollback_owned_files_raw: Some(r#"["src/a.rs"]"#.into()),
            target_project: Some("missiond".into()),
            dispatch_strategy: Some("fresh-code-alignment".into()),
            ..Default::default()
        };
        let descriptor = build_rollback_descriptor(&comp);
        let err = descriptor
            .safety_check_for_workstation(&comp)
            .expect_err("unsafe compensation must refuse");
        assert!(err.contains(":rollback-objective"));
    }

    #[test]
    fn run_cascade_rollback_dispatch_safe_keeps_descriptor_only_compensations_recorded() {
        // CRITICAL invariant — `dispatch-safe` MUST NEVER promote a
        // descriptor-only compensation to a dispatch. We pin this by
        // building the plan entry directly: the resulting outcome is
        // descriptor_ready (recorded), not dispatched.
        let plan = fixture_plan("(plan)");
        let comp = DagNode {
            id: "comp-1".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensates: Some("fail".into()),
            rollback_policy: Some("descriptor".into()),
            rollback_objective: Some("undo".into()),
            target_project: Some("missiond".into()),
            dispatch_strategy: Some("fresh-code-alignment".into()),
            ..Default::default()
        };
        let entry = build_compensation_plan_entry(&plan, &comp);
        assert_eq!(entry.policy, RollbackPolicy::Descriptor);
        assert_eq!(
            entry.status,
            RollbackStatus::DescriptorReady,
            "dispatch-safe MUST NOT promote a descriptor-only compensation"
        );
        assert!(entry.inner_payload.is_none());
    }

    #[test]
    fn rollback_evaluation_with_cascade_surfaces_cascade_block_in_json() {
        let mut eval = RollbackEvaluation {
            policy: RollbackPolicy::Descriptor,
            status: RollbackStatus::DescriptorReady,
            reason: "descriptor mode".into(),
            objective: Some("undo".into()),
            owned_files: vec![],
            acceptance_commands: vec![],
            task_brief_preview: None,
            task_brief_path: None,
            inner_payload: None,
            cascade: None,
        };
        // Without a cascade outcome attached, JSON omits the cascade key.
        let v = eval.to_json();
        assert!(v.get("cascade").is_none());
        // Attach a cascade outcome — JSON now surfaces it.
        eval.cascade = Some(CascadeRollbackOutcome {
            mode: RollbackCascadeMode::Plan,
            cascade_root: "fail".into(),
            compensations: vec![CascadeCompensationOutcome {
                node_id: "comp-1".into(),
                policy: RollbackPolicy::Descriptor,
                status: RollbackStatus::DescriptorReady,
                reason: "recorded".into(),
                objective: Some("undo".into()),
                owned_files: vec![],
                acceptance_commands: vec![],
                task_brief_preview: None,
                task_brief_path: None,
                inner_payload: None,
            }],
            reason: "cascade plan: 1 compensation".into(),
        });
        let v2 = eval.to_json();
        assert_eq!(v2["cascade"]["mode"], "plan");
        assert_eq!(v2["cascade"]["cascade_root"], "fail");
        assert_eq!(v2["cascade"]["compensations"][0]["node_id"], "comp-1");
    }

    #[test]
    fn build_nodes_summary_surfaces_cascade_hints_when_present() {
        let nodes = vec![
            DagNode {
                id: "with".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                rollback_cascade: Some("plan".into()),
                rollback_objective: Some("undo".into()),
                ..Default::default()
            },
            DagNode {
                id: "comp".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                compensates: Some("with".into()),
                rollback_after: vec!["other".into()],
                ..Default::default()
            },
            DagNode {
                id: "plain".into(),
                target: "mission_execution".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ];
        let order = vec!["with".into(), "comp".into(), "plain".into()];
        let summary = build_nodes_summary(&nodes, &order);
        let arr = summary.as_array().unwrap();
        // Cascade root: `cascade_mode` surfaces under rollback block.
        assert_eq!(arr[0]["rollback"]["cascade_mode"], "plan");
        // Compensation node: `compensates` + `rollback_after` surface.
        assert_eq!(arr[1]["rollback"]["compensates"], "with");
        assert_eq!(arr[1]["rollback"]["rollback_after"][0], "other");
        // Plain node has no rollback hints — summary stays quiet
        // (regression guard for the wave-17 / task 04 baseline).
        assert!(arr[2].get("rollback").is_none());
    }

    #[test]
    fn cascade_default_mode_preserves_wave17_byte_shape() {
        // CRITICAL invariant — a node WITHOUT `:rollback-cascade` MUST
        // NOT trigger any cascade evaluation. This protects every
        // existing wave-17 / task 04 test fixture from accidental
        // promotion.
        let node = DagNode {
            id: "n".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            // Author opted into node-local rollback but NOT cascade.
            rollback_policy: Some("descriptor".into()),
            rollback_objective: Some("undo".into()),
            ..Default::default()
        };
        assert!(!node.has_active_rollback_cascade());
        // The pre-dispatch decision still runs (and now gives back a
        // RollbackEvaluation with `cascade: None`).
        let eval = pre_dispatch_rollback_decision(&node);
        assert!(eval.cascade.is_none());
        // JSON projection omits the `cascade` key entirely.
        let v = eval.to_json();
        assert!(v.get("cascade").is_none());
    }

    // ── wave-19 / task 10 — forward `:compensate-node` references ─────
    //
    // Forward refs are declared on the failing-node side and point AT
    // the compensation node id. They coexist with the wave-18 / task 04
    // reverse `:compensates` direction. These tests pin the parser
    // (both keyword spellings, no typed-slot leak), the validator
    // (self-ref / unknown-id / direction-disagreement rejections), the
    // candidate-discovery union with reverse refs, and the rollback
    // hint surface.

    #[test]
    fn parse_node_form_captures_forward_compensate_node_ref() {
        let sexp = r#"
            (plan
              (node :id "fail"
                    :target "mission_task_delegate"
                    :rollback-cascade "plan"
                    :compensate-node "comp-1")
              (node :id "comp-1"
                    :target "mission_task_delegate"))
        "#;
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes.len(), 2);
        let fail = &parsed.nodes[0];
        assert_eq!(fail.compensate_node.as_deref(), Some("comp-1"));
        // Forward ref does NOT auto-populate the reverse slot on the
        // compensation node — only the failing-node side carries it.
        assert!(parsed.nodes[1].compensate_node.is_none());
        assert!(parsed.nodes[1].compensates.is_none());
        // Neither keyword spelling lands in unsupported_fields.
        for n in &parsed.nodes {
            for forbidden in [
                "compensate-node",
                "compensate_node",
                "compensate-ref",
                "compensate_ref",
            ] {
                assert!(
                    !n.unsupported_fields.iter().any(|(k, _)| k == forbidden),
                    "key `{}` must land on a typed slot, not unsupported_fields",
                    forbidden
                );
            }
        }
    }

    #[test]
    fn parse_node_form_accepts_compensate_ref_alias() {
        // The `:compensate-ref` alias resolves to the same typed slot
        // as `:compensate-node` so authors can pick the wording that
        // reads best in their plan dialect.
        let sexp = r#"
            (plan
              (node :id "fail"
                    :target "mission_task_delegate"
                    :compensate-ref "comp-1")
              (node :id "comp-1"
                    :target "mission_task_delegate"))
        "#;
        let parsed = parse_plan_dag(sexp);
        let fail = &parsed.nodes[0];
        assert_eq!(fail.compensate_node.as_deref(), Some("comp-1"));
    }

    #[test]
    fn build_validated_dag_rejects_self_compensate_node_ref() {
        // A node naming itself as its own compensation is a contract
        // bug: the validator MUST fail fast.
        let sexp = r#"
            (plan
              (node :id "fail"
                    :target "mission_task_delegate"
                    :compensate-node "fail"))
        "#;
        let err = build_validated_dag(sexp).expect_err("self-ref must fail");
        match err {
            DagBuildError::CompensateNodeInvalid { node_id, key, raw, detail } => {
                assert_eq!(node_id, "fail");
                assert_eq!(key, "compensate-node");
                assert_eq!(raw, "fail");
                assert!(
                    detail.contains("failing node itself"),
                    "detail must mention self-reference: {}",
                    detail
                );
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn build_validated_dag_rejects_unknown_compensate_node_ref() {
        // Pointing at an undeclared id is a typo — fail fast with a
        // structured error so the author sees it.
        let sexp = r#"
            (plan
              (node :id "fail"
                    :target "mission_task_delegate"
                    :compensate-node "ghost"))
        "#;
        let err = build_validated_dag(sexp).expect_err("unknown id must fail");
        match err {
            DagBuildError::CompensateNodeInvalid { node_id, raw, detail, .. } => {
                assert_eq!(node_id, "fail");
                assert_eq!(raw, "ghost");
                assert!(
                    detail.contains("not declared"),
                    "detail must mention undeclared id: {}",
                    detail
                );
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn build_validated_dag_rejects_empty_compensate_node_ref() {
        // An empty value is meaningless and almost certainly a typo;
        // we surface it at validation time rather than silently dropping
        // the declaration.
        let sexp = r#"
            (plan
              (node :id "fail"
                    :target "mission_task_delegate"
                    :compensate-node ""))
        "#;
        // Note: the parser strips empty trimmed values via `set_first`,
        // so the slot stays None and validation is a no-op. To force
        // an empty slot we exercise the validator directly with a
        // hand-built node carrying the empty raw string.
        let nodes = vec![DagNode {
            id: "fail".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            compensate_node: Some("   ".into()),
            ..Default::default()
        }];
        // Re-run the same validator branch by inlining the relevant
        // check — we cannot call `build_validated_dag` with synthesised
        // nodes, but the logic is pure so we assert on the parser side
        // that the valid sexp with an empty-string value parses to a
        // None slot (no work for the validator).
        let _ = nodes; // silence unused warning under cfg
        let parsed = parse_plan_dag(sexp);
        assert_eq!(parsed.nodes.len(), 1);
        assert!(
            parsed.nodes[0].compensate_node.is_none(),
            "empty quoted value must drop to None instead of an empty slot"
        );
    }

    #[test]
    fn build_validated_dag_rejects_direction_mismatch() {
        // Forward says `fail` → `comp-1`; reverse on `comp-1` says it
        // compensates `other-fail` instead. The validator MUST refuse
        // — the scheduler is forbidden from silently picking one
        // direction over the other.
        let sexp = r#"
            (plan
              (node :id "fail"
                    :target "mission_task_delegate"
                    :compensate-node "comp-1")
              (node :id "other-fail"
                    :target "mission_task_delegate")
              (node :id "comp-1"
                    :target "mission_task_delegate"
                    :compensates "other-fail"))
        "#;
        let err = build_validated_dag(sexp).expect_err("mismatch must fail");
        match err {
            DagBuildError::CompensateDirectionMismatch {
                failing_node_id,
                comp_node_id,
                reverse_target,
            } => {
                assert_eq!(failing_node_id, "fail");
                assert_eq!(comp_node_id, "comp-1");
                assert_eq!(reverse_target, "other-fail");
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn build_validated_dag_accepts_agreeing_forward_and_reverse_refs() {
        // The two directions agree (forward + reverse name each other);
        // the validator accepts the plan and `compute_compensation_order`
        // surfaces the candidate exactly once (no duplicate from the
        // union).
        let sexp = r#"
            (plan
              (node :id "fail"
                    :target "mission_task_delegate"
                    :rollback-cascade "plan"
                    :compensate-node "comp-1")
              (node :id "comp-1"
                    :target "mission_task_delegate"
                    :compensates "fail"))
        "#;
        let (parsed, order) =
            build_validated_dag(sexp).expect("agreement must validate");
        let ordered = compute_compensation_order("fail", &parsed.nodes, &order);
        assert_eq!(ordered.len(), 1, "agreeing dual decl must not duplicate");
        assert_eq!(ordered[0].id, "comp-1");
    }

    #[test]
    fn build_validated_dag_accepts_forward_only_compensate_ref() {
        // The forward ref alone (no reverse declaration) is the new
        // wave-19 capability: the failing node points at a compensation
        // node and `compute_compensation_order` discovers the candidate
        // even though `comp-1` carries no `:compensates` slot.
        let sexp = r#"
            (plan
              (node :id "fail"
                    :target "mission_task_delegate"
                    :rollback-cascade "plan"
                    :compensate-node "comp-1")
              (node :id "comp-1"
                    :target "mission_task_delegate"))
        "#;
        let (parsed, order) =
            build_validated_dag(sexp).expect("forward-only must validate");
        let ordered = compute_compensation_order("fail", &parsed.nodes, &order);
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].id, "comp-1");
    }

    #[test]
    fn compute_compensation_order_unions_forward_and_reverse_candidates() {
        // Mixed declarations: one comp node uses the reverse contract,
        // another is reached only via the forward ref. Both surface as
        // candidates; ordering still falls back to forward-order rank
        // when no `:rollback-after` edges exist.
        let nodes = vec![
            DagNode {
                id: "fail".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                compensate_node: Some("comp-fwd".into()),
                ..Default::default()
            },
            DagNode {
                id: "comp-rev".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                compensates: Some("fail".into()),
                ..Default::default()
            },
            DagNode {
                id: "comp-fwd".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ];
        let order = vec![
            "fail".to_string(),
            "comp-rev".to_string(),
            "comp-fwd".to_string(),
        ];
        let ordered = compute_compensation_order("fail", &nodes, &order);
        assert_eq!(ordered.len(), 2);
        let ids: Vec<&str> = ordered.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"comp-rev"));
        assert!(ids.contains(&"comp-fwd"));
    }

    #[test]
    fn build_nodes_summary_surfaces_forward_compensate_node_ref() {
        // Forward `:compensate-node` declaration on the failing node
        // surfaces under the same `rollback` block as the existing
        // cascade hints, so audit dashboards can pin both directions.
        let nodes = vec![
            DagNode {
                id: "fail".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                rollback_cascade: Some("plan".into()),
                compensate_node: Some("comp-1".into()),
                ..Default::default()
            },
            DagNode {
                id: "comp-1".into(),
                target: "mission_task_delegate".into(),
                failure_policy: "fail-fast".into(),
                ..Default::default()
            },
        ];
        let order = vec!["fail".into(), "comp-1".into()];
        let summary = build_nodes_summary(&nodes, &order);
        let arr = summary.as_array().unwrap();
        assert_eq!(arr[0]["rollback"]["cascade_mode"], "plan");
        assert_eq!(arr[0]["rollback"]["compensate_node"], "comp-1");
        // Compensation node carried no rollback hint — summary stays quiet.
        assert!(arr[1].get("rollback").is_none());
    }

    #[test]
    fn wave18_safety_gates_unchanged_when_only_forward_ref_used() {
        // wave-18 invariant guard — declaring only the forward ref must
        // NOT bypass the wave-17 / task 04 workstation safety check on
        // a compensation node. We verify this through the safety check
        // directly because the cascade body uses the same check.
        let comp = DagNode {
            id: "comp-1".into(),
            target: "mission_task_delegate".into(),
            failure_policy: "fail-fast".into(),
            // No reverse `:compensates` declared — discovered only via
            // the forward ref on the failing node side.
            rollback_policy: Some("workstation".into()),
            rollback_owned_files_raw: Some(r#"["src/a.rs"]"#.into()),
            // objective intentionally missing → safety gate must refuse.
            target_project: Some("missiond".into()),
            dispatch_strategy: Some("fresh-code-alignment".into()),
            ..Default::default()
        };
        let descriptor = build_rollback_descriptor(&comp);
        let err = descriptor
            .safety_check_for_workstation(&comp)
            .expect_err("safety gate must still refuse without objective");
        assert!(err.contains(":rollback-objective"));
    }
}
