//! mission_plan — DAG scheduler v1 (minimal, explicit-node-only).
//!
//! This module is loaded by `mission_plan(action=execute, scheduler_mode="dag_v1")`.
//! It is intentionally separated from `plan.rs` so the v0 single-node runner
//! stays untouched as the default contract.
//!
//! Lisp authority:
//!   - intent-flow.lisp        :: F-intent-alignment-plan-execution-loop ::
//!                                 s6 execution-runner
//!   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//!                                 role plan-runner
//!   - intent-tools.lisp       :: implemented-surface mission_plan ::
//!                                 :execute-contract
//!
//! Scope (v1) — what this scheduler DOES support:
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
//!       - Sequential by topological order (concurrency is future work).
//!       - `failure-policy=fail-fast` (default): the first failure stops the
//!         scheduler and downstream nodes are marked `skipped_upstream_failed`.
//!       - `failure-policy=continue`: failed node taints its transitive
//!         downstream (marked `skipped_upstream_failed`); independent nodes
//!         keep running. Per-node policy applies to that node's own failure.
//!   * `dry_run=true`: returns the planned DAG (nodes + topo order) without
//!     dispatching anything and without writing evidence.
//!   * Evidence sidecar: each executed node appends a `plan_dag_node_dispatch`
//!     entry via `super::plan::append_plan_evidence_entry`.
//!
//! Out of scope (v1) — explicitly NOT supported:
//!   * Concurrent dispatch across independent ready nodes (sequential only).
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

use anyhow::Result;
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

    let (parsed, order) = match build_validated_dag(&plan.sexp_text) {
        Ok(v) => v,
        Err(e) => return Ok(e.into_tool_result()),
    };

    let nodes_summary = build_nodes_summary(&parsed.nodes, &order);
    let node_hint_summary = build_node_hint_summary(&parsed);

    if dry_run {
        return Ok(ToolResult::json_pretty(&json!({
            "status": "dry_run",
            "execute_mode": "internal",
            "scheduler_mode": "dag_v1",
            "runner_status": "dry_run_no_dispatch",
            "plan_id": plan.id,
            "board_task_id": plan.board_task_id,
            "nodes": nodes_summary,
            "topological_order": order,
            "node_hint_summary": node_hint_summary,
        })));
    }

    let outcome = execute_sequential(state, args, plan, &parsed, &order).await?;
    let aggregate_status = outcome.aggregate_status();
    let evidence_path = outcome.evidence_path.clone();
    let evidence_error = outcome.evidence_error.clone();
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
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "runner_status": outcome.runner_status(),
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "nodes": outcome.node_results_json(),
        "topological_order": order,
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

#[derive(Debug, Clone)]
enum NodeState {
    Succeeded,
    Failed { reason: String },
    SkippedUpstreamFailed { failed_dep: String },
    SkippedCondition,
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

async fn execute_sequential(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    parsed: &ParsedDag,
    order: &[String],
) -> Result<ExecutionOutcome> {
    let by_id: HashMap<&str, &DagNode> =
        parsed.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // Reverse-adjacency for failure propagation: who depends on each node.
    let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &parsed.nodes {
        for dep in &n.depends_on {
            succs.entry(dep.as_str()).or_default().push(n.id.as_str());
        }
    }
    let mut tainted_by: HashMap<String, String> = HashMap::new();
    let mut outcome = ExecutionOutcome::default();

    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());

    'outer: for id in order {
        if let Some(failed_dep) = tainted_by.get(id).cloned() {
            outcome.results.push(NodeResult {
                id: id.clone(),
                target: by_id
                    .get(id.as_str())
                    .map(|n| n.target.clone())
                    .unwrap_or_default(),
                state: NodeState::SkippedUpstreamFailed { failed_dep },
                dispatch_strategy: "unknown".to_string(),
                inner_payload: Value::Null,
            });
            continue;
        }
        let Some(node) = by_id.get(id.as_str()).copied() else { continue };

