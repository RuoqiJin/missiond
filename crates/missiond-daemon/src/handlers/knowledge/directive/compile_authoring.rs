use anyhow::{anyhow, Result};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use crate::handlers::knowledge::file_artifacts::{
    attempt_artifact_write, ArtifactKind, WriterContext,
};
use crate::handlers::knowledge::review_gate::{
    apply_compile_review_gates, parse_compile_review_gate, parse_review_gate_policy,
    review_gate_policy_was_explicit,
};
use crate::minimax_client::ChatMessage;
use crate::state::AppState;
use missiond_core::types::DirectiveStatus;

use super::load_sonnet_compiler_model;

pub(super) const COMPILER_MODE_DRY_RUN: &str = "dry_run";
pub(super) const COMPILER_MODE_SONNET: &str = "sonnet";
const SONNET_MAX_TOKENS: u32 = 2048;
pub(super) const ALLOWED_SEXP_HEADS: &[&str] =
    &["directive", "directive-draft", "intent-alignment"];

pub(super) async fn action_compile(state: &AppState, args: &Value) -> Result<ToolResult> {
    let utterance = match args.get("utterance").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::MISSING_PARAM, "compile requires `utterance`")
                    .with_suggestion("provide the user utterance to compile into a lisp directive"),
            ))
        }
    };
    let source = args
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("user_utterance")
        .to_string();
    let conversation_id = args
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let persist = args
        .get("persist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let compiler_mode = args
        .get("compiler_mode")
        .and_then(|v| v.as_str())
        .unwrap_or(COMPILER_MODE_DRY_RUN)
        .to_string();
    if compiler_mode != COMPILER_MODE_DRY_RUN && compiler_mode != COMPILER_MODE_SONNET {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("unknown compiler_mode `{}`", compiler_mode),
            )
            .with_suggestion("use compiler_mode=\"dry_run\" or \"sonnet\""),
        ));
    }

    let review_gate = args
        .get("review_gate")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let affected_pillars = collect_string_list(args.get("affected_pillars"));
    let non_goals = collect_string_list(args.get("non_goals"));
    let acceptance = collect_string_list(args.get("acceptance"));

    if compiler_mode == COMPILER_MODE_DRY_RUN {
        return action_compile_dry_run(
            state,
            args,
            &utterance,
            &source,
            conversation_id.as_deref(),
            persist,
            review_gate.as_deref(),
            &affected_pillars,
            &non_goals,
            &acceptance,
        )
        .await;
    }

    action_compile_sonnet(
        state,
        args,
        &utterance,
        &source,
        conversation_id.as_deref(),
        persist,
        review_gate.as_deref(),
        &affected_pillars,
        &non_goals,
        &acceptance,
    )
    .await
}

/// Caller-supplied args that gate the file-first writer. Pulled into a
/// struct so the dry-run + sonnet paths share one extraction routine and
/// `attempt_artifact_write` is invoked with consistent semantics.
pub(super) struct DirectiveFileArgs<'a> {
    /// `write_file=true` opts into the file-first SSOT mirror after the DB
    /// row is committed. Default is false so legacy callers stay
    /// byte-identical.
    pub(super) write_file: bool,
    /// `overwrite_file=true` opts into replacing an existing artifact;
    /// default refuses to overwrite (intent-memory.lisp directive-layer
    /// file-first-artifacts is append-by-version, never silently replaced).
    pub(super) overwrite_file: bool,
    pub(super) topic: Option<&'a str>,
    pub(super) project: Option<&'a str>,
    pub(super) cwd: Option<&'a str>,
    pub(super) target_project: Option<&'a str>,
}

pub(super) fn extract_directive_file_args(args: &Value) -> DirectiveFileArgs<'_> {
    DirectiveFileArgs {
        write_file: args
            .get("write_file")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        overwrite_file: args
            .get("overwrite_file")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        topic: args.get("topic").and_then(|v| v.as_str()),
        project: args.get("project").and_then(|v| v.as_str()),
        cwd: args.get("cwd").and_then(|v| v.as_str()),
        target_project: args.get("target_project").and_then(|v| v.as_str()),
    }
}

