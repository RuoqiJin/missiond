use serde_json::Value;

/// Outcome of dispatching a single node — produced inside the spawned task
/// so the scheduler's main loop can decide success/failure + record evidence
/// without holding any per-node lock during the dispatch itself.
pub(in crate::handlers::knowledge::plan_dag) struct DispatchOutcome {
    pub(in crate::handlers::knowledge::plan_dag) node_id: String,
    pub(in crate::handlers::knowledge::plan_dag) target: String,
    pub(in crate::handlers::knowledge::plan_dag) dispatch_strategy: String,
    pub(in crate::handlers::knowledge::plan_dag) inner_payload: Value,
    /// `Ok(())` when the inner handler returned a non-error tool result;
    /// `Err(reason)` when either inner-args building or the inner handler
    /// surfaced an error. The reason string is what we surface in the
    /// per-node response under `reason` and in the `running -> failed`
    /// evidence entry's failure annotation.
    pub(in crate::handlers::knowledge::plan_dag) classification: std::result::Result<(), String>,
    /// wave-16 / task 05 — true when the failure originated from a
    /// workstation-dispatch safe-descriptor refusal (unsupported
    /// target / project root unresolved / missing objective). These
    /// failures are deterministic policy checks — re-running them
    /// without changing the inputs would refuse identically. The
    /// scheduler honours this flag by skipping the retry loop and
    /// surfacing `retry_skipped_non_retryable=true` on the response.
    pub(in crate::handlers::knowledge::plan_dag) non_retryable: bool,
}
