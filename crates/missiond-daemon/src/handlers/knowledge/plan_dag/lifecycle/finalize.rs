use missiond_core::types::Plan;
use serde_json::{json, Value};

use crate::handlers::knowledge::evidence_collector::{self, AppendOutcome, EvidenceEntry};
use crate::state::AppState;

/// wave-17 / task 05 — append one `dag_finalized` evidence row. Mirrors the
/// per-node evidence layout (same source + kind taxonomy) so audit
/// dashboards that pivot on `state_transition` see the finalize entry next
/// to the per-node entries it summarises. Updates `evidence_path` /
/// `evidence_error` on the response payload so callers see the same
/// freshness signal the per-node writes already provide.
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_dag_finalized(
    state: &AppState,
    plan: &Plan,
    args: &Value,
    aggregate_status: &str,
    plan_status_after: Option<&str>,
    plan_status_update_error: Option<&str>,
    distill_block: Option<&Value>,
    payload: &mut Value,
) {
    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());

    let final_plan_status = plan_status_after.unwrap_or("unchanged");
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::NOTE,
    )
    .with_state_transition("dag_finalized")
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("event_kind", json!("plan_dag_finalized"))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("plan_version", json!(plan.version))
    .with_extra("aggregate_status", json!(aggregate_status))
    .with_extra("final_plan_status", json!(final_plan_status));
    if let Some(err) = plan_status_update_error {
        entry = entry.with_extra("plan_status_update_error", json!(err));
    }
    if let Some(d) = distill_block {
        // Distill block on evidence is the same shape the response carries
        // (triggered + reason + mode + optional result/warning) so audit
        // consumers can correlate without a second JSON parse.
        entry = entry.with_extra("distill", d.clone());
    } else {
        entry = entry.with_extra(
            "distill",
            json!({"triggered": false, "reason": "not_requested"}),
        );
    }
    let append_outcome = evidence_collector::append(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %plan.id,
            error = %error,
            "DAG finalize: evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        payload["evidence_path"] = json!(p);
    }
    if let Some(e) = err {
        payload["evidence_error"] = json!(e);
    }
}
