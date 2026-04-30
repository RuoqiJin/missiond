/// wave-17 / task 03 — deterministic id format used when an acceptance
/// evaluation needs to surface a manual-required pause. Distinct from
/// the wave-16 / task 04 review-gate id format so the wave-17 / task 01
/// resume helper does NOT accidentally re-dispatch acceptance pauses
/// (its validator hard-requires `action=plan-node` AND the node still
/// carrying `:review-gate "question-event"` — neither holds for an
/// acceptance pause).
///
/// Layout: `acceptance:plan:<plan_id>:v<version>:<node_id>`.
pub(in crate::handlers::knowledge::plan_dag) fn derive_acceptance_pause_id(
    plan_id: uuid::Uuid,
    plan_version: i32,
    node_id: &str,
) -> String {
    format!("acceptance:plan:{}:v{}:{}", plan_id, plan_version, node_id)
}
