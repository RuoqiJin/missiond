use serde_json::{json, Value};

use super::super::auto_answer::is_destructive_review_action;
use super::super::resolution::ReviewDecision;
use super::{
    proposal_json_kind, LlmAutoApproveProposal, LlmAutoApproveProposalBundle,
    LlmAutoApproveProposalConfidence, LlmAutoApproveProposalStatus,
};

// V3 invariant anchor: apply-gate mode must never auto-approve without
// explicit caller approval and a matching proposal hash.

mod evaluate;
mod hash;
mod input;
mod outcome;
mod payload;
mod preflight;

#[allow(unused_imports)]
pub(crate) use self::evaluate::*;
#[allow(unused_imports)]
pub(crate) use self::hash::*;
#[allow(unused_imports)]
pub(crate) use self::input::*;
#[allow(unused_imports)]
pub(crate) use self::outcome::*;
#[allow(unused_imports)]
pub(crate) use self::payload::*;
#[allow(unused_imports)]
pub(crate) use self::preflight::*;
