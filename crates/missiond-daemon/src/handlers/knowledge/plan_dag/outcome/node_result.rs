use serde_json::Value;

use super::super::acceptance::AcceptanceEvaluation;
use super::super::rollback::RollbackEvaluation;
use super::state::NodeState;

/// wave-16 / task 05 — `Default` is implemented to make wave-13/14/15
/// test fixtures resilient against the retry-bookkeeping fields added
/// in this wave. Production construction sites (`execute_with_concurrency`
/// + the `NodeResult::skipped` helper) always populate every field
/// explicitly; the default impl only catches test fixtures using
/// `..Default::default()` so adding a new bookkeeping field doesn't
/// require touching every old test.
#[derive(Debug, Clone)]
pub(in crate::handlers::knowledge::plan_dag) struct NodeResult {
    pub(in crate::handlers::knowledge::plan_dag) id: String,
    pub(in crate::handlers::knowledge::plan_dag) target: String,
    pub(in crate::handlers::knowledge::plan_dag) state: NodeState,
    pub(in crate::handlers::knowledge::plan_dag) dispatch_strategy: String,
    pub(in crate::handlers::knowledge::plan_dag) inner_payload: Value,
    /// wave-16 / task 05 — number of dispatch attempts the scheduler
    /// actually consumed for this node. Always ≥ 1 for executed nodes
    /// (we count the first dispatch as attempt 1); equals
    /// `effective_max_attempts` only when every attempt failed. Skipped
    /// / paused nodes report `0` because the scheduler never invoked
    /// the inner handler. Surfaces on `node_results[].retry.attempts`.
    pub(in crate::handlers::knowledge::plan_dag) attempts_made: u32,
    /// wave-16 / task 05 — total attempts the scheduler was authorised
    /// to make for this node (= `effective_max_attempts` at dispatch
    /// time). Echoed alongside `attempts_made` so consumers can spot
    /// "exhausted retries" without re-deriving the policy.
    pub(in crate::handlers::knowledge::plan_dag) max_attempts: u32,
    /// wave-16 / task 05 — true iff the node failed without retrying
    /// because the failure was classified non-retryable (currently:
    /// safe-descriptor refusals from the workstation-dispatch
    /// substrate). Surfaces on the per-node response so consumers can
    /// distinguish "we exhausted attempts" from "we refused to retry".
    pub(in crate::handlers::knowledge::plan_dag) retry_skipped_non_retryable: bool,
    /// wave-17 / task 04 — conservative rollback decision result.
    /// `None` means the rollback evaluator never ran (skipped node,
    /// node terminated successfully, or the failed node carried no
    /// rollback hints — see `RollbackEvaluation::is_inactive`).
    /// `Some(e)` carries the full evaluation block — the scheduler
    /// stamps it onto `node_results[].rollback` so callers see what
    /// happened (descriptor recorded / dispatch attempted / refused
    /// / failed) without re-deriving from evidence.
    pub(in crate::handlers::knowledge::plan_dag) rollback: Option<RollbackEvaluation>,
    /// wave-17 / task 03 — deterministic acceptance phase result.
    /// `None` means the acceptance evaluator never ran for this node
    /// (skipped node, dispatch failed before acceptance, no hints
    /// declared). `Some(e)` carries the full evaluation block — the
    /// scheduler stamps it onto `node_results[].acceptance` so callers
    /// see what the evaluator decided + why.
    pub(in crate::handlers::knowledge::plan_dag) acceptance: Option<AcceptanceEvaluation>,
}

impl NodeResult {
    /// wave-16 / task 05 — minimal builder used by skip / pause sites
    /// that never invoked the inner handler. Keeps construction local
    /// to the scheduler so the per-call-site retry bookkeeping
    /// (`attempts_made = 0`, `max_attempts = 1`) stays consistent.
    pub(in crate::handlers::knowledge::plan_dag) fn skipped(
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
