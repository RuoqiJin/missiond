use std::collections::{HashMap, HashSet};

use super::super::DagNode;

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
pub(in crate::handlers::knowledge::plan_dag) fn compute_compensation_order<'a>(
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
            let forward_match = forward_targets.contains(&n.id.to_ascii_lowercase());
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
    let candidate_ids: HashSet<&str> = candidates.iter().map(|n| n.id.as_str()).collect();
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
