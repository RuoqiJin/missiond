use missiond_core::types::PlanStatus;
use serde_json::{json, Value};

use super::acceptance::AcceptanceEvaluation;
use super::rollback::RollbackEvaluation;

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
pub(super) enum NodeState {
    Succeeded,
    Failed {
        reason: String,
    },
    SkippedUpstreamFailed {
        failed_dep: String,
    },
    SkippedCondition,
    /// `failure-policy=fail-fast` aborted the scheduler before this node was
    /// ever ready. Distinct from `SkippedUpstreamFailed` because the failing
    /// upstream is not necessarily a transitive dependency — under fail-fast
    /// every still-pending node is force-skipped once the abort flag flips.
    SkippedFailFastAbort {
        aborter: String,
    },
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
/// intermediate phases (`Pending`, `Ready`, `Claimed`, `Running`) never leak
/// into the response — they live entirely in the scheduler's internal state
/// map.
///
/// `Ready` is the brief moment between the scheduler computing the ready set
/// and dispatching it to the JoinSet. The current loop transitions
/// `Pending -> Claimed -> Running` (wave-17 / task 02 added the explicit
/// `Claimed` step between ready-set selection and JoinSet hand-off so the
/// claim/lease registry can stamp metadata before the inner handler runs).
/// The variant `Ready` is kept in the enum to satisfy the wave-13/02 spec
/// lifecycle list and to leave room for a future scheduler that materialises
/// a persistent ready queue (`#[allow(dead_code)]` is intentional for now).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodeLifecycle {
    Pending,
    #[allow(dead_code)]
    Ready,
    /// wave-17 / task 02 — node has had its claim registered (or
    /// recorded best-effort under `enforce_claims=false`) but the
    /// inner handler has not yet been invoked. Mostly invisible from
    /// the outside: the dispatch path moves through `Claimed` for one
    /// wave-loop cycle before flipping to `Running`. Surfaces on the
    /// `pending -> claimed` evidence row + bus event so observers can
    /// pivot on the new transition without reconstructing it from
    /// `pending -> running` reasoning.
    Claimed,
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
pub(super) struct NodeResult {
    pub(super) id: String,
    pub(super) target: String,
    pub(super) state: NodeState,
    pub(super) dispatch_strategy: String,
    pub(super) inner_payload: Value,
    /// wave-16 / task 05 — number of dispatch attempts the scheduler
    /// actually consumed for this node. Always ≥ 1 for executed nodes
    /// (we count the first dispatch as attempt 1); equals
    /// `effective_max_attempts` only when every attempt failed. Skipped
    /// / paused nodes report `0` because the scheduler never invoked
    /// the inner handler. Surfaces on `node_results[].retry.attempts`.
    pub(super) attempts_made: u32,
    /// wave-16 / task 05 — total attempts the scheduler was authorised
    /// to make for this node (= `effective_max_attempts` at dispatch
    /// time). Echoed alongside `attempts_made` so consumers can spot
    /// "exhausted retries" without re-deriving the policy.
    pub(super) max_attempts: u32,
    /// wave-16 / task 05 — true iff the node failed without retrying
    /// because the failure was classified non-retryable (currently:
    /// safe-descriptor refusals from the workstation-dispatch
    /// substrate). Surfaces on the per-node response so consumers can
    /// distinguish "we exhausted attempts" from "we refused to retry".
    pub(super) retry_skipped_non_retryable: bool,
    /// wave-17 / task 04 — conservative rollback decision result.
    /// `None` means the rollback evaluator never ran (skipped node,
    /// node terminated successfully, or the failed node carried no
    /// rollback hints — see `RollbackEvaluation::is_inactive`).
    /// `Some(e)` carries the full evaluation block — the scheduler
    /// stamps it onto `node_results[].rollback` so callers see what
    /// happened (descriptor recorded / dispatch attempted / refused
    /// / failed) without re-deriving from evidence.
    pub(super) rollback: Option<RollbackEvaluation>,
    /// wave-17 / task 03 — deterministic acceptance phase result.
    /// `None` means the acceptance evaluator never ran for this node
    /// (skipped node, dispatch failed before acceptance, no hints
    /// declared). `Some(e)` carries the full evaluation block — the
    /// scheduler stamps it onto `node_results[].acceptance` so callers
    /// see what the evaluator decided + why.
    pub(super) acceptance: Option<AcceptanceEvaluation>,
}

impl NodeResult {
    /// wave-16 / task 05 — minimal builder used by skip / pause sites
    /// that never invoked the inner handler. Keeps construction local
    /// to the scheduler so the per-call-site retry bookkeeping
    /// (`attempts_made = 0`, `max_attempts = 1`) stays consistent.
    pub(super) fn skipped(
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
            rollback: None,
            acceptance: None,
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
            rollback: None,
            acceptance: None,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct ExecutionOutcome {
    pub(super) results: Vec<NodeResult>,
    /// Set true iff a node with `failure-policy=fail-fast` failed and we
    /// stopped scheduling additional ready nodes.
    pub(super) aborted_fail_fast: bool,
    pub(super) evidence_path: Option<String>,
    pub(super) evidence_error: Option<String>,
    /// Per-transition `PlanNodeStateChanged` bus publish warnings collected
    /// during this run. Bus publish is intentionally non-blocking for the
    /// main dispatch path (durable evidence already lives in the sidecar);
    /// the warnings are surfaced on the response so callers can detect a
    /// degraded observability path without scraping daemon logs. Empty
    /// when every transition published cleanly.
    pub(super) bus_publish_warnings: Vec<String>,
}

impl ExecutionOutcome {
    pub(super) fn node_results_json(&self) -> Value {
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
    pub(super) fn paused_nodes_json(&self) -> Value {
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
    pub(super) fn paused_node_ids(&self) -> Vec<String> {
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
    pub(super) fn review_question_ids(&self) -> Vec<String> {
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
    pub(super) fn skipped_nodes_json(&self) -> Value {
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

    pub(super) fn aggregate_status(&self) -> &'static str {
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

    pub(super) fn runner_status(&self) -> &'static str {
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

    pub(super) fn target_plan_status(&self) -> Option<PlanStatus> {
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
