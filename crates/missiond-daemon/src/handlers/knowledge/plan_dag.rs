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
use missiond_core::event::events::ExecutionEvent;
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

/// One node in the executable DAG. Only fields on the v1 allowlist are kept
/// here; unsupported fields land in `unsupported_fields` and are surfaced via
/// `node_hint_summary` so author intent never disappears silently.
#[derive(Debug, Clone)]
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
    /// Per-node unsupported `:keyword value` pairs, kept in source order.
    pub unsupported_fields: Vec<(String, String)>,
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
        "topological_order": order,
        "concurrency_plan": concurrency_plan,
        "node_hint_summary": node_hint_summary,
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
/// byte-identical for downstream readers; v2 only adds `SkippedFailFastAbort`
/// to distinguish "we never dispatched you because an unrelated upstream
/// failed under fail-fast" from "your direct dependency failed".
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
}

#[derive(Debug, Clone)]
struct NodeResult {
    id: String,
    target: String,
    state: NodeState,
    dispatch_strategy: String,
    inner_payload: Value,
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
            out.push(e);
        }
        Value::Array(out)
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
        // Some nodes may have been skipped without any outright failure
        // (e.g. condition gating). Treat that as partial too.
        "dag_partial"
    }

    fn runner_status(&self) -> &'static str {
        if self.aborted_fail_fast {
            "fail_fast_aborted"
        } else if self.all_succeeded() {
            "all_nodes_dispatched"
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
}

