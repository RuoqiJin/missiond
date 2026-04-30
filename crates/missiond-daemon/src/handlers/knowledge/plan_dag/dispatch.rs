//! Facade for DAG node dispatch.
//!
//! Outcome shapes, workstation substrate projection, task-contract context,
//! and the async node runner live in submodules while callers keep importing
//! through `plan_dag::dispatch`.

mod runner;
mod task_contract_ctx;
mod types;
mod workstation;

pub(super) use runner::dispatch_node;
pub(super) use task_contract_ctx::TaskContractDispatchCtx;
pub(super) use types::DispatchOutcome;
#[cfg(test)]
pub(super) use workstation::{node_to_workstation_hints, workstation_outcome_to_dispatch_pair};
