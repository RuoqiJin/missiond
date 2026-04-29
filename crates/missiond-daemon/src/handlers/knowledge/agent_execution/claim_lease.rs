pub(super) const DEFAULT_LEASE_SECS: i64 = 1800;
pub(super) const MAX_LEASE_SECS: i64 = 24 * 3600;

pub(super) fn scopes_overlap(a: &str, b: &str) -> bool {
    scopes_overlap_pure(a, b)
}

/// wave-17 / task 02 — pure scope-overlap predicate exposed to the
/// PLAN DAG scheduler so claim-lease conflict detection reuses the
/// exact semantics established by wave12-01 (agent_execution::action_claim)
/// and wave16-06 (enforce_scoped_commit_completion).
///
/// Same prefix-match contract: empty strings never overlap; strings match if
/// they are equal OR one is a prefix of the other. Re-exporting this from the
/// facade keeps the `plan_dag.rs` dependency stable while the implementation
/// now lives under the V3 claim-lease surface.
pub(in crate::handlers::knowledge) fn scopes_overlap_pure(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.starts_with(b) || b.starts_with(a)
}
