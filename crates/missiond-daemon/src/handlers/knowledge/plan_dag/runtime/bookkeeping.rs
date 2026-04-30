use std::collections::HashMap;

use super::super::outcome::{NodeLifecycle, NodeResult, NodeState};
use super::super::parser::DagNode;

pub(super) fn build_node_map(nodes: &[DagNode]) -> HashMap<String, DagNode> {
    nodes.iter().map(|n| (n.id.clone(), n.clone())).collect()
}

pub(super) fn build_successor_map<'a>(nodes: &'a [DagNode]) -> HashMap<&'a str, Vec<&'a str>> {
    let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in nodes {
        for dep in &node.depends_on {
            succs
                .entry(dep.as_str())
                .or_default()
                .push(node.id.as_str());
        }
    }
    succs
}

pub(super) fn build_topo_index(order: &[String]) -> HashMap<&str, usize> {
    order
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect()
}

pub(super) fn initialize_lifecycle(nodes: &[DagNode]) -> HashMap<String, NodeLifecycle> {
    nodes
        .iter()
        .map(|n| (n.id.clone(), NodeLifecycle::Pending))
        .collect()
}

pub(super) fn collect_tainted_pending(
    order: &[String],
    lifecycle: &HashMap<String, NodeLifecycle>,
    tainted_by: &HashMap<String, String>,
) -> Vec<(String, NodeState)> {
    let mut became_skipped = Vec::new();
    for id in order {
        if !matches!(lifecycle.get(id.as_str()), Some(NodeLifecycle::Pending)) {
            continue;
        }
        if let Some(failed_dep) = tainted_by.get(id.as_str()).cloned() {
            became_skipped.push((id.clone(), NodeState::SkippedUpstreamFailed { failed_dep }));
        }
    }
    became_skipped
}

pub(super) fn compute_ready_ids(
    order: &[String],
    lifecycle: &HashMap<String, NodeLifecycle>,
    by_id: &HashMap<String, DagNode>,
) -> Vec<String> {
    let mut ready_ids = Vec::new();
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
    ready_ids
}

pub(super) fn has_running_nodes(lifecycle: &HashMap<String, NodeLifecycle>) -> bool {
    lifecycle
        .values()
        .any(|s| matches!(s, NodeLifecycle::Running))
}

pub(super) fn pending_ids(
    order: &[String],
    lifecycle: &HashMap<String, NodeLifecycle>,
) -> Vec<String> {
    order
        .iter()
        .filter(|id| matches!(lifecycle.get(id.as_str()), Some(NodeLifecycle::Pending)))
        .cloned()
        .collect()
}

pub(super) fn stitch_results_topologically(
    results_by_id: HashMap<String, NodeResult>,
    topo_index: &HashMap<&str, usize>,
) -> Vec<NodeResult> {
    let mut ordered: Vec<(usize, NodeResult)> = results_by_id
        .into_iter()
        .filter_map(|(id, r)| topo_index.get(id.as_str()).map(|&i| (i, r)))
        .collect();
    ordered.sort_by_key(|(i, _)| *i);
    ordered.into_iter().map(|(_, r)| r).collect()
}