async fn dispatch_node(
    state: AppState,
    plan: Plan,
    node: DagNode,
) -> Result<DispatchOutcome> {
    let inner_args_built = build_node_inner_args(&node, &plan);
    let dispatch_strategy = inner_args_built.dispatch_strategy.clone();
    let inner_args = match inner_args_built.inner_args {
        Ok(v) => v,
        Err(err_payload) => {
            let reason = err_payload
                .as_object()
                .and_then(|m| m.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("inner args build failed")
                .to_string();
            return Ok(DispatchOutcome {
                node_id: node.id.clone(),
                target: node.target.clone(),
                dispatch_strategy,
                inner_payload: err_payload,
                classification: Err(reason),
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
    outcome: &mut ExecutionOutcome,
) {
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        PLAN_NODE_DEFAULT_ATTEMPT,
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
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT));
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
        PLAN_NODE_DEFAULT_ATTEMPT,
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
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT));
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
            results_by_id.insert(
                id.clone(),
                NodeResult {
                    id,
                    target: node.target.clone(),
                    state: state_skip,
                    dispatch_strategy,
                    inner_payload: Value::Null,
                },
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
                results_by_id.insert(
                    id.clone(),
                    NodeResult {
                        id,
                        target: node.target.clone(),
                        state: NodeState::SkippedFailFastAbort {
                            aborter: aborter.clone(),
                        },
                        dispatch_strategy,
                        inner_payload: Value::Null,
                    },
                );
            }
            break;
        }

        // 4. If nothing ready and nothing running, we're done.
        if ready_ids.is_empty() && !any_running {
            break;
        }

        // 5. Filter ready set by condition gate. Nodes with non-empty
        //    `:condition` skip in v2 just like v1 — taint propagated.
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
                    NodeResult {
                        id: id.clone(),
                        target: node.target.clone(),
                        state: NodeState::SkippedCondition,
                        dispatch_strategy,
                        inner_payload: Value::Null,
                    },
                );
                propagate_taint(node, &succs, &mut tainted_by);
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
        let mut join_set: tokio::task::JoinSet<Result<DispatchOutcome>> =
            tokio::task::JoinSet::new();
        for node in to_dispatch {
            let dispatch_strategy = node
                .dispatch_strategy
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            lifecycle.insert(node.id.clone(), NodeLifecycle::Running);
            emit_evidence_running(state, &ctx, &node, &dispatch_strategy, &mut outcome).await;
            let state_clone = state.clone();
            let plan_clone = plan.clone();
            join_set.spawn(async move { dispatch_node(state_clone, plan_clone, node).await });
        }

        // 7. Drain wave; for each result decide success/failure, update
        //    lifecycle + taint, write finish evidence.
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
            } = dispatch_outcome;
            let node = match by_id.get(&node_id) {
                Some(n) => n.clone(),
                None => continue,
            };
            let succeeded = classification.is_ok();
            emit_evidence_finished(
                state,
                &ctx,
                &node,
                &dispatch_strategy,
                &inner_payload,
                succeeded,
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
                    },
                );
            } else {
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
        });
        o.results.push(NodeResult {
            id: "n2".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedUpstreamFailed {
                failed_dep: "n1".into(),
            },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
        });
        o.results.push(NodeResult {
            id: "n3".into(),
            target: "mission_execution".into(),
            state: NodeState::Succeeded,
            dispatch_strategy: "unknown".into(),
            inner_payload: json!({"ok": true}),
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
                objective: None,
                depends_on: vec![],
                condition: None,
                failure_policy: "fail-fast".into(),
                timeout_ms: None,
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
                flow_id: None,
                unsupported_fields: vec![],
            },
            DagNode {
                id: "b".into(),
                target: "mission_execution".into(),
                objective: None,
                depends_on: vec!["a".into()],
                condition: None,
                failure_policy: "fail-fast".into(),
                timeout_ms: None,
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
                flow_id: None,
                unsupported_fields: vec![],
            },
            DagNode {
                id: "c".into(),
                target: "mission_execution".into(),
                objective: None,
                depends_on: vec!["b".into()],
                condition: None,
                failure_policy: "fail-fast".into(),
                timeout_ms: None,
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
                flow_id: None,
                unsupported_fields: vec![],
            },
            DagNode {
                id: "d".into(),
                target: "mission_execution".into(),
                objective: None,
                depends_on: vec!["a".into()],
                condition: None,
                failure_policy: "fail-fast".into(),
                timeout_ms: None,
                dispatch_strategy: None,
                target_project: None,
                requested_cwd: None,
                flow_id: None,
                unsupported_fields: vec![],
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
            depends_on: vec![],
            condition: None,
            failure_policy: "fail-fast".into(),
            timeout_ms: None,
            dispatch_strategy: Some("fresh-code-alignment".into()),
            target_project: Some("missiond".into()),
            requested_cwd: Some("/abs/path".into()),
            flow_id: None,
            unsupported_fields: vec![],
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
            depends_on: vec![],
            condition: None,
            failure_policy: "fail-fast".into(),
            timeout_ms: Some(15_000),
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: Some("/abs/path".into()),
            flow_id: None,
            unsupported_fields: vec![],
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
            objective: None,
            depends_on: vec![],
            condition: None,
            failure_policy: "fail-fast".into(),
            timeout_ms: None,
            dispatch_strategy: None,
            target_project: None,
            requested_cwd: None,
            flow_id: None,
            unsupported_fields: vec![],
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
            objective: None,
            depends_on: vec![],
            condition: None,
            failure_policy: "fail-fast".into(),
            timeout_ms: None,
            dispatch_strategy: Some("agent-team".into()),
            target_project: None,
            requested_cwd: None,
            flow_id: None,
            unsupported_fields: vec![],
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
        });
        o.results.push(NodeResult {
            id: "b".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedUpstreamFailed { failed_dep: "a".into() },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
        });
        o.results.push(NodeResult {
            id: "c".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedCondition,
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
        });
        o.results.push(NodeResult {
            id: "d".into(),
            target: "mission_execution".into(),
            state: NodeState::Failed { reason: "boom".into() },
            dispatch_strategy: "agent-team".into(),
            inner_payload: json!({"error": "boom"}),
        });
        o.results.push(NodeResult {
            id: "e".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedFailFastAbort { aborter: "d".into() },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
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
        });
        o.results.push(NodeResult {
            id: "b".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedFailFastAbort { aborter: "a".into() },
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
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
        });
        o.results.push(NodeResult {
            id: "b".into(),
            target: "mission_execution".into(),
            state: NodeState::SkippedCondition,
            dispatch_strategy: "unknown".into(),
            inner_payload: Value::Null,
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
}
