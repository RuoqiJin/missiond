use std::collections::{BTreeSet, HashMap, HashSet};

use super::super::acceptance::AcceptanceRequires;
use super::scanner::parse_plan_dag;
use super::types::{DagBuildError, DagNode, ParsedDag, VALID_TARGETS};

/// Parse and validate a PLAN.lisp body, returning a topologically-sorted node
/// list ready for sequential dispatch.
pub(in crate::handlers::knowledge::plan_dag) fn build_validated_dag(
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
