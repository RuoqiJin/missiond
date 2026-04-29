use serde_json::{json, Value};
use std::collections::HashMap;

use super::{DagNode, ParsedDag, RollbackPolicy};

pub(super) fn build_nodes_summary(nodes: &[DagNode], order: &[String]) -> Value {
    let mut by_id: HashMap<&str, &DagNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut out: Vec<Value> = Vec::with_capacity(order.len());
    for id in order {
        let Some(n) = by_id.remove(id.as_str()) else {
            continue;
        };
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
pub(super) fn build_retry_plan(nodes: &[DagNode], order: &[String]) -> Value {
    let by_id: HashMap<&str, &DagNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut out: Vec<Value> = Vec::new();
    for id in order {
        let Some(n) = by_id.get(id.as_str()) else {
            continue;
        };
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
pub(super) fn build_node_hint_summary(parsed: &ParsedDag) -> Value {
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