/// After the directive row is committed, optionally mirror the compiled
/// sexp to the file-first SSOT
/// (`<project_root>/.missiond/alignment/<topic>/intent-alignment.lisp`).
///
/// Contract:
///   - `write_file=false` (default): no-op, payload returned as-is.
///   - missing topic with `write_file=true`: stamp `file_write_error` and
///     downgrade `status` to `partial`. The DB row already exists; we never
///     roll it back. The caller can retry by calling `mission_directive
///     (action=compile, persist=false)` against the topic until it has
///     supplied one. (We don't refuse the row up-front because callers may
///     legitimately want a draft without a file mirror — write_file is opt
///     in.)
///   - resolve / write failure: same partial semantics, errors flow through
///     `AttemptOutcome::splice_into`.
async fn maybe_write_directive_artifact(
    state: &AppState,
    args: &DirectiveFileArgs<'_>,
    payload: &mut Value,
    sexp: &str,
) {
    if !args.write_file {
        return;
    }
    let topic = match args.topic.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => t,
        None => {
            // Match the resolver-failure splice shape so callers can rely on
            // a single key (`file_write_error`) regardless of why the write
            // didn't happen. Status is downgraded to partial because the DB
            // row landed but the file SSOT did not.
            if let Some(map) = payload.as_object_mut() {
                map.insert("file_written".to_string(), json!(false));
                map.insert(
                    "file_write_error".to_string(),
                    json!("write_file=true requires a non-empty `topic` argument"),
                );
                let already_partial = map
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s == "partial")
                    .unwrap_or(false);
                if !already_partial {
                    map.insert("status".to_string(), json!("partial"));
                }
            }
            return;
        }
    };
    let outcome = attempt_artifact_write(
        &state.project_registry,
        WriterContext {
            kind: ArtifactKind::IntentAlignment,
            topic,
            project: args.project,
            cwd: args.cwd,
            target_project: args.target_project,
            overwrite: args.overwrite_file,
        },
        sexp,
    )
    .await;
    outcome.splice_into(payload);
}

pub(super) fn enrich_persisted_directive_sexp(
    sexp: &str,
    directive_id: &str,
    version: i32,
) -> String {
    if sexp.contains(":directive_id")
        && (sexp.contains(":version") || sexp.contains(":directive_version"))
    {
        return sexp.to_string();
    }

    let trimmed_len = sexp.trim_end().len();
    let trailing = &sexp[trimmed_len..];
    let mut core = sexp[..trimmed_len].to_string();
    if !core.ends_with(')') {
        return sexp.to_string();
    }
    core.pop();
    if !core.contains(":directive_id") {
        core.push_str(&format!("\n  :directive_id {:?}", directive_id));
    }
    if !core.contains(":version") && !core.contains(":directive_version") {
        core.push_str(&format!("\n  :version {}", version));
    }
    core.push(')');
    core.push_str(trailing);
    core
}

#[allow(clippy::too_many_arguments)]
async fn action_compile_dry_run(
    state: &AppState,
    args: &Value,
    utterance: &str,
    source: &str,
    conversation_id: Option<&str>,
    persist: bool,
    review_gate: Option<&str>,
    affected_pillars: &[String],
    non_goals: &[String],
    acceptance: &[String],
) -> Result<ToolResult> {
    let preview_sexp = format!(
        "(directive-draft\n  :utterance {:?}\n  :source {:?}\n  :status :draft)\n",
        utterance, source
    );
    let mut payload = json!({
        "status": "dry_run",
        "compiler_mode": COMPILER_MODE_DRY_RUN,
        "flow_ref": "F-intent-alignment-plan-execution-loop :: s2 intent-alignment-authoring",
        "utterance": utterance,
        "source": source,
        "conversation_id": conversation_id,
        "compiled_sexp_preview": preview_sexp,
        "next_step": "rerun with compiler_mode=\"sonnet\" to invoke directive-compiler actor; or persist=true to insert a draft row",
    });

    if persist {
        let refs_v = build_references_json(
            source,
            conversation_id,
            COMPILER_MODE_DRY_RUN,
            review_gate,
            affected_pillars,
            non_goals,
            acceptance,
        );
        let id = state
            .store
            .directive_insert(
                utterance,
                &preview_sexp,
                1,
                DirectiveStatus::Draft,
                None,
                Some(&refs_v),
            )
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["persisted"] = json!(true);
        payload["directive_id"] = json!(id);
        payload["version"] = json!(1);
        let persisted_preview_sexp =
            enrich_persisted_directive_sexp(&preview_sexp, &id.to_string(), 1);
        payload["compiled_sexp_preview"] = json!(persisted_preview_sexp);

        // wave-14 :: file-first SSOT mirror. Only writes when the caller
        // opted in via write_file=true; the DB row stays committed even if
        // the file write fails (file-vs-db contract — partial status).
        let file_args = extract_directive_file_args(args);
        let topic_for_gate = file_args.topic.map(|s| s.to_string());
        let sexp_for_file = payload["compiled_sexp_preview"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| preview_sexp.clone());
        maybe_write_directive_artifact(state, &file_args, &mut payload, &sexp_for_file).await;

        // wave-14 :: review-gate auto-create. Default policy = Manual keeps
        // the wave-11 explicit-emit (`emit_review_question=true`) the only
        // way to fire an event; `emit_question` policy auto-fires after a
        // successful file write; `off` suppresses both. The hook MUST run
        // after the file-first splice so it can see `file_written`.
        let policy = parse_review_gate_policy(args);
        let policy_explicit = review_gate_policy_was_explicit(args);
        let legacy = parse_compile_review_gate(args);
        apply_compile_review_gates(
            &mut payload,
            &state.bus,
            policy,
            policy_explicit,
            &legacy,
            "directive",
            &id.to_string(),
            1,
            topic_for_gate.as_deref(),
        )
        .await;
    } else {
        payload["persisted"] = json!(false);
    }
    Ok(ToolResult::json_pretty(&payload))
}

