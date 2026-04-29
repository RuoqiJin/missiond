use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use std::collections::{BTreeSet, HashMap, HashSet};

use super::acceptance::{AcceptanceMode, AcceptanceRequires};
use super::rollback::{RollbackCascadeMode, RollbackPolicy};

const VALID_TARGETS: &[&str] = &[
    "mission_execution",
    "mission_task_delegate",
    "mission_flow_run",
];

pub(super) const FAILURE_POLICY_FAIL_FAST: &str = "fail-fast";
const FAILURE_POLICY_CONTINUE: &str = "continue";

/// wave-16 / task 05 — retry policy ceiling.
///
/// `:retry-count` (alias `:max-attempts`) is interpreted as **additional**
/// attempts beyond the first. The scheduler always runs attempt 1; every
/// retry hint adds N more attempts on top, capped here so a runaway plan
/// (`:retry-count 9999`) cannot melt the dispatch loop. The cap matches
/// the safe-default the wave brief calls out (max attempts = 3 → at most
/// two retries after the first attempt).
pub(super) const MAX_NODE_ATTEMPTS_CAP: u32 = 3;

/// wave-16 / task 05 — upper bound on the optional `:retry-delay-ms`
/// pause between attempts. We cap at 60 seconds to keep an authoring
/// mistake (`:retry-delay-ms 999999999`) from stalling the entire wave
/// scheduler. Authors that legitimately need longer back-offs should
/// model that as a separate plan node, not a per-node sleep.
pub(super) const MAX_RETRY_DELAY_MS: u64 = 60_000;

/// One node in the executable DAG. Only fields on the v1 allowlist are kept
/// here; unsupported fields land in `unsupported_fields` and are surfaced via
/// `node_hint_summary` so author intent never disappears silently.
#[derive(Debug, Clone, Default)]
pub(in crate::handlers::knowledge) struct DagNode {
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
            Some("question-event") | Some("question_event") => ReviewGateKind::QuestionEvent,
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
        !self.acceptance_depends_on.is_empty() && self.acceptance_requires_kind().is_some()
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

/// Result of parsing a PLAN.lisp body for explicit `(node ...)` forms.
#[derive(Debug, Clone, Default)]
pub(in crate::handlers::knowledge) struct ParsedDag {
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
    pub(super) fn into_tool_result(self) -> ToolResult {
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
            && n.acceptance_requires_raw
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && n.acceptance_source_node
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
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
                raw: Some("fan-in declared without :acceptance-depends-on".to_string()),
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
                detail: ":acceptance-requires \"evidence_keys\" requires `:acceptance-source-node`"
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
    let by_id: HashMap<&str, &DagNode> = parsed.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
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
                detail: format!("`{}` is not declared in this plan", trimmed),
            });
        };
        // (c) reverse-direction agreement (only when the comp node ALSO
        //     declared `:compensates`). Compared case-insensitively to
        //     mirror the existing `compute_compensation_order` matching.
        if let Some(reverse_raw) = comp.compensates.as_deref() {
            let reverse = reverse_raw.trim();
            if !reverse.is_empty() && reverse.to_ascii_lowercase() != n.id.to_ascii_lowercase() {
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
fn compute_transitive_ancestors(nodes: &[DagNode]) -> HashMap<String, HashSet<String>> {
    let by_id: HashMap<&str, &DagNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
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
            "failure-policy" | "failure_policy" => set_first(&mut failure_policy, &value),
            "timeout-ms" | "timeout_ms" => {
                if let Ok(n) = value.trim().parse::<i64>() {
                    if timeout_ms.is_none() {
                        timeout_ms = Some(n);
                    }
                }
            }
            "dispatch-strategy" | "dispatch_strategy" => set_first(&mut dispatch_strategy, &value),
            "target-project" | "target_project" | "project" => {
                set_first(&mut target_project, &value)
            }
            "requested-cwd" | "requested_cwd" | "cwd" => set_first(&mut requested_cwd, &value),
            "flow-id" | "flow_id" => set_first(&mut flow_id, &value),
            // wave-15 / task 05 — workstation-dispatch hint contract.
            // Captured here so the scheduler can route eligible nodes
            // without a second parse pass; only consumed when
            // `:workstation-dispatch true` is also set.
            "scope" => set_first(&mut scope, &value),
            "commit-policy" | "commit_policy" => set_first(&mut commit_policy, &value),
            "owned-files" | "owned_files" => set_first(&mut owned_files_raw, &value),
            "forbidden-files" | "forbidden_files" => set_first(&mut forbidden_files_raw, &value),
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
            "compensate-node" | "compensate_node" | "compensate-ref" | "compensate_ref" => {
                set_first(&mut compensate_node, &value)
            }
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
