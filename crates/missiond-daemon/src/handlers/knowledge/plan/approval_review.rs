use super::*;

mod approve;
mod mark;
mod proposer;
mod subscriber;
mod supersede;

use self::proposer::{
    attach_plan_apply_gate_block, attach_plan_proposal_block, build_plan_automation_ctx,
    parse_plan_proposer_mode_or_error, plan_proposer_summary, request_plan_auto_approve_proposal,
};

pub(super) use self::approve::action_approve;
pub(super) use self::mark::action_mark;
pub(crate) use self::subscriber::{handle_review_resolved_event, PlanSubscriberOutcome};
pub(super) use self::supersede::action_supersede;

// ───────────────────────────────────────────────────────────────────────
// approve / mark / supersede — control actions
// ───────────────────────────────────────────────────────────────────────

/// Action whitelist for the plan surface — the parsed
/// `review:plan:<id>:v<v>:<action>` envelope's `<action>` segment must be
/// in this list before we accept the resolution. Mirrors the manager
/// state-changing actions: compile / approve / mark / supersede. (`get`
/// / `list` / `by_task` / `record_evidence` / `execute` never resolve a
/// gate.)
pub(super) const PLAN_REVIEW_ACTIONS: &[&str] = &["compile", "approve", "mark", "supersede"];