#[allow(clippy::too_many_arguments)]
async fn action_compile_sonnet(
    state: &AppState,
    args: &Value,
    utterance: &str,
    source: &str,
    conversation_id: Option<&str>,
    persist: bool,
    review_gate: Option<&str>,
    affected_pillars: &[String],
    non_goals: &[String],
    acceptance: &[String],
) -> Result<ToolResult> {
    let sonnet = match state.sonnet.as_ref() {
        Some(s) => s,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    "LLM_UNAVAILABLE",
                    "Sonnet gateway not initialized; cannot run directive-compiler actor",
                )
                .with_suggestion(
                    "fallback: rerun with compiler_mode=\"dry_run\", or boot the daemon with sonnet gateway enabled",
                ),
            ))
        }
    };

    let system_prompt = build_compiler_system_prompt();
    let user_prompt = build_compiler_user_prompt(
        utterance,
        source,
        review_gate,
        affected_pillars,
        non_goals,
        acceptance,
    );
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
        },
    ];

    let raw = sonnet
        .call_interactive(messages, Some(SONNET_MAX_TOKENS), "directive_compiler")
        .await
        .map_err(|e| anyhow!("Sonnet call failed: {}", e))?;

    let compiled_sexp = match validate_compiled_sexp(&raw) {
        Ok(s) => s,
        Err(SexpValidationError { code, reason, hint }) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(code, reason).with_suggestion(hint),
            ))
        }
    };
    let compiler_model = load_sonnet_compiler_model()?;

    let mut payload = json!({
        "status": "compiled",
        "compiler_mode": COMPILER_MODE_SONNET,
        "compiler_model": compiler_model.clone(),
        "flow_ref": "F-intent-alignment-plan-execution-loop :: s2 intent-alignment-authoring",
        "utterance": utterance,
        "source": source,
        "conversation_id": conversation_id,
        "compiled_sexp": compiled_sexp,
        "review_required": true,
        "next_step": "review via mission_directive(action=approve) after human edit/review",
    });

    if persist {
        let refs_v = build_references_json(
            source,
            conversation_id,
            COMPILER_MODE_SONNET,
            review_gate,
            affected_pillars,
            non_goals,
            acceptance,
        );
        let id = state
            .store
            .directive_insert(
                utterance,
                &compiled_sexp,
                1,
                DirectiveStatus::Draft,
                Some(compiler_model.as_str()),
                Some(&refs_v),
            )
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["persisted"] = json!(true);
        payload["directive_id"] = json!(id);
        payload["version"] = json!(1);
        let persisted_compiled_sexp =
            enrich_persisted_directive_sexp(&compiled_sexp, &id.to_string(), 1);
        payload["compiled_sexp"] = json!(persisted_compiled_sexp);

        // wave-14 :: file-first SSOT mirror — same partial semantics as
        // dry_run path. The compiled sexp is the durable artifact; we
        // splice the path/sha so callers can verify on-disk parity.
        let file_args = extract_directive_file_args(args);
        let topic_for_gate = file_args.topic.map(|s| s.to_string());
        let sexp_for_file = payload["compiled_sexp"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| compiled_sexp.clone());
        maybe_write_directive_artifact(state, &file_args, &mut payload, &sexp_for_file).await;

        // wave-14 :: review-gate auto-create. See dry_run branch above for
        // policy semantics; same hook applies after the file write splice
        // so the EmitQuestion path can observe `file_written`.
        let policy = parse_review_gate_policy(args);
        let policy_explicit = review_gate_policy_was_explicit(args);
        let legacy = parse_compile_review_gate(args);
        apply_compile_review_gates(
            &mut payload,
            &state.bus,
            policy,
            policy_explicit,
            &legacy,
            "directive",
            &id.to_string(),
            1,
            topic_for_gate.as_deref(),
        )
        .await;
    } else {
        payload["persisted"] = json!(false);
    }
    Ok(ToolResult::json_pretty(&payload))
}

