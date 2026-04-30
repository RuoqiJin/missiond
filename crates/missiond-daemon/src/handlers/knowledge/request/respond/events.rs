use super::*;

/// Pure event-sequence allocator. The initial `request_received` event
/// occupies seq 1; review responses pick up at max(existing) + 1 so each
/// respond call lands a fresh, monotonically increasing event file.
pub(in crate::handlers::knowledge::request) fn next_event_seq(
    existing_filenames: &[String],
) -> u64 {
    let max = existing_filenames
        .iter()
        .filter_map(|n| parse_event_seq_from_filename(n))
        .max()
        .unwrap_or(0);
    max + 1
}

pub(in crate::handlers::knowledge::request) fn parse_event_seq_from_filename(
    name: &str,
) -> Option<u64> {
    let stem = name.strip_suffix(".event.lisp")?;
    stem.parse::<u64>().ok()
}

pub(in crate::handlers::knowledge::request) fn event_path_for_seq(
    events_dir: &Path,
    seq: u64,
) -> PathBuf {
    events_dir.join(format!("{:06}.event.lisp", seq))
}

pub(in crate::handlers::knowledge::request) fn list_event_filenames(
    events_dir: &Path,
) -> Vec<String> {
    let read = match std::fs::read_dir(events_dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    read.filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::handlers::knowledge::request) enum RespondOutcome {
    Recorded,
    Dispatched,
    Blocked,
}

impl RespondOutcome {
    pub(in crate::handlers::knowledge::request) fn wire(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
            Self::Dispatched => "dispatched",
            Self::Blocked => "blocked",
        }
    }

    fn event_kind(self) -> &'static str {
        match self {
            Self::Recorded => "review_response_recorded",
            Self::Dispatched => "review_response_dispatched",
            Self::Blocked => "review_response_blocked",
        }
    }
}

pub(in crate::handlers::knowledge::request) struct ReviewEventArgs<'a> {
    pub(in crate::handlers::knowledge::request) request_id: &'a str,
    pub(in crate::handlers::knowledge::request) seq: u64,
    pub(in crate::handlers::knowledge::request) decision: RespondDecision,
    pub(in crate::handlers::knowledge::request) outcome: RespondOutcome,
    pub(in crate::handlers::knowledge::request) note: Option<&'a str>,
    pub(in crate::handlers::knowledge::request) directive_ref: Option<&'a DirectiveRef>,
    pub(in crate::handlers::knowledge::request) plan_ref: Option<&'a PlanRef>,
    pub(in crate::handlers::knowledge::request) execute: bool,
    pub(in crate::handlers::knowledge::request) inner_action: Option<&'a str>,
    pub(in crate::handlers::knowledge::request) blocked_reason: Option<&'a str>,
    pub(in crate::handlers::knowledge::request) created_at: &'a str,
}

pub(in crate::handlers::knowledge::request) fn build_review_event_lisp(
    args: &ReviewEventArgs<'_>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, ";; MissionD review-response event.");
    let _ = writeln!(out, ";; Schema: {}", EVENT_SCHEMA);
    let event_id = format!("evt-{}-{:06}", args.request_id, args.seq);
    let _ = writeln!(out, "(lifecycle-event {}", lisp_string(&event_id));
    let _ = writeln!(out, "  :schema {}", lisp_string(EVENT_SCHEMA));
    let _ = writeln!(out, "  :seq {}", args.seq);
    let _ = writeln!(out, "  :event_id {}", lisp_string(&event_id));
    let _ = writeln!(out, "  :request_id {}", lisp_string(args.request_id));
    let _ = writeln!(out, "  :kind :{}", args.outcome.event_kind());
    let _ = writeln!(
        out,
        "  :actor (:role :user :id \"mission_request.respond\")"
    );
    let _ = writeln!(out, "  :time {}", lisp_string(args.created_at));
    let _ = writeln!(out, "  :payload");
    let _ = writeln!(out, "    (:decision :{}", args.decision.wire());
    let _ = writeln!(out, "     :outcome :{}", args.outcome.wire());
    if let Some(note) = args.note {
        let _ = writeln!(out, "     :note {}", lisp_string(note));
    }
    if let Some(d) = args.directive_ref {
        let _ = writeln!(out, "     :directive_id {}", lisp_string(&d.id));
        let _ = writeln!(out, "     :directive_version {}", d.version);
    }
    if let Some(p) = args.plan_ref {
        let _ = writeln!(out, "     :plan_id {}", lisp_string(&p.id));
    }
    let _ = writeln!(
        out,
        "     :execute {}",
        if args.execute { "true" } else { "false" }
    );
    if let Some(inner) = args.inner_action {
        let _ = writeln!(out, "     :inner_action {}", lisp_string(inner));
    }
    if let Some(reason) = args.blocked_reason {
        let _ = writeln!(out, "     :blocked_reason {}", lisp_string(reason));
    }
    let _ = writeln!(out, "    )");
    let _ = writeln!(
        out,
        "  :idempotency_key {})",
        lisp_string(&format!(
            "{}/{}/{:06}",
            args.request_id,
            args.outcome.event_kind(),
            args.seq
        ))
    );
    out
}

pub(in crate::handlers::knowledge::request) fn next_action_for(
    decision: RespondDecision,
    outcome: RespondOutcome,
) -> &'static str {
    match (decision, outcome) {
        (RespondDecision::ApproveIntent, RespondOutcome::Dispatched) => {
            "directive approved and plan.lisp projection requested; review the returned plan review_packet"
        }
        (RespondDecision::ApprovePlan, RespondOutcome::Dispatched) => {
            "plan approved; call mission_request respond with response=execute_plan + execute=true to dispatch the plan"
        }
        (RespondDecision::ExecutePlan, RespondOutcome::Dispatched) => {
            "plan execute requested; observe execution status through mission_request status and task receipts"
        }
        (RespondDecision::RejectIntent, RespondOutcome::Recorded) => {
            "rejection recorded; revise the message and call mission_request start again"
        }
        (RespondDecision::RejectPlan, RespondOutcome::Recorded) => {
            "rejection recorded; revise the plan source and call mission_request advance or start again"
        }
        (RespondDecision::AskQuestion, RespondOutcome::Recorded) => {
            "question recorded; wait for follow-up answer, then call mission_request respond again"
        }
        (_, RespondOutcome::Blocked) => {
            "supply the missing reference (or required flag) and re-call mission_request respond"
        }
        _ => "review_packet describes the next legal continuation",
    }
}