        // Conditions are not evaluated in v1; non-empty `:condition` forces a
        // structured skip so authors notice the gating field is honoured.
        if node
            .condition
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            outcome.results.push(NodeResult {
                id: id.clone(),
                target: node.target.clone(),
                state: NodeState::SkippedCondition,
                dispatch_strategy: node
                    .dispatch_strategy
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                inner_payload: Value::Null,
            });
            propagate_taint(node, &succs, &mut tainted_by);
            continue;
        }

        let inner_args_built = build_node_inner_args(node, plan);
        let dispatch_strategy = inner_args_built.dispatch_strategy.clone();
        let inner_args = match inner_args_built.inner_args {
            Ok(v) => v,
            Err(err_payload) => {
                outcome.results.push(NodeResult {
                    id: id.clone(),
                    target: node.target.clone(),
                    state: NodeState::Failed {
                        reason: err_payload
                            .as_object()
                            .and_then(|m| m.get("error"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("inner args build failed")
                            .to_string(),
                    },
                    dispatch_strategy: dispatch_strategy.clone(),
                    inner_payload: err_payload,
                });
                if node.failure_policy == FAILURE_POLICY_FAIL_FAST {
                    outcome.aborted_fail_fast = true;
                    propagate_taint(node, &succs, &mut tainted_by);
                    break 'outer;
                }
                propagate_taint(node, &succs, &mut tainted_by);
                continue;
            }
        };

        let inner_result = match node.target.as_str() {
            "mission_execution" => {
                super::agent_execution::handle(state, "mission_execution", inner_args.clone()).await?
            }
            "mission_task_delegate" => {
                super::super::compute::task_delegate::handle(
                    state,
                    "mission_task_delegate",
                    inner_args.clone(),
                )
                .await?
            }
            "mission_flow_run" => {
                super::super::compute::flow_run::handle(state, "mission_flow_run", inner_args.clone())
                    .await?
            }
            _ => unreachable!("DAG validation already enforced target whitelist"),
        };

        let inner_payload = tool_result_payload(&inner_result);
        let inner_is_error = inner_result.is_error.unwrap_or(false);

        // Append per-node evidence. Both the success and failure branches
        // route through the typed evidence collector — the only differences
        // are the `state_transition` annotation and whether the inner JSON
        // lands under `inner_dispatch` (success) or the legacy `inner_error`
        // extra slot (failure).
        //
        // The legacy wire-form preserved fields:
        //   * `kind="plan_dag_node_dispatch"` is now mapped to
        //     `source=plan_dag_node_dispatch` + canonical `kind="dispatch"`
        //     (matches the wave-12 typed collector contract).
        //   * `scheduler_mode`, `node_id`, `target_tool`, and
        //     `dispatch_strategy` keep their flat-top-level placement so
        //     existing audit dashboards do not need to traverse the new
        //     `inner_dispatch` wrapper.
        //   * On the failure branch the inner payload remains discoverable as
        //     `inner_error` (legacy key) for byte-compat; on the success
        //     branch the inner payload moves to `inner_dispatch` (canonical
        //     typed key) and the typed setter is authoritative.
        //
        // Each node also carries one `EventRef::unavailable(...)` placeholder
        // so consumers can distinguish "no events at all" from "we tried to
        // correlate but the bus subscription is not yet wired" — the DAG
        // scheduler does not yet plumb live ExecutionEvent ids through.
        if !inner_is_error {
            let entry = EvidenceEntry::new(
                evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
                evidence_collector::kind::DISPATCH,
            )
            .with_inner_dispatch(inner_payload.clone())
            .with_state_transition("ready -> succeeded")
            .add_execution_event(EventRef::unavailable(
                "plan_dag scheduler v1 does not yet subscribe to the live \
                 ExecutionEvent bus; caller correlates by plan_id + node_id",
            ))
            .with_extra("scheduler_mode", json!("dag_v1"))
            .with_extra("node_id", json!(node.id))
            .with_extra("plan_id", json!(plan.id))
            .with_extra("target_tool", json!(node.target))
            .with_extra("target", json!(node.target))
            .with_extra("dispatch_strategy", json!(dispatch_strategy))
            // Legacy `inner_result` alias: pre-wave12 sidecars carried the
            // success payload under `inner_result`; the canonical typed slot
            // is `inner_dispatch` (set above). We keep BOTH so historical
            // readers (audit dashboards, retrospective queries) that filter
            // on `inner_result` keep working byte-for-byte.
            .with_extra("inner_result", inner_payload.clone());
            let append_outcome = evidence_collector::append(
                state,
                plan.id,
                project_arg,
                cwd_arg,
                target_project_arg.or(node.target_project.as_deref()),
                entry,
            )
            .await;
            if let AppendOutcome::Failed { error } = &append_outcome {
                tracing::warn!(plan_id = %plan.id, node_id = %node.id, error = %error, "DAG scheduler: evidence append failed");
            }
            let (path, err) = append_outcome.into_legacy_tuple();
            if let Some(p) = path {
                outcome.evidence_path = Some(p);
            }
            if let Some(e) = err {
                outcome.evidence_error = Some(e);
            }

            outcome.results.push(NodeResult {
                id: id.clone(),
                target: node.target.clone(),
                state: NodeState::Succeeded,
                dispatch_strategy,
                inner_payload,
            });
        } else {
            // Inner error — record evidence with explicit failure transition.
            // The inner payload lands under the legacy `inner_error` extra so
            // historical readers that filtered on that key keep working; we
            // intentionally do NOT call `with_inner_dispatch` here so the
            // success vs failure branches produce distinct sidecar shapes.
            let entry = EvidenceEntry::new(
                evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
                evidence_collector::kind::DISPATCH,
            )
            .with_state_transition("ready -> failed")
            .add_execution_event(EventRef::unavailable(
                "plan_dag scheduler v1 does not yet subscribe to the live \
                 ExecutionEvent bus; caller correlates by plan_id + node_id",
            ))
            .with_extra("scheduler_mode", json!("dag_v1"))
            .with_extra("node_id", json!(node.id))
            .with_extra("plan_id", json!(plan.id))
            .with_extra("target_tool", json!(node.target))
            .with_extra("target", json!(node.target))
            .with_extra("dispatch_strategy", json!(dispatch_strategy))
            .with_extra("inner_error", inner_payload.clone());
            let append_outcome = evidence_collector::append(
                state,
                plan.id,
                project_arg,
                cwd_arg,
                target_project_arg.or(node.target_project.as_deref()),
                entry,
            )
            .await;
            if let AppendOutcome::Failed { error } = &append_outcome {
                tracing::warn!(plan_id = %plan.id, node_id = %node.id, error = %error, "DAG scheduler: evidence append failed");
            }
            let (path, err) = append_outcome.into_legacy_tuple();
            if let Some(p) = path {
                outcome.evidence_path = Some(p);
            }
            if let Some(e) = err {
                outcome.evidence_error = Some(e);
            }
            outcome.results.push(NodeResult {
                id: id.clone(),
                target: node.target.clone(),
                state: NodeState::Failed {
                    reason: inner_payload
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("inner handler returned error")
                        .to_string(),
                },
                dispatch_strategy,
                inner_payload,
            });
            if node.failure_policy == FAILURE_POLICY_FAIL_FAST {
                outcome.aborted_fail_fast = true;
                propagate_taint(node, &succs, &mut tainted_by);
                break 'outer;
            }
            propagate_taint(node, &succs, &mut tainted_by);
        }
    }

    Ok(outcome)
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
    fn build_dag_success_entry(node: &DagNode, plan: &Plan, dispatch_strategy: &str, inner_payload: Value) -> Value {
        EvidenceEntry::new(
            evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
            evidence_collector::kind::DISPATCH,
        )
        .with_inner_dispatch(inner_payload.clone())
        .with_state_transition("ready -> succeeded")
        .add_execution_event(EventRef::unavailable(
            "plan_dag scheduler v1 does not yet subscribe to the live \
             ExecutionEvent bus; caller correlates by plan_id + node_id",
        ))
        .with_extra("scheduler_mode", json!("dag_v1"))
        .with_extra("node_id", json!(node.id))
        .with_extra("plan_id", json!(plan.id))
        .with_extra("target_tool", json!(node.target))
        .with_extra("target", json!(node.target))
        .with_extra("dispatch_strategy", json!(dispatch_strategy))
        .with_extra("inner_result", inner_payload)
        .into_json()
    }

    fn build_dag_failure_entry(node: &DagNode, plan: &Plan, dispatch_strategy: &str, inner_payload: Value) -> Value {
        EvidenceEntry::new(
            evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
            evidence_collector::kind::DISPATCH,
        )
        .with_state_transition("ready -> failed")
        .add_execution_event(EventRef::unavailable(
            "plan_dag scheduler v1 does not yet subscribe to the live \
             ExecutionEvent bus; caller correlates by plan_id + node_id",
        ))
        .with_extra("scheduler_mode", json!("dag_v1"))
        .with_extra("node_id", json!(node.id))
        .with_extra("plan_id", json!(plan.id))
        .with_extra("target_tool", json!(node.target))
        .with_extra("target", json!(node.target))
        .with_extra("dispatch_strategy", json!(dispatch_strategy))
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

    #[test]
    fn dag_node_dispatch_evidence_records_event_unavailability_reason() {
        let node = fixture_dag_node("n2", "mission_execution");
        let plan = fixture_plan("(plan)");
        let entry = build_dag_success_entry(&node, &plan, "agent-team", json!({"ok": true}));
        let events = entry["execution_events"]
            .as_array()
            .expect("execution_events array present");
        assert_eq!(events.len(), 1, "exactly one placeholder reference per node");
        assert_eq!(events[0]["unavailable"], true);
        let reason = events[0]["unavailable_reason"]
            .as_str()
            .expect("reason recorded as string");
        assert!(
            reason.contains("ExecutionEvent bus"),
            "reason must mention the bus subscription gap so consumers can route on it: {}",
            reason
        );
    }

}
