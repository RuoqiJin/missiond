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

use super::evidence_collector::{
    self, AppendOutcome, EventRef, EvidenceEntry,
};
use super::plan::{
    build_internal_dispatch_args, tool_result_payload, ParsedPlanHints,
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
    Ok((parsed, order))
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
        workstation_dispatch_flag,
        review_gate,
        review_action,
        review_text,
        retry_count,
        retry_delay_ms,
        retry_parse_error,
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

    let (parsed, order) = match build_validated_dag(&plan.sexp_text) {
        Ok(v) => v,
        Err(e) => return Ok(e.into_tool_result()),
    };

    let nodes_summary = build_nodes_summary(&parsed.nodes, &order);
    let node_hint_summary = build_node_hint_summary(&parsed);
    let concurrency_plan = compute_concurrency_plan(&parsed.nodes, &order, max_parallel_nodes);
    let retry_plan = build_retry_plan(&parsed.nodes, &order);

    if dry_run {
        return Ok(ToolResult::json_pretty(&json!({
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
        })));
    }

    let outcome =
        execute_with_concurrency(state, args, plan, &parsed, &order, max_parallel_nodes).await?;
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
        "evidence_path": evidence_path,
    });
    match plan_status_update {
        Ok(s) => payload["plan_status"] = json!(s),
        Err(e) => payload["status_update_error"] = json!(e),
    }
    if let Some(err) = evidence_error {
        payload["evidence_error"] = json!(err);
    }
    if !bus_publish_warnings.is_empty() {
        payload["bus_publish_warnings"] = json!(bus_publish_warnings);
    }
    Ok(ToolResult::json_pretty(&payload))
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
/// intermediate phases (`Pending`, `Ready`, `Running`) never leak into the
/// response — they live entirely in the scheduler's internal state map.
///
/// `Ready` is the brief moment between the scheduler computing the ready set
/// and dispatching it to the JoinSet. The current loop transitions
/// `Pending -> Running` directly (skipping the explicit `Ready` storage)
/// because the ready set is recomputed each iteration; the variant is kept
/// in the enum to satisfy the wave-13/02 spec lifecycle list and to leave
/// room for a future scheduler that materialises a persistent ready queue
/// (`#[allow(dead_code)]` is intentional for now).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeLifecycle {
    Pending,
    #[allow(dead_code)]
    Ready,
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

async fn dispatch_node(
    state: AppState,
    plan: Plan,
    node: DagNode,
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
        let outcome = super::workstation_dispatch::run_workstation_dispatch(
            &state,
            &plan,
            &node.target,
            &dispatch_strategy,
            merged,
            false,
        )
        .await;
        let (inner_payload, classification, non_retryable) =
            workstation_outcome_to_dispatch_pair(
                &node,
                &dispatch_strategy,
                outcome,
                &dispatch_decision,
            );
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

/// Pre-built immutable evidence parameters that vary per call to
/// `action_execute_dag_v1`. The scheduler captures these once so each
/// per-node evidence emit doesn't re-thread the same args through.
struct EvidenceCtx<'a> {
    plan_id: uuid::Uuid,
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
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("ready -> running")
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
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
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

    let ctx = EvidenceCtx {
        plan_id: plan.id,
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
        let mut join_set: tokio::task::JoinSet<Result<DispatchOutcome>> =
            tokio::task::JoinSet::new();
        for node in to_dispatch {
            let dispatch_strategy = node
                .dispatch_strategy
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            lifecycle.insert(node.id.clone(), NodeLifecycle::Running);
            let attempt = {
                let entry = attempts_made.entry(node.id.clone()).or_insert(0);
                *entry += 1;
                *entry
            };
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
            join_set.spawn(async move { dispatch_node(state_clone, plan_clone, node).await });
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
                lifecycle.insert(node_id.clone(), NodeLifecycle::Succeeded);
                results_by_id.insert(
                    node_id.clone(),
                    NodeResult {
                        id: node_id,
                        target,
                        state: NodeState::Succeeded,
                        dispatch_strategy,
                        inner_payload,
                        attempts_made: current_attempt,
                        max_attempts,
                        retry_skipped_non_retryable: false,
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
                join_set.spawn(async move {
                    dispatch_node(state_clone, plan_clone, node_clone).await
                });
                continue;
            }

            // Final failure — exhausted retries OR non-retryable OR
            // fail-fast already aborted this wave.
            lifecycle.insert(node_id.clone(), NodeLifecycle::Failed);
            let reason = classification
                .err()
                .unwrap_or_else(|| "inner handler returned error".to_string());
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
                },
            );
            // Taint propagates regardless of policy — it just changes
            // whether *unrelated* nodes also get aborted (fail-fast) or
            // can keep running (continue).
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
}
