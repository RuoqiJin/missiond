mod cascade;
mod descriptor;
mod run;
mod types;

#[allow(unused_imports)]
pub(super) use cascade::{
    build_compensation_plan_entry, compute_compensation_order, run_cascade_rollback,
};
#[cfg(test)]
pub(super) use descriptor::pre_dispatch_rollback_decision;
pub(super) use descriptor::{build_rollback_descriptor, RollbackDescriptor};
pub(super) use run::{run_rollback, truncate_rollback_brief_preview};
pub(super) use types::{
    CascadeCompensationOutcome, CascadeRollbackOutcome, RollbackCascadeMode, RollbackEvaluation,
    RollbackPolicy, RollbackStatus,
};

use super::DagNode;
