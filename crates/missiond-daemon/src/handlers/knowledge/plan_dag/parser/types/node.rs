use super::super::super::acceptance::{AcceptanceMode, AcceptanceRequires};

pub(in crate::handlers::knowledge::plan_dag::parser) const VALID_TARGETS: &[&str] = &[
    "mission_execution",
    "mission_task_delegate",
    "mission_flow_run",
];

pub(in crate::handlers::knowledge::plan_dag) const FAILURE_POLICY_FAIL_FAST: &str = "fail-fast";
pub(in crate::handlers::knowledge::plan_dag::parser) const FAILURE_POLICY_CONTINUE: &str =
    "continue";

/// wave-16 / task 05 — retry policy ceiling.
///
/// `:retry-count` (alias `:max-attempts`) is interpreted as **additional**
/// attempts beyond the first. The scheduler always runs attempt 1; every
/// retry hint adds N more attempts on top, capped here so a runaway plan
/// (`:retry-count 9999`) cannot melt the dispatch loop. The cap matches
/// the safe-default the wave brief calls out (max attempts = 3 → at most
/// two retries after the first attempt).
pub(in crate::handlers::knowledge::plan_dag) const MAX_NODE_ATTEMPTS_CAP: u32 = 3;

/// wave-16 / task 05 — upper bound on the optional `:retry-delay-ms`
/// pause between attempts. We cap at 60 seconds to keep an authoring
/// mistake (`:retry-delay-ms 999999999`) from stalling the entire wave
/// scheduler. Authors that legitimately need longer back-offs should
/// model that as a separate plan node, not a per-node sleep.
pub(in crate::handlers::knowledge::plan_dag) const MAX_RETRY_DELAY_MS: u64 = 60_000;

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
    pub(in crate::handlers::knowledge::plan_dag) fn workstation_dispatch_opt_in(&self) -> bool {
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
    pub(in crate::handlers::knowledge::plan_dag) fn review_gate_kind(&self) -> ReviewGateKind {
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
    pub(in crate::handlers::knowledge::plan_dag) fn effective_max_attempts(&self) -> u32 {
        let extra = self.retry_count.unwrap_or(0);
        let total = extra.saturating_add(1);
        total.clamp(1, MAX_NODE_ATTEMPTS_CAP)
    }

    /// True iff the node opted into ≥ 1 retry attempt. Used by the
    /// dry-run / dispatch surface to decide whether to emit a
    /// `retry_plan` entry for this node (we omit nodes with the default
    /// single-attempt contract so the v2 byte-shape stays untouched
    /// for callers that do not opt in).
    pub(in crate::handlers::knowledge::plan_dag) fn retry_enabled(&self) -> bool {
        self.effective_max_attempts() > 1
    }

    /// wave-16 / task 05 — clamp the optional `:retry-delay-ms` to the
    /// safe ceiling. Absent / 0 → `None` so the scheduler skips the
    /// `tokio::time::sleep` entirely (no idle wake-up cost).
    pub(in crate::handlers::knowledge::plan_dag) fn effective_retry_delay_ms(&self) -> Option<u64> {
        self.retry_delay_ms
            .filter(|&n| n > 0)
            .map(|n| n.min(MAX_RETRY_DELAY_MS))
    }

    /// wave-17 / task 03 — typed projection of `:acceptance-mode`. Pure
    /// helper so the scheduler can pivot on the enum without
    /// re-tokenising the raw string. Returns `None` when the author did
    /// not declare a mode OR wrote an unrecognised value (the parser
    /// also pushes unrecognised values into `unsupported_fields`).
    pub(in crate::handlers::knowledge::plan_dag) fn acceptance_mode_kind(
        &self,
    ) -> Option<AcceptanceMode> {
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
    pub(in crate::handlers::knowledge::plan_dag) fn has_acceptance_hints(&self) -> bool {
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
    pub(in crate::handlers::knowledge::plan_dag) fn acceptance_requires_kind(
        &self,
    ) -> Option<AcceptanceRequires> {
        let raw = self.acceptance_requires_raw.as_deref()?.trim();
        if raw.is_empty() {
            return None;
        }
        AcceptanceRequires::parse(raw)
    }

    /// wave-18 / task 03 — true iff this node opted into cross-node
    /// acceptance fan-in (one or more `:acceptance-depends-on` entries
    /// AND a recognised `:acceptance-requires` mode).
    pub(in crate::handlers::knowledge::plan_dag) fn has_acceptance_fan_in(&self) -> bool {
        !self.acceptance_depends_on.is_empty() && self.acceptance_requires_kind().is_some()
    }
}

/// wave-16 / task 04 — typed projection of `:review-gate` for the
/// scheduler. Kept on the parser side so dispatch-time logic can match
/// without re-tokenising the raw string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::handlers::knowledge::plan_dag) enum ReviewGateKind {
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
