/// Per-node attempt counter. v2 has no retry policy so every transition
/// reports `attempt=1`; encapsulating the constant in a helper keeps the
/// retry-aware future scheduler a single-call-site change.
pub(in crate::handlers::knowledge::plan_dag) const PLAN_NODE_DEFAULT_ATTEMPT: u32 = 1;

/// wave-16 / task 05 — pure retry decision. Extracted out of the wave
/// loop so the predicate can be unit-tested without standing up an
/// `AppState`. `should_retry` is true iff the failed attempt is not
/// classified non-retryable AND attempts remain AND the wave is not
/// already in fail-fast abort. The wave loop honours this decision
/// deterministically so the tests below pin the contract.
pub(in crate::handlers::knowledge::plan_dag) fn plan_node_should_retry(
    current_attempt: u32,
    max_attempts: u32,
    non_retryable: bool,
    abort_new_dispatch: bool,
) -> bool {
    if non_retryable || abort_new_dispatch {
        return false;
    }
    let attempts_remaining = max_attempts.saturating_sub(current_attempt);
    attempts_remaining > 0
}
