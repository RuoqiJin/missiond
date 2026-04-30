use super::*;
use serde_json::Value;

pub(in crate::handlers::knowledge::plan) fn collect_string_list(v: Option<&Value>) -> Vec<String> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![s.clone()]
            }
        }
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|item| match item {
                Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(in crate::handlers::knowledge::plan) fn build_planner_system_prompt() -> String {
    let heads = ALLOWED_PLAN_HEADS.join(" / ");
    format!(
        "You are MissionD's plan-compiler actor (intent-layer). \
         Compile the input directive + board_task context into ONE Lisp s-expression \
         representing the executable plan. \
         Output rules: \
         (1) emit ONLY one top-level s-expression — no Markdown, no fences, no commentary. \
         (2) the top-level head must be one of: {}. \
         (3) the sexp MUST contain the literal board_task_id value somewhere — typically \
             :board_task_id \"<id>\" — so it anchors to the right execution row. \
         (4) include keyword fields :goal :phases :tasks (and when applicable :acceptance \
             :constraints :rollback :tests :files), each as nested sexps. \
         (5) all parentheses must be balanced; string literals stay inside double quotes. \
         (6) keep the sexp human-readable; indent nested fields with two spaces.",
        heads
    )
}

#[allow(clippy::too_many_arguments)]
pub(in crate::handlers::knowledge::plan) fn build_planner_user_prompt(
    board_task_id: &str,
    directive_pin: Option<(uuid::Uuid, i32)>,
    directive_sexp: Option<&str>,
    target_project: Option<&str>,
    dispatch_strategy: Option<&str>,
    parallelism: Option<&str>,
    acceptance: &[String],
    constraints: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("Board task id (anchor): ");
    out.push_str(board_task_id);
    if let Some((id, ver)) = directive_pin {
        out.push_str(&format!("\nDirective: {} v{}", id, ver));
    }
    if let Some(sexp) = directive_sexp {
        out.push_str("\nApproved directive sexp:\n");
        out.push_str(sexp);
    }
    if let Some(tp) = target_project {
        out.push_str("\nTarget project context: ");
        out.push_str(tp);
    }
    if let Some(ds) = dispatch_strategy {
        out.push_str("\nDispatch strategy hint: ");
        out.push_str(ds);
    }
    if let Some(p) = parallelism {
        out.push_str("\nParallelism hint: ");
        out.push_str(p);
    }
    if !acceptance.is_empty() {
        out.push_str("\nAcceptance: ");
        out.push_str(&acceptance.join("; "));
    }
    if !constraints.is_empty() {
        out.push_str("\nConstraints: ");
        out.push_str(&constraints.join("; "));
    }
    out.push_str("\n\nReturn one Lisp s-expression as specified.");
    out
}

#[derive(Debug)]
pub(in crate::handlers::knowledge::plan) struct SexpValidationError {
    pub(in crate::handlers::knowledge::plan) code: &'static str,
    pub(in crate::handlers::knowledge::plan) reason: String,
    pub(in crate::handlers::knowledge::plan) hint: &'static str,
}

pub(in crate::handlers::knowledge::plan) fn validate_compiled_plan_sexp(
    raw: &str,
    board_task_id: &str,
) -> std::result::Result<String, SexpValidationError> {
    let stripped = strip_fenced_code_block(raw);
    let trimmed = stripped.trim();
    if trimmed.is_empty() {
        return Err(SexpValidationError {
            code: "INVALID_COMPILER_OUTPUT",
            reason: "compiler returned empty content after stripping fences".to_string(),
            hint: "rerun with compiler_mode=\"dry_run\" or retry sonnet",
        });
    }
    if !trimmed.starts_with('(') {
        return Err(SexpValidationError {
            code: "INVALID_COMPILER_OUTPUT",
            reason: format!(
                "compiler output must start with `(`; got `{}…`",
                trimmed.chars().take(16).collect::<String>()
            ),
            hint: "ensure the LLM emits one bare s-expression, no Markdown",
        });
    }
    if !parens_balanced(trimmed) {
        return Err(SexpValidationError {
            code: "INVALID_COMPILER_OUTPUT",
            reason: "parentheses are not balanced in compiler output".to_string(),
            hint: "retry the compile or fall back to compiler_mode=\"dry_run\"",
        });
    }
    let head = top_level_head(trimmed).unwrap_or("");
    if !ALLOWED_PLAN_HEADS.contains(&head) {
        return Err(SexpValidationError {
            code: "INVALID_COMPILER_OUTPUT",
            reason: format!(
                "top-level head `{}` not in allowlist {:?}",
                head, ALLOWED_PLAN_HEADS
            ),
            hint: "compiler must emit (plan …) | (plan-draft …) | (PLAN …)",
        });
    }
    if !trimmed.contains(board_task_id) {
        return Err(SexpValidationError {
            code: "INVALID_COMPILER_OUTPUT",
            reason: format!(
                "compiled plan does not reference board_task_id `{}`; refusing un-anchored plan",
                board_task_id
            ),
            hint: "the planner must include :board_task_id <id> so the row anchors correctly",
        });
    }
    Ok(trimmed.to_string())
}

/// Strip a leading ```lang fence and a trailing ``` fence (if both present).
/// Tolerant: lone fences or missing language tags are also handled.
pub(in crate::handlers::knowledge::plan) fn strip_fenced_code_block(input: &str) -> String {
    let trimmed = input.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }
    let after_open = match trimmed.find('\n') {
        Some(idx) => &trimmed[idx + 1..],
        None => return trimmed.to_string(),
    };
    let body = match after_open.rfind("```") {
        Some(idx) => &after_open[..idx],
        None => after_open,
    };
    body.trim().to_string()
}

/// Balanced parens counter that ignores `(` / `)` inside double-quoted strings.
/// Honors `\\` and `\"` escape sequences inside strings.
pub(in crate::handlers::knowledge::plan) fn parens_balanced(s: &str) -> bool {
    let mut depth: i64 = 0;
    let mut in_string = false;
    let mut escape = false;
    for ch in s.chars() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match ch {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    !in_string && depth == 0
}

/// Extract the top-level head symbol from a sexp like `(plan ...)` → `plan`.
/// Returns None when the input does not start with `(` followed by a symbol char.
pub(in crate::handlers::knowledge::plan) fn top_level_head(s: &str) -> Option<&str> {
    let trimmed = s.trim_start();
    let inner = trimmed.strip_prefix('(')?.trim_start();
    let end = inner
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || *c == '(' || *c == ')')
        .map(|(i, _)| i)
        .unwrap_or(inner.len());
    if end == 0 {
        None
    } else {
        Some(&inner[..end])
    }
}
