//! Facade for DAG rollback policy/evaluation types.
//!
//! Node hint projections, policy parsing, node-local evaluation JSON, and
//! cascade rollback JSON live in submodules while callers keep importing
//! through `rollback::types`.

mod cascade;
mod evaluation;
mod node_ext;
mod policy;

pub(in crate::handlers::knowledge::plan_dag) use cascade::{
    CascadeCompensationOutcome, CascadeRollbackOutcome, RollbackCascadeMode,
};
pub(in crate::handlers::knowledge::plan_dag) use evaluation::{RollbackEvaluation, RollbackStatus};
pub(in crate::handlers::knowledge::plan_dag) use policy::RollbackPolicy;
