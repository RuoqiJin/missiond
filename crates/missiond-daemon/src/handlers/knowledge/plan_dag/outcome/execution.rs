use missiond_core::types::PlanStatus;
use serde_json::{json, Value};

use super::node_result::NodeResult;
use super::state::NodeState;

#[derive(Debug, Default)]
pub(in crate::handlers::knowledge::plan_dag) struct ExecutionOutcome {
    pub(in crate::handlers::knowledge::plan_dag) results: Vec<NodeResult>,
    /// Set true iff a node with `failure-policy=fail-fast` failed and we
    /// stopped scheduling additional ready nodes.
    pub(in crate::handlers::knowledge::plan_dag) aborted_fail_fast: bool,
    pub(in crate::handlers::knowledge::plan_dag) evidence_path: Option<String>,
    pub(in crate::handlers::knowledge::plan_dag) evidence_error: Option<String>,
    /// Per-transition `PlanNodeStateChanged` bus publish warnings collected
    /// during this run. Bus publish is intentionally non-blocking for the
    /// main dispatch path (durable evidence already lives in the sidecar);
    /// the warnings are surfaced on the response so callers can detect a
    /// degraded observability path without scraping daemon logs. Empty
    /// when every transition published cleanly.
    pub(in crate::handlers::knowledge::plan_dag) bus_publish_warnings: Vec<String>,
}

impl ExecutionOutcome {
    pub(in crate::handlers::knowledge::plan_dag) fn node_results_json(&self) -> Value {
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
                NodeState::Paused { question_id, .. } => {
                    ("paused", Some(("review_question_id", question_id.clone())))
                }
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
            if r.max_attempts > 1 || r.attempts_made > 1 || r.retry_skipped_non_retryable {
                let mut retry = json!({
                    "attempts": r.attempts_made,
                    "max_attempts": r.max_attempts,
                });
                if r.retry_skipped_non_retryable {
                    retry["non_retryable"] = json!(true);
                }
                e["retry"] = retry;
            }
            // wave-17 / task 03 — acceptance evaluation surface. Quiet
            // (omitted) when the evaluator never ran OR ran but found no
            // hints declared so the wave-16 byte-shape is preserved for
            // callers that did not opt into the acceptance contract.
            if let Some(acc) = r.acceptance.as_ref() {
                if !acc.is_inactive() {
                    e["acceptance"] = acc.to_json();
                }
            }
            // wave-17 / task 04 — rollback evaluation surface. Quiet
            // (omitted) when the rollback evaluator never ran OR
            // produced an inactive evaluation (no hints declared) so
            // the wave-17 / task 03 byte-shape is preserved for
            // callers that did not opt into the rollback contract.
            if let Some(rb) = r.rollback.as_ref() {
                if !rb.is_inactive() {
                    e["rollback"] = rb.to_json();
                }
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
    pub(in crate::handlers::knowledge::plan_dag) fn paused_nodes_json(&self) -> Value {
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
    pub(in crate::handlers::knowledge::plan_dag) fn paused_node_ids(&self) -> Vec<String> {
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
    pub(in crate::handlers::knowledge::plan_dag) fn review_question_ids(&self) -> Vec<String> {
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
    pub(in crate::handlers::knowledge::plan_dag) fn skipped_nodes_json(&self) -> Value {
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
        self.results
            .iter()
            .any(|r| matches!(r.state, NodeState::Failed { .. }))
    }

    fn all_succeeded(&self) -> bool {
        !self.results.is_empty()
            && self
                .results
                .iter()
                .all(|r| matches!(r.state, NodeState::Succeeded))
    }

    pub(in crate::handlers::knowledge::plan_dag) fn aggregate_status(&self) -> &'static str {
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

    pub(in crate::handlers::knowledge::plan_dag) fn runner_status(&self) -> &'static str {
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

    pub(in crate::handlers::knowledge::plan_dag) fn target_plan_status(
        &self,
    ) -> Option<PlanStatus> {
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
