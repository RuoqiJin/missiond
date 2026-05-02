//! Flow runner — executes a FlowDefinition node by node with retry/skip policies.
//! Persists FlowContext to the Board Task's flow_context column after each node.

use anyhow::Result;
use tracing::{error, info, warn};

use super::handlers::execute_node;
use super::{ErrorPolicy, FlowContext, FlowDefinition, FlowNode};
use crate::state::AppState;

/// Run a flow to completion. Persists context to Board Task after each node.
///
/// Uses `Box::pin` to break the async recursion cycle:
/// run_flow → execute_node → dispatch_tool → flow_run::handle → run_flow
pub(crate) fn run_flow<'a>(
    state: &'a AppState,
    flow: &'a FlowDefinition,
    ctx: &'a mut FlowContext,
    task_id: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        let start_node = ctx.current_node;

        for (i, node) in flow.nodes.iter().enumerate().skip(start_node) {
            ctx.current_node = i;

            info!(
                flow = %flow.id,
                node = %node.id,
                index = i,
                total = flow.nodes.len(),
                "Flow: executing node"
            );

            let result = execute_with_retry(state, node, ctx, task_id).await;

            match result {
                Ok(output) => {
                    if let Some(ref key) = node.save_as {
                        ctx.set(key, &output);
                    }
                    ctx.completed_nodes.push(node.id.clone());
                    ctx.last_error = None;

                    persist_context(state, task_id, ctx).await;

                    info!(
                        flow = %flow.id,
                        node = %node.id,
                        output_len = output.len(),
                        "Flow: node completed"
                    );
                }
                Err(e) => {
                    ctx.last_error = Some(e.to_string());
                    persist_context(state, task_id, ctx).await;

                    error!(
                        flow = %flow.id,
                        node = %node.id,
                        error = %e,
                        "Flow: node failed"
                    );
                    return Err(e);
                }
            }
        }

        info!(flow = %flow.id, nodes = flow.nodes.len(), "Flow: completed all nodes");
        Ok(())
    }) // Box::pin
}

/// Execute a node with retry logic based on its error policy.
async fn execute_with_retry(
    state: &AppState,
    node: &FlowNode,
    ctx: &FlowContext,
    task_id: &str,
) -> Result<String> {
    let max_attempts = match &node.on_error {
        ErrorPolicy::Retry(n) => *n as usize,
        _ => 1,
    };

    let mut last_err = None;
    for attempt in 0..max_attempts {
        match execute_node(state, &node.node_type, ctx, task_id).await {
            Ok(output) => return Ok(output),
            Err(e) => {
                if attempt + 1 < max_attempts {
                    let backoff = std::time::Duration::from_secs(2u64.pow(attempt as u32));
                    warn!(
                        node = %node.id,
                        attempt = attempt + 1,
                        max = max_attempts,
                        backoff_secs = backoff.as_secs(),
                        error = %e,
                        "Flow: retrying node"
                    );
                    tokio::time::sleep(backoff).await;
                }
                last_err = Some(e);
            }
        }
    }

    let err = last_err.unwrap();
    match &node.on_error {
        ErrorPolicy::Skip => {
            warn!(node = %node.id, error = %err, "Flow: skipping failed node");
            Ok(String::new())
        }
        _ => Err(err),
    }
}

/// Persist flow context to the Board Task's flow_context field.
async fn persist_context(state: &AppState, task_id: &str, ctx: &FlowContext) {
    let json = match serde_json::to_string(ctx) {
        Ok(j) => j,
        Err(e) => {
            warn!(error = %e, "Flow: failed to serialize context");
            return;
        }
    };

    if let Err(e) = state
        .store
        .update_board_task(
            task_id,
            &missiond_core::types::UpdateBoardTaskInput {
                flow_context: Some(json),
                ..Default::default()
            },
        )
        .await
    {
        warn!(task_id, error = %e, "Flow: failed to persist context");
    }
}
