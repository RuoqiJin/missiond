use missiond_core::event::events::ExecutionEvent;

use crate::handlers::knowledge::evidence_collector::{self, EventRef};
use crate::state::AppState;

use super::super::DagNode;

/// `EventRef::unavailable` reason kept for the legacy fallback path —
/// publish *and* deterministic-id construction must both fail before we
/// surrender to it. Wave-14 :: Task 02 wires `PlanNodeStateChanged` so the
/// normal path now writes `EventRef::new(...)` either with the live `Seq`
/// from the bus or with the deterministic
/// `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` id when the bus
/// publish fails. The `unavailable` placeholder is unreachable today (the
/// deterministic id is always derivable) but kept on the call surface so
/// the contract `evidence_collector` documents (`unavailable` → "we tried
/// to correlate but couldn't") stays implementable if a future caller
/// genuinely cannot stamp an id.
#[allow(dead_code)]
const EVENT_REF_UNAVAILABLE_REASON: &str =
    "plan_dag scheduler could not derive a live or deterministic \
     ExecutionEvent reference; this is a fallback path";

/// Domain tag used in `EventRef::source` for plan-node lifecycle entries.
/// Mirrors `Domain::Execution::as_str()` (kept as a `&'static str` here so
/// we don't pull the enum reference into every evidence call site).
pub(in crate::handlers::knowledge::plan_dag) const EVENT_REF_SOURCE_EXECUTION: &str = "execution";

/// Kind tag matching `ExecutionEvent::PlanNodeStateChanged.kind()`. Kept
/// duplicated here so test assertions can pin the wire form without taking
/// a dep on the event-trait reflection.
pub(in crate::handlers::knowledge::plan_dag) const EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED: &str =
    "plan_node_state_changed";

/// Build the deterministic event id used as a stable correlation key when
/// the live bus publish either succeeds (used in the publish dedupe context)
/// or fails (used as the `EventRef::event_id`). Format
/// `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` matches the wave-14
/// task brief verbatim so external consumers can grep on it.
pub(in crate::handlers::knowledge::plan_dag) fn deterministic_plan_node_event_id(
    plan_id: uuid::Uuid,
    node_id: &str,
    attempt: u32,
    from: &str,
    to: &str,
) -> String {
    format!(
        "plan-node:{}:{}:{}:{}-{}",
        plan_id, node_id, attempt, from, to
    )
}

/// Build the `PlanNodeStateChanged` payload for a single transition.
/// Pure helper so unit tests can pin the wire shape without standing up an
/// `AppState`.
pub(in crate::handlers::knowledge::plan_dag) fn build_plan_node_state_changed_event(
    plan_id: uuid::Uuid,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    from: &str,
    to: &str,
    reason: Option<String>,
) -> ExecutionEvent {
    ExecutionEvent::PlanNodeStateChanged {
        plan_id: plan_id.to_string(),
        node_id: node.id.clone(),
        from: from.to_string(),
        to: to.to_string(),
        target: Some(node.target.clone()),
        dispatch_strategy: Some(dispatch_strategy.to_string()),
        target_project: node.target_project.clone(),
        attempt: Some(attempt),
        reason,
    }
}

/// Publish a `PlanNodeStateChanged` event and return the `EventRef` to
/// embed in the evidence entry. On bus success we surface the live `Seq` as
/// the event id; on failure we fall back to the deterministic id derived
/// from `plan_id`/`node_id`/`attempt`/`from`/`to` so the audit trail still
/// carries a stable correlation key, and we record a warning string the
/// caller can lift into `outcome.bus_publish_warnings`.
///
/// The function NEVER aborts the dispatch on a publish failure — the
/// scheduler's main loop only consults the returned warning to decide
/// whether to surface `bus_publish_warnings` on the response. This matches
/// the wave-14 / task 02 brief: bus publish failure is observability-only.
pub(in crate::handlers::knowledge::plan_dag) async fn publish_plan_node_state_change(
    state: &AppState,
    plan_id: uuid::Uuid,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    from: &str,
    to: &str,
    reason: Option<String>,
) -> (EventRef, Option<String>) {
    let ev = build_plan_node_state_changed_event(
        plan_id,
        node,
        dispatch_strategy,
        attempt,
        from,
        to,
        reason,
    );
    let deterministic_id = deterministic_plan_node_event_id(plan_id, &node.id, attempt, from, to);
    match state.bus.publish_execution_with_seq(ev).await {
        Ok(seq) => (
            EventRef::new(
                EVENT_REF_SOURCE_EXECUTION,
                EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
                seq.0.to_string(),
            ),
            None,
        ),
        Err(err) => {
            // Wave-17 / task 06 — try the resolver before surrendering to
            // the deterministic-id fallback. The resolver checks its
            // in-memory cache first (a previous attempt for the same
            // transition may already have cached a real `Seq`), then
            // falls through to a bounded read-only scan of the persistent
            // event log so refs survive daemon restarts. Lookup failure
            // NEVER aborts the dispatch — on every error path we keep the
            // deterministic id so the audit trail still carries a stable
            // correlation key.
            let plan_id_str = plan_id.to_string();
            let recovered = state
                .bus
                .event_ref_resolver
                .lookup_or_query_plan_node_state_change(
                    state.bus.log.as_ref(),
                    &plan_id_str,
                    &node.id,
                    attempt,
                    from,
                    to,
                )
                .await;
            if recovered.status == evidence_collector::EventRefStatus::Log {
                let warning = format!(
                    "plan_node_state_changed bus publish failed for {} ({} -> {}): {}; \
                     evidence ref recovered from event log",
                    node.id, from, to, err
                );
                tracing::warn!(
                    plan_id = %plan_id,
                    node_id = %node.id,
                    from = %from,
                    to = %to,
                    error = %err,
                    "DAG scheduler: PlanNodeStateChanged bus publish failed; recovered event ref from log"
                );
                return (recovered, Some(warning));
            }
            let warning = format!(
                "plan_node_state_changed bus publish failed for {} ({} -> {}): {}; \
                 evidence ref falls back to deterministic id `{}`",
                node.id, from, to, err, deterministic_id
            );
            tracing::warn!(
                plan_id = %plan_id,
                node_id = %node.id,
                from = %from,
                to = %to,
                error = %err,
                "DAG scheduler: PlanNodeStateChanged bus publish failed; deterministic event ref retained"
            );
            (
                EventRef::new(
                    EVENT_REF_SOURCE_EXECUTION,
                    EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
                    deterministic_id,
                ),
                Some(warning),
            )
        }
    }
}
