//! Facade for DAG acceptance evaluation.
//!
//! The submodules keep the V3 surface explicit: typed acceptance contract,
//! per-node evaluation, cross-node fan-in, payload scanning, and pause-id egress
//! are independently pinned while callers keep importing through `acceptance`.

mod evaluator;
mod fan_in;
mod pause;
mod payload;
mod types;

pub(super) use evaluator::evaluate_node_acceptance;
pub(super) use fan_in::apply_acceptance_fan_in;
pub(super) use pause::derive_acceptance_pause_id;
pub(super) use types::{
    AcceptanceEvaluation, AcceptanceMode, AcceptanceRequires, AcceptanceStatus,
};
