use missiond_core::types::Plan;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

use super::super::plan::{build_internal_dispatch_args, tool_result_payload, ParsedPlanHints};
use super::DagNode;

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
pub(super) fn compute_concurrency_plan(
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
pub(super) fn propagate_taint<'a>(
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
pub(super) struct NodeInnerArgs {
    pub(super) inner_args: std::result::Result<Value, Value>,
    pub(super) dispatch_strategy: String,
}

pub(super) fn build_node_inner_args(node: &DagNode, plan: &Plan) -> NodeInnerArgs {
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
    if let Some(value) = &node.execution_order {
        node_args.insert("execution_order".to_string(), Value::String(value.clone()));
    }
    if let Some(value) = &node.parallel_group {
        node_args.insert("parallel_group".to_string(), Value::String(value.clone()));
    }
    if let Some(value) = node.atom_level {
        node_args.insert("atom_level".to_string(), Value::Number(value.into()));
    }
    if let Some(value) = &node.atom_task_id {
        node_args.insert("atom_task_id".to_string(), Value::String(value.clone()));
    } else if !node.id.trim().is_empty() {
        node_args.insert("atom_task_id".to_string(), Value::String(node.id.clone()));
    }
    if !node.id.trim().is_empty() {
        node_args.insert("atom_path".to_string(), Value::String(node.id.clone()));
    }
    if let Some(value) = &node.predicted_tool_sequence_raw {
        node_args.insert(
            "predicted_tool_sequence".to_string(),
            Value::String(value.clone()),
        );
    }
    if let Some(value) = &node.context_sources_raw {
        node_args.insert("context_sources".to_string(), Value::String(value.clone()));
    }
    let dispatch_strategy = node
        .dispatch_strategy
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let args_value = Value::Object(node_args);

    // Plan-hint slot is empty: each node carries its own hints, so the
    // shared parser is bypassed.
    let empty_hints = ParsedPlanHints::default();
    let inner_args = match build_internal_dispatch_args(
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
        inner_args,
        dispatch_strategy,
    }
}
