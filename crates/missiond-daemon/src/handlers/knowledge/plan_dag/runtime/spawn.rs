use std::collections::HashMap;

use anyhow::Result;
use missiond_core::types::Plan;

use crate::state::AppState;

use super::super::dispatch::{dispatch_node, DispatchOutcome, TaskContractDispatchCtx};
use super::super::lifecycle::{emit_evidence_running, EvidenceCtx};
use super::super::outcome::{ExecutionOutcome, NodeLifecycle};
use super::super::parser::DagNode;

pub(super) async fn spawn_dispatch_attempt(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    node: DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    task_contract_ctx: &TaskContractDispatchCtx,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    outcome: &mut ExecutionOutcome,
    join_set: &mut tokio::task::JoinSet<Result<DispatchOutcome>>,
) {
    lifecycle.insert(node.id.clone(), NodeLifecycle::Running);
    emit_evidence_running(state, ctx, &node, dispatch_strategy, attempt, outcome).await;
    let state_clone = state.clone();
    let plan_clone = plan.clone();
    let task_contract_ctx_clone = task_contract_ctx.clone();
    join_set.spawn(async move {
        dispatch_node(state_clone, plan_clone, node, task_contract_ctx_clone).await
    });
}
