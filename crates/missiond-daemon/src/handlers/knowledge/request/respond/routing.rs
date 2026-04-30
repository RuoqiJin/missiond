use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::handlers::knowledge::request) enum RespondDecision {
    ApproveIntent,
    RejectIntent,
    AskQuestion,
    ApprovePlan,
    RejectPlan,
    ExecutePlan,
}

impl RespondDecision {
    pub(in crate::handlers::knowledge::request) fn wire(self) -> &'static str {
        match self {
            Self::ApproveIntent => "approve_intent",
            Self::RejectIntent => "reject_intent",
            Self::AskQuestion => "ask_question",
            Self::ApprovePlan => "approve_plan",
            Self::RejectPlan => "reject_plan",
            Self::ExecutePlan => "execute_plan",
        }
    }

    pub(in crate::handlers::knowledge::request) fn requires_directive_ref(self) -> bool {
        matches!(self, Self::ApproveIntent | Self::RejectIntent)
    }

    pub(in crate::handlers::knowledge::request) fn requires_plan_ref(self) -> bool {
        matches!(
            self,
            Self::ApprovePlan | Self::RejectPlan | Self::ExecutePlan
        )
    }

    /// Record-only routes never mutate directive/plan approval state and
    /// only persist a request-local review event so the user decision
    /// remains auditable.
    pub(in crate::handlers::knowledge::request) fn record_only(self) -> bool {
        matches!(
            self,
            Self::RejectIntent | Self::RejectPlan | Self::AskQuestion
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::knowledge::request) enum RespondParseError {
    Missing,
    Unknown(String),
}

impl RespondParseError {
    pub(in crate::handlers::knowledge::request) fn into_tool_error(self) -> ToolError {
        match self {
            Self::Missing => ToolError::new(
                error_codes::MISSING_PARAM,
                "respond requires `response` (or `decision`)",
            )
            .with_suggestion(
                "valid: approve_intent|reject_intent|ask_question|approve_plan|reject_plan|execute_plan",
            ),
            Self::Unknown(raw) => ToolError::new(
                error_codes::INVALID_PARAM,
                format!("unknown respond decision `{}`", raw),
            )
            .with_suggestion(
                "valid: approve_intent|reject_intent|ask_question|approve_plan|reject_plan|execute_plan",
            ),
        }
    }
}

/// Pure decision parse — accepts `response` or `decision`. Pulled out so
/// unit tests can pin the canonical wire vocabulary without an AppState.
pub(in crate::handlers::knowledge::request) fn parse_respond_decision(
    args: &Value,
) -> std::result::Result<RespondDecision, RespondParseError> {
    let raw = nonblank(args.get("response"))
        .or_else(|| nonblank(args.get("decision")))
        .ok_or(RespondParseError::Missing)?;
    match raw.as_str() {
        "approve_intent" => Ok(RespondDecision::ApproveIntent),
        "reject_intent" => Ok(RespondDecision::RejectIntent),
        "ask_question" => Ok(RespondDecision::AskQuestion),
        "approve_plan" => Ok(RespondDecision::ApprovePlan),
        "reject_plan" => Ok(RespondDecision::RejectPlan),
        "execute_plan" => Ok(RespondDecision::ExecutePlan),
        _ => Err(RespondParseError::Unknown(raw)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::knowledge::request) struct DirectiveRef {
    pub(in crate::handlers::knowledge::request) id: String,
    pub(in crate::handlers::knowledge::request) version: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::knowledge::request) struct PlanRef {
    pub(in crate::handlers::knowledge::request) id: String,
}

/// Best-effort scan of a Lisp artifact for `:<key> "<uuid>"`. Pure helper —
/// no IO, no regex crate. Picks the first occurrence so callers keep the
/// canonical persisted ref ahead of any later debug noise.
pub(in crate::handlers::knowledge::request) fn extract_lisp_keyword_string(
    text: &str,
    key: &str,
) -> Option<String> {
    let needle = format!(":{}", key);
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find(&needle) {
        let abs = cursor + found;
        let after = &text[abs + needle.len()..];
        let trimmed = after.trim_start_matches([' ', '\t', '\r', '\n']);
        if let Some(stripped) = trimmed.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                let val = &stripped[..end];
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
        cursor = abs + needle.len();
    }
    None
}

pub(in crate::handlers::knowledge::request) fn extract_lisp_keyword_int(
    text: &str,
    key: &str,
) -> Option<i32> {
    let needle = format!(":{}", key);
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find(&needle) {
        let abs = cursor + found;
        let after = &text[abs + needle.len()..];
        let trimmed = after.trim_start_matches([' ', '\t', '\r', '\n']);
        let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<i32>() {
            return Some(n);
        }
        cursor = abs + needle.len();
    }
    None
}

pub(in crate::handlers::knowledge::request) fn is_uuid_shaped(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

pub(in crate::handlers::knowledge::request) fn resolve_directive_ref(
    args: &Value,
    intent_alignment_text: Option<&str>,
) -> Option<DirectiveRef> {
    let id =
        nonblank(args.get("approved_directive_id")).or_else(|| nonblank(args.get("directive_id")));
    let version = args
        .get("directive_version")
        .and_then(|v| v.as_i64())
        .map(|n| n as i32);

    let (id, version) = match (id, version) {
        (Some(id), Some(v)) => (id, v),
        _ => match intent_alignment_text.and_then(extract_directive_ref_from_artifact) {
            Some(parsed) => (parsed.id, parsed.version),
            None => return None,
        },
    };
    Some(DirectiveRef { id, version })
}

pub(in crate::handlers::knowledge::request) fn extract_directive_ref_from_artifact(
    text: &str,
) -> Option<DirectiveRef> {
    let id = match extract_lisp_keyword_string(text, "directive_id") {
        Some(id) => id,
        None => extract_lisp_keyword_string(text, "id").filter(|id| is_uuid_shaped(id))?,
    };
    let version = extract_lisp_keyword_int(text, "directive_version")
        .or_else(|| extract_lisp_keyword_int(text, "version"))?;
    Some(DirectiveRef { id, version })
}

pub(in crate::handlers::knowledge::request) fn resolve_plan_ref(
    args: &Value,
    plan_text: Option<&str>,
    event_texts: &[String],
) -> Option<PlanRef> {
    if let Some(id) =
        nonblank(args.get("approved_plan_id")).or_else(|| nonblank(args.get("plan_id")))
    {
        return Some(PlanRef { id });
    }
    plan_text
        .and_then(extract_plan_ref_from_artifact)
        .or_else(|| extract_latest_plan_ref_from_events(event_texts))
}

pub(in crate::handlers::knowledge::request) fn extract_plan_ref_from_artifact(
    text: &str,
) -> Option<PlanRef> {
    if let Some(id) = extract_lisp_keyword_string(text, "plan_id") {
        return Some(PlanRef { id });
    }
    // Request-local plan.lisp may contain nested node ids such as
    // `(:id "root" ...)`; never treat those as persisted plan refs.
    extract_lisp_keyword_string(text, "id")
        .filter(|id| is_uuid_shaped(id))
        .map(|id| PlanRef { id })
}

pub(in crate::handlers::knowledge::request) fn extract_latest_plan_ref_from_events(
    event_texts: &[String],
) -> Option<PlanRef> {
    event_texts
        .iter()
        .rev()
        .find_map(|text| extract_lisp_keyword_string(text, "plan_id").map(|id| PlanRef { id }))
}

/// Build the plan-authoring continuation for response=approve_intent.
///
/// `mission_request` stays the public adapter, but the actual plan compile
/// still flows through unified_entry so the existing mission_plan gate,
/// compiler, and projection metadata remain authoritative.
pub(in crate::handlers::knowledge::request) fn build_respond_plan_compile_args(
    args: &Value,
    directive: &DirectiveRef,
    request_id: &str,
) -> Value {
    let mut out = serde_json::Map::new();
    let board_task_id =
        nonblank(args.get("board_task_id")).unwrap_or_else(|| request_id.to_string());
    out.insert("approved_directive_id".into(), json!(directive.id.clone()));
    out.insert("directive_version".into(), json!(directive.version));
    out.insert("board_task_id".into(), json!(board_task_id));

    // The inner mission_plan compile only understands write_file; the
    // V3-preferred compat_write_file name and the legacy write_file alias
    // are both mission_request-local controls. Forward write_file=true to
    // the inner surface only when the caller opted into compat writes.
    // Per (compat-writer-policy ...) in .missiond/v3/missiond-blueprint.lisp.
    let compat_requested = match args.as_object() {
        Some(map) => compat_write_requested(map),
        None => false,
    };

    for key in [
        "compiler_mode",
        "persist",
        "target",
        "target_project",
        "dispatch_strategy",
        "parallelism",
        "objective",
        "requested_cwd",
        "flow_id",
        "overwrite_file",
        "topic",
        "project",
        "cwd",
        "review_gate_policy",
        "emit_review_question",
        "review_question_text",
        "review_question_id",
        "plan_acceptance",
        "plan_constraints",
    ] {
        if let Some(v) = args.get(key) {
            if !v.is_null() {
                out.insert(key.into(), v.clone());
            }
        }
    }
    if compat_requested {
        out.insert("write_file".into(), json!(true));
    }
    Value::Object(out)
}
