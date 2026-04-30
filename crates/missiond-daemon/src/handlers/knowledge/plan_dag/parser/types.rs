//! Facade for DAG parser types.
//!
//! Node shape / typed hint projections and build-error egress live in
//! submodules so V3 can pin them as separate surfaces while callers keep
//! importing through `parser::types`.

mod errors;
mod node;

pub(in crate::handlers::knowledge::plan_dag) use errors::DagBuildError;
pub(in crate::handlers::knowledge) use node::{DagNode, ParsedDag};
pub(in crate::handlers::knowledge::plan_dag) use node::{
    ReviewGateKind, FAILURE_POLICY_FAIL_FAST, MAX_NODE_ATTEMPTS_CAP, MAX_RETRY_DELAY_MS,
};
pub(super) use node::{FAILURE_POLICY_CONTINUE, VALID_TARGETS};