// ───────────────────────────────────────────────────────────────────────
// directive-compiler helpers
// ───────────────────────────────────────────────────────────────────────

pub(super) fn collect_string_list(v: Option<&Value>) -> Vec<String> {
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

pub(super) fn build_references_json(
    source: &str,
    conversation_id: Option<&str>,
    compiler_mode: &str,
    review_gate: Option<&str>,
    affected_pillars: &[String],
    non_goals: &[String],
    acceptance: &[String],
) -> Value {
    let mut refs = serde_json::Map::new();
    refs.insert("source".into(), json!(source));
    if let Some(cid) = conversation_id {
        refs.insert("conversation_id".into(), json!(cid));
    }
    refs.insert("compiler_mode".into(), json!(compiler_mode));
    if let Some(rg) = review_gate {
        refs.insert("review_gate".into(), json!(rg));
    }
    if !affected_pillars.is_empty() {
        refs.insert("affected_pillars".into(), json!(affected_pillars));
    }
    if !non_goals.is_empty() {
        refs.insert("non_goals".into(), json!(non_goals));
    }
    if !acceptance.is_empty() {
        refs.insert("acceptance".into(), json!(acceptance));
    }
    Value::Object(refs)
}

pub(super) fn build_compiler_system_prompt() -> String {
    let heads = ALLOWED_SEXP_HEADS.join(" / ");
    format!(
        "You are MissionD's directive-compiler actor. \
         Compile the user utterance into a single Lisp s-expression that captures the alignment intent. \
         Output rules: \
         (1) emit ONLY one top-level s-expression — no Markdown, no commentary, no fences. \
         (2) the top-level head must be one of: {}. \
         (3) include keyword fields :goal :scope and (when given) :affected-pillars :non-goals :acceptance :review-gate :source. \
         (4) all parentheses must be balanced; string literals stay inside double quotes. \
         (5) keep the sexp human-readable; indent nested fields with two spaces.",
        heads
    )
}

fn build_compiler_user_prompt(
    utterance: &str,
    source: &str,
    review_gate: Option<&str>,
    affected_pillars: &[String],
    non_goals: &[String],
    acceptance: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("User utterance:\n");
    out.push_str(utterance);
    out.push_str("\n\nProvenance source: ");
    out.push_str(source);
    if let Some(rg) = review_gate {
        out.push_str("\nReview gate hint: ");
        out.push_str(rg);
    }
    if !affected_pillars.is_empty() {
        out.push_str("\nAffected pillars: ");
        out.push_str(&affected_pillars.join(", "));
    }
    if !non_goals.is_empty() {
        out.push_str("\nNon-goals: ");
        out.push_str(&non_goals.join("; "));
    }
    if !acceptance.is_empty() {
        out.push_str("\nAcceptance: ");
        out.push_str(&acceptance.join("; "));
    }
    out.push_str("\n\nReturn one Lisp s-expression as specified.");
    out
}

#[derive(Debug)]
pub(super) struct SexpValidationError {
    pub(super) code: &'static str,
    pub(super) reason: String,
    pub(super) hint: &'static str,
}

pub(super) fn validate_compiled_sexp(
    raw: &str,
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
    if !ALLOWED_SEXP_HEADS.contains(&head) {
        return Err(SexpValidationError {
            code: "INVALID_COMPILER_OUTPUT",
            reason: format!(
                "top-level head `{}` not in allowlist {:?}",
                head, ALLOWED_SEXP_HEADS
            ),
            hint: "compiler must emit (directive …) | (directive-draft …) | (intent-alignment …)",
        });
    }
    Ok(trimmed.to_string())
}

/// Strip a leading ```lang fence and a trailing ``` fence (if both present).
/// Tolerant: lone fences or missing language tags are also handled.
pub(super) fn strip_fenced_code_block(input: &str) -> String {
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
pub(super) fn parens_balanced(s: &str) -> bool {
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

/// Extract the top-level head symbol from a sexp like `(directive ...)` → `directive`.
/// Returns None when the input does not start with `(` followed by a symbol char.
pub(super) fn top_level_head(s: &str) -> Option<&str> {
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
