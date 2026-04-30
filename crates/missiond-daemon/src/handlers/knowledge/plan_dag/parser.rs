//! Facade for the mission_plan DAG parser/validator core.
//!
//! The submodules keep the V3 entry/core/egress boundary explicit:
//! - types.rs is the facade for DAG node/error shapes.
//! - types/node.rs owns node shapes and typed hint projections.
//! - types/errors.rs owns build-error egress.
//! - scanner.rs owns PLAN.lisp S-expression scanning and node parsing.
//! - validation.rs owns DAG contract validation and topological ordering.

mod scanner;
mod types;
mod validation;

#[allow(unused_imports)]
pub(super) use scanner::parse_plan_dag;
#[allow(unused_imports)]
pub(super) use types::{
    DagBuildError, ReviewGateKind, FAILURE_POLICY_FAIL_FAST, MAX_NODE_ATTEMPTS_CAP,
    MAX_RETRY_DELAY_MS,
};
pub(in crate::handlers::knowledge) use types::{DagNode, ParsedDag};
pub(super) use validation::build_validated_dag;
