//! Facade for conservative cascade rollback planning and dispatch.
//!
//! Compensation ordering, plan-mode projection, dispatch outcome mapping, and
//! async dispatch-safe execution live in submodules while callers keep importing
//! through `rollback::cascade`.

mod dispatch_outcome;
mod ordering;
mod plan_entry;
mod runner;

#[allow(unused_imports)]
pub(in crate::handlers::knowledge::plan_dag) use ordering::compute_compensation_order;
#[allow(unused_imports)]
pub(in crate::handlers::knowledge::plan_dag) use plan_entry::build_compensation_plan_entry;
pub(in crate::handlers::knowledge::plan_dag) use runner::run_cascade_rollback;
