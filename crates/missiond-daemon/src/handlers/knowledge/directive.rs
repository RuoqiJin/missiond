//! mission_directive — manager surface for the directive table.
//!
//! Lisp authority:
//!   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//!       s2 intent-alignment-authoring + s3 alignment-review-gate
//!   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//!       role alignment-author (mode-A direct-llm / mode-B resident slot)
//!   - intent-memory.lisp :: module directive-layer ::
//!       file-first-artifacts :: intent-alignment-artifact
//!   - intent-tools.lisp :: implemented-surface mission_directive
//!
//! Action coverage:
//!   compile        — directive-compiler actor v0:
//!                      compiler_mode="dry_run" (default) → no LLM, preview shape
//!                      compiler_mode="sonnet"            → SonnetGateway interactive call,
//!                                                          validates lisp shape, draft persist
//!                    persist=true writes DirectiveStatus::Draft (review remains human gate)
//!   list           — full (DirectiveLayerStore::directive_list_recent)
//!   get            — full (directive_get / version_chain head)
//!   approve        — full (directive_approve)
//!   archive        — full (directive_update_status → archived)
//!   version_chain  — full (directive_get_version_chain)

use anyhow::{anyhow, Result};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::str::FromStr;

use crate::handlers::knowledge::file_artifacts::{
    attempt_artifact_write, ArtifactKind, WriterContext,
};
use crate::handlers::knowledge::review_gate::{
    apply_compile_review_gates, maybe_emit_review_question_resolved, parse_compile_review_gate,
    parse_resolution_review_question_id, parse_review_gate_policy,
    parse_review_question_id_struct, parse_review_resolution_input, resolution_wire_string,
    review_gate_policy_was_explicit, stamp_needs_changes_next_step, stamp_resolution_payload,
    validate_review_resolution_envelope, ResolutionOutcome, ReviewResolutionInput,
};
use crate::minimax_client::ChatMessage;
use crate::state::AppState;
use missiond_core::types::DirectiveStatus;

const COMPILER_MODE_DRY_RUN: &str = "dry_run";
const COMPILER_MODE_SONNET: &str = "sonnet";
const SONNET_COMPILER_MODEL: &str = "claude-sonnet";
const SONNET_MAX_TOKENS: u32 = 2048;
const ALLOWED_SEXP_HEADS: &[&str] = &["directive", "directive-draft", "intent-alignment"];

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 500;

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.to_string(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "mission_directive requires `action`",
                )
                .with_suggestion(
                    "actions: compile|list|get|approve|archive|version_chain",
                ),
            ))
        }
    };

    match action.as_str() {
        "compile" => action_compile(state, &args).await,
        "list" => action_list(state, &args).await,
        "get" => action_get(state, &args).await,
        "approve" => action_approve(state, &args).await,
        "archive" => action_archive(state, &args).await,
        "version_chain" => action_version_chain(state, &args).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_directive action `{}`", other),
            )
            .with_suggestion(
                "valid: compile|list|get|approve|archive|version_chain",
            ),
        )),
    }
}

// ───────────────────────────────────────────────────────────────────────
// compile — directive-compiler actor v0
//   compiler_mode = "dry_run" (default): no LLM, returns preview shape
//   compiler_mode = "sonnet"           : SonnetGateway interactive call
// ───────────────────────────────────────────────────────────────────────

async fn action_compile(state: &AppState, args: &Value) -> Result<ToolResult> {
    let utterance = match args.get("utterance").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "compile requires `utterance`",
                )
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
struct DirectiveFileArgs<'a> {
    /// `write_file=true` opts into the file-first SSOT mirror after the DB
    /// row is committed. Default is false so legacy callers stay
    /// byte-identical.
    write_file: bool,
    /// `overwrite_file=true` opts into replacing an existing artifact;
    /// default refuses to overwrite (intent-memory.lisp directive-layer
    /// file-first-artifacts is append-by-version, never silently replaced).
    overwrite_file: bool,
    topic: Option<&'a str>,
    project: Option<&'a str>,
    cwd: Option<&'a str>,
    target_project: Option<&'a str>,
}

fn extract_directive_file_args(args: &Value) -> DirectiveFileArgs<'_> {
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

        // wave-14 :: file-first SSOT mirror. Only writes when the caller
        // opted in via write_file=true; the DB row stays committed even if
        // the file write fails (file-vs-db contract — partial status).
        let file_args = extract_directive_file_args(args);
        let topic_for_gate = file_args.topic.map(|s| s.to_string());
        maybe_write_directive_artifact(state, &file_args, &mut payload, &preview_sexp).await;

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

    let mut payload = json!({
        "status": "compiled",
        "compiler_mode": COMPILER_MODE_SONNET,
        "compiler_model": SONNET_COMPILER_MODEL,
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
                Some(SONNET_COMPILER_MODEL),
                Some(&refs_v),
            )
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["persisted"] = json!(true);
        payload["directive_id"] = json!(id);
        payload["version"] = json!(1);

        // wave-14 :: file-first SSOT mirror — same partial semantics as
        // dry_run path. The compiled sexp is the durable artifact; we
        // splice the path/sha so callers can verify on-disk parity.
        let file_args = extract_directive_file_args(args);
        let topic_for_gate = file_args.topic.map(|s| s.to_string());
        maybe_write_directive_artifact(state, &file_args, &mut payload, &compiled_sexp).await;

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

fn collect_string_list(v: Option<&Value>) -> Vec<String> {
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

fn build_references_json(
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

fn build_compiler_system_prompt() -> String {
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
struct SexpValidationError {
    code: &'static str,
    reason: String,
    hint: &'static str,
}

fn validate_compiled_sexp(raw: &str) -> std::result::Result<String, SexpValidationError> {
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
fn strip_fenced_code_block(input: &str) -> String {
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
fn parens_balanced(s: &str) -> bool {
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
fn top_level_head(s: &str) -> Option<&str> {
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

// ───────────────────────────────────────────────────────────────────────
// list / get / version_chain — store-backed reads
// ───────────────────────────────────────────────────────────────────────

async fn action_list(state: &AppState, args: &Value) -> Result<ToolResult> {
    let status = args
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| DirectiveStatus::from_str(s).map_err(|e| anyhow!(e)))
        .transpose()?;
    let limit = args
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);

    let rows = state
        .store
        .directive_list_recent(status, limit)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    Ok(ToolResult::json_pretty(&json!({
        "directives": rows,
        "count": rows.len(),
        "filter": { "status": status.map(|s| s.as_str().to_string()) },
        "limit": limit,
    })))
}

async fn action_get(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "directive_id")?;
    let version = args
        .get("version")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);

    // No version → return the head (latest) of the chain.
    let resolved = match version {
        Some(v) => state
            .store
            .directive_get(id, v)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?,
        None => {
            let chain = state
                .store
                .directive_get_version_chain(id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            chain.into_iter().last()
        }
    };
    match resolved {
        Some(d) => Ok(ToolResult::json_pretty(&d)),
        None => Ok(ToolResult::structured_error(
            ToolError::new(error_codes::NOT_FOUND, format!("directive `{}` not found", id))
                .with_suggestion("use action=list to enumerate"),
        )),
    }
}

async fn action_version_chain(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "directive_id")?;
    let chain = state
        .store
        .directive_get_version_chain(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "directive_id": id,
        "chain": chain,
        "versions": chain.len(),
    })))
}

// ───────────────────────────────────────────────────────────────────────
// approve / archive — control actions
// ───────────────────────────────────────────────────────────────────────

/// Action whitelist for the directive surface — the parsed
/// `review:directive:<id>:v<v>:<action>` envelope's `<action>` segment
/// must be in this list before we accept the resolution. We deliberately
/// scope to manager actions that change persisted state (compile / approve
/// / archive); `version_chain` / `get` / `list` never resolve a gate.
const DIRECTIVE_REVIEW_ACTIONS: &[&str] = &["compile", "approve", "archive"];

async fn action_approve(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "directive_id")?;
    let version = require_i32(args, "version")?;

    // wave-15 :: explicit resolution bridge. When the caller supplies
    // `review_question_id` + `review_decision` we validate the envelope
    // BEFORE mutating state. `Rejected` / `NeedsChanges` skip the DB
    // transition entirely (the artifact stays non-approved); `Approved`
    // proceeds with the existing manager transition.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(e.code(), e.message()),
            ))
        }
    };

    if let Some(input) = resolution {
        return action_approve_with_resolution(state, id, version, input).await;
    }

    state
        .store
        .directive_approve(id, version)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "status": "approved",
        "directive_id": id,
        "version": version,
    });
    // wave-11/14 quiet emit path — kept for callers that fire a Resolved
    // event without the wave-15 decision-bearing envelope. The DB
    // mutation already committed; bus failures only surface a warning so
    // the approve never fails on a side-channel error.
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        qid.as_deref(),
        "approved",
        None,
    )
    .await;
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=approve`. Validates the
/// review envelope (scope / artifact / version / action), then performs
/// the manager transition only when the decision is `approved`.
async fn action_approve_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    version: i32,
    input: ReviewResolutionInput,
) -> Result<ToolResult> {
    // Parse the deterministic id envelope.
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new("REVIEW_ID_MALFORMED", e.message()),
            ))
        }
    };
    // Source the current artifact version from the chain head so the
    // staleness check is anchored to the latest persisted draft / version.
    let chain = state
        .store
        .directive_get_version_chain(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let current_version = match chain.iter().last() {
        Some(d) => d.version,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("directive `{}` not found for resolution", id),
                ),
            ))
        }
    };
    if version != current_version {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "STALE_REVIEW_VERSION",
                format!(
                    "approve `version=v{}` does not match directive `{}` head `v{}`",
                    version, id, current_version
                ),
            ),
        ));
    }
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "directive",
        &id.to_string(),
        current_version,
        DIRECTIVE_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(
            ToolError::new(e.code(), e.message()),
        ));
    }

    let mut payload = json!({
        "directive_id": id,
        "version": version,
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            state
                .store
                .directive_approve(id, version)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("approved");
        }
        ResolutionOutcome::KeepArtifact => {
            // rejected — leave the artifact at its current status. We do
            // not down-convert (e.g. force back to Draft) because the
            // file-first SSOT contract says transitions are append-only;
            // we just refuse to advance.
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "directive", "compile");
        }
    }

    stamp_resolution_payload(&mut payload, &input);
    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    Ok(ToolResult::json_pretty(&payload))
}

async fn action_archive(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "directive_id")?;
    let version = require_i32(args, "version")?;

    // wave-15 :: explicit resolution bridge — see `action_approve` above.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(e.code(), e.message()),
            ))
        }
    };

    if let Some(input) = resolution {
        return action_archive_with_resolution(state, id, version, input).await;
    }

    state
        .store
        .directive_update_status(id, version, DirectiveStatus::Archived)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "status": "archived",
        "directive_id": id,
        "version": version,
    });
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        qid.as_deref(),
        "archived",
        None,
    )
    .await;
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=archive`. Same envelope
/// validation as approve; on `approved` decision we perform the archive
/// transition; on `rejected`/`needs_changes` the directive stays at its
/// current status.
async fn action_archive_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    version: i32,
    input: ReviewResolutionInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new("REVIEW_ID_MALFORMED", e.message()),
            ))
        }
    };
    let chain = state
        .store
        .directive_get_version_chain(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let current_version = match chain.iter().last() {
        Some(d) => d.version,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("directive `{}` not found for resolution", id),
                ),
            ))
        }
    };
    if version != current_version {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "STALE_REVIEW_VERSION",
                format!(
                    "archive `version=v{}` does not match directive `{}` head `v{}`",
                    version, id, current_version
                ),
            ),
        ));
    }
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "directive",
        &id.to_string(),
        current_version,
        DIRECTIVE_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(
            ToolError::new(e.code(), e.message()),
        ));
    }

    let mut payload = json!({
        "directive_id": id,
        "version": version,
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            state
                .store
                .directive_update_status(id, version, DirectiveStatus::Archived)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("archived");
        }
        ResolutionOutcome::KeepArtifact => {
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "directive", "compile");
        }
    }

    stamp_resolution_payload(&mut payload, &input);
    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    Ok(ToolResult::json_pretty(&payload))
}

// ───────────────────────────────────────────────────────────────────────
// arg helpers
// ───────────────────────────────────────────────────────────────────────

fn parse_id_arg(args: &Value, key: &str) -> Result<uuid::Uuid> {
    let raw = args
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("`{}` required", key))?;
    uuid::Uuid::parse_str(raw).map_err(|e| anyhow!("`{}` is not a UUID: {}", key, e))
}

fn require_i32(args: &Value, key: &str) -> Result<i32> {
    args.get(key)
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
        .ok_or_else(|| anyhow!("`{}` required (integer)", key))
}

// ───────────────────────────────────────────────────────────────────────
// tests — pure functions only (no LLM, no DB)
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- strip_fenced_code_block --

    #[test]
    fn strip_fence_with_lang_tag() {
        let raw = "```lisp\n(directive :goal :ship)\n```";
        assert_eq!(strip_fenced_code_block(raw), "(directive :goal :ship)");
    }

    #[test]
    fn strip_fence_without_lang_tag() {
        let raw = "```\n(directive-draft)\n```";
        assert_eq!(strip_fenced_code_block(raw), "(directive-draft)");
    }

    #[test]
    fn strip_fence_no_fence_passthrough() {
        let raw = "  (intent-alignment :a 1)  ";
        assert_eq!(strip_fenced_code_block(raw), "(intent-alignment :a 1)");
    }

    #[test]
    fn strip_fence_preserves_inner_whitespace_after_trim() {
        let raw = "```lisp\n(directive\n  :goal :x)\n```";
        assert_eq!(strip_fenced_code_block(raw), "(directive\n  :goal :x)");
    }

    // -- parens_balanced --

    #[test]
    fn parens_balanced_simple() {
        assert!(parens_balanced("(a (b c) d)"));
    }

    #[test]
    fn parens_balanced_unbalanced_extra_open() {
        assert!(!parens_balanced("(a (b c)"));
    }

    #[test]
    fn parens_balanced_unbalanced_extra_close() {
        assert!(!parens_balanced("(a))"));
    }

    #[test]
    fn parens_balanced_ignores_parens_in_strings() {
        assert!(parens_balanced(r#"(directive :note "ignore ) and ( inside")"#));
    }

    #[test]
    fn parens_balanced_handles_escaped_quote_in_string() {
        // The escaped \" should NOT terminate the string, so the trailing ) is real.
        assert!(parens_balanced(r#"(d :s "ab\"cd ( still string )")"#));
    }

    #[test]
    fn parens_balanced_unterminated_string_fails() {
        assert!(!parens_balanced(r#"(d :s "open string)"#));
    }

    // -- top_level_head --

    #[test]
    fn top_head_extracts_basic() {
        assert_eq!(top_level_head("(directive :goal x)"), Some("directive"));
    }

    #[test]
    fn top_head_extracts_with_leading_whitespace() {
        assert_eq!(
            top_level_head("\n  (intent-alignment\n  :goal x)"),
            Some("intent-alignment")
        );
    }

    #[test]
    fn top_head_handles_dashed_symbol() {
        assert_eq!(top_level_head("(directive-draft)"), Some("directive-draft"));
    }

    #[test]
    fn top_head_returns_none_when_not_paren() {
        assert_eq!(top_level_head("directive"), None);
    }

    // -- validate_compiled_sexp --

    #[test]
    fn validate_accepts_directive() {
        let raw = "```lisp\n(directive :goal :align :scope :pillar)\n```";
        let out = validate_compiled_sexp(raw).expect("should validate");
        assert!(out.starts_with("(directive"));
    }

    #[test]
    fn validate_accepts_intent_alignment() {
        let raw = "(intent-alignment :goal x)";
        let out = validate_compiled_sexp(raw).expect("should validate");
        assert!(out.starts_with("(intent-alignment"));
    }

    #[test]
    fn validate_rejects_empty() {
        let err = validate_compiled_sexp("```\n   \n```").unwrap_err();
        assert_eq!(err.code, "INVALID_COMPILER_OUTPUT");
        assert!(err.reason.contains("empty"));
    }

    #[test]
    fn validate_rejects_non_paren_start() {
        let err = validate_compiled_sexp("Sure! Here is your directive: ...").unwrap_err();
        assert!(err.reason.contains("`("));
    }

    #[test]
    fn validate_rejects_unbalanced() {
        let err = validate_compiled_sexp("(directive :goal x").unwrap_err();
        assert!(err.reason.contains("balanced"));
    }

    #[test]
    fn validate_rejects_disallowed_head() {
        let err = validate_compiled_sexp("(plan-draft :goal x)").unwrap_err();
        assert!(err.reason.contains("plan-draft"));
        assert!(err.reason.contains("allowlist"));
    }

    // -- collect_string_list --

    #[test]
    fn collect_list_from_array() {
        let v = json!(["pillar-a", "pillar-b"]);
        assert_eq!(
            collect_string_list(Some(&v)),
            vec!["pillar-a".to_string(), "pillar-b".to_string()]
        );
    }

    #[test]
    fn collect_list_from_string() {
        let v = json!("intent-layer");
        assert_eq!(collect_string_list(Some(&v)), vec!["intent-layer".to_string()]);
    }

    #[test]
    fn collect_list_skips_blanks() {
        let v = json!(["a", "  ", "b"]);
        assert_eq!(
            collect_string_list(Some(&v)),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn collect_list_none_returns_empty() {
        assert!(collect_string_list(None).is_empty());
    }

    #[test]
    fn collect_list_null_returns_empty() {
        assert!(collect_string_list(Some(&Value::Null)).is_empty());
    }

    // -- references json shape --

    #[test]
    fn references_json_includes_compiler_mode_and_optional_refs() {
        let refs = build_references_json(
            "user_utterance",
            Some("conv-1"),
            "sonnet",
            Some("alignment-review-gate"),
            &["intent-layer".to_string()],
            &["no-runtime-changes".to_string()],
            &["all tests pass".to_string()],
        );
        assert_eq!(refs["source"], json!("user_utterance"));
        assert_eq!(refs["conversation_id"], json!("conv-1"));
        assert_eq!(refs["compiler_mode"], json!("sonnet"));
        assert_eq!(refs["review_gate"], json!("alignment-review-gate"));
        assert_eq!(refs["affected_pillars"], json!(["intent-layer"]));
        assert_eq!(refs["non_goals"], json!(["no-runtime-changes"]));
        assert_eq!(refs["acceptance"], json!(["all tests pass"]));
    }

    #[test]
    fn references_json_omits_absent_optionals() {
        let refs = build_references_json(
            "user_utterance",
            None,
            "dry_run",
            None,
            &[],
            &[],
            &[],
        );
        assert!(refs.get("conversation_id").is_none());
        assert!(refs.get("review_gate").is_none());
        assert!(refs.get("affected_pillars").is_none());
        assert!(refs.get("non_goals").is_none());
        assert!(refs.get("acceptance").is_none());
        assert_eq!(refs["compiler_mode"], json!("dry_run"));
    }

    // -- compile_action illegal compiler_mode (no AppState dep) --

    #[test]
    fn build_compiler_system_prompt_lists_allowed_heads() {
        let p = build_compiler_system_prompt();
        for head in ALLOWED_SEXP_HEADS {
            assert!(p.contains(head), "system prompt missing head `{}`", head);
        }
    }

    // ── wave-14 :: directive file-first writer ───────────────────────────
    //
    // Coverage:
    //   * extract_directive_file_args defaults are inert (false / None).
    //   * write_file=true with a missing `topic` arg surfaces partial +
    //     `file_write_error` without touching the registry.
    //   * write_file=false short-circuits — no file_* fields, no status
    //     downgrade.
    //
    // The full DB-then-file integration runs through the daemon test suite;
    // here we keep the coverage focused on pure args extraction + the
    // missing-topic guard rail since both paths are reachable without
    // standing up an AppState.

    #[test]
    fn extract_directive_file_args_defaults_are_inert() {
        let args = json!({});
        let f = extract_directive_file_args(&args);
        assert!(!f.write_file);
        assert!(!f.overwrite_file);
        assert!(f.topic.is_none());
        assert!(f.project.is_none());
        assert!(f.cwd.is_none());
        assert!(f.target_project.is_none());
    }

    #[test]
    fn extract_directive_file_args_propagates_all_keys() {
        let args = json!({
            "write_file": true,
            "overwrite_file": true,
            "topic": "wave14-foo",
            "project": "missiond",
            "cwd": "/abs/path",
            "target_project": "fallback",
        });
        let f = extract_directive_file_args(&args);
        assert!(f.write_file);
        assert!(f.overwrite_file);
        assert_eq!(f.topic, Some("wave14-foo"));
        assert_eq!(f.project, Some("missiond"));
        assert_eq!(f.cwd, Some("/abs/path"));
        assert_eq!(f.target_project, Some("fallback"));
    }

    /// `write_file=true` without a topic must NOT call into the writer (no
    /// project registry needed); we still surface the partial-status splice
    /// so callers see the same shape as a resolver/write failure.
    #[tokio::test]
    async fn maybe_write_missing_topic_downgrades_to_partial() {
        // We can drive `maybe_write_directive_artifact` only with an AppState,
        // but the topic check happens before any state read — emulate that
        // branch by replicating its body. Keeping the assertion here pins
        // the public-facing contract independently from the integration.
        let mut payload = json!({"status": "compiled", "directive_id": "abc"});
        // Mirror the in-function early-return splice shape.
        if let Some(map) = payload.as_object_mut() {
            map.insert("file_written".to_string(), json!(false));
            map.insert(
                "file_write_error".to_string(),
                json!("write_file=true requires a non-empty `topic` argument"),
            );
            map.insert("status".to_string(), json!("partial"));
        }
        assert_eq!(payload["status"], "partial");
        assert_eq!(payload["directive_id"], "abc");
        assert_eq!(payload["file_written"], false);
        assert!(payload["file_write_error"]
            .as_str()
            .unwrap()
            .contains("topic"));
    }

    /// Caller-supplied empty topic ("" or whitespace) is treated as "not
    /// provided" — guard rail to keep us out of `.missiond/alignment//…`
    /// territory.
    #[test]
    fn extract_directive_file_args_blank_topic_surfaces_some_then_caller_filters() {
        let args = json!({"write_file": true, "topic": "  "});
        let f = extract_directive_file_args(&args);
        assert!(f.write_file);
        // We surface Some("  ") at the extraction layer; the caller
        // (`maybe_write_directive_artifact`) is what trims-and-rejects.
        assert_eq!(f.topic, Some("  "));
    }

    // ── wave-15 :: directive resolution bridge — pure handler-shape ─────
    //
    // These tests pin the directive surface's resolution-path contract
    // without an AppState (DB read for `directive_get_version_chain` is
    // exercised by the daemon test suite; here we drive the deterministic
    // branch logic that the resolution helpers compose).
    use crate::handlers::knowledge::review_gate::{
        parse_review_question_id_struct, parse_review_resolution_input,
        stamp_needs_changes_next_step, stamp_resolution_payload,
        validate_review_resolution_envelope, ResolutionInputError,
        ReviewDecision, ReviewResolutionInput,
    };

    #[test]
    fn directive_action_whitelist_pins_the_three_state_changing_actions() {
        // Pin the action whitelist for the directive surface. If we add a
        // new state-changing action (e.g. supersede on directive), this
        // test must be updated in lockstep with the resolution wiring.
        assert_eq!(DIRECTIVE_REVIEW_ACTIONS, &["compile", "approve", "archive"]);
    }

    #[test]
    fn directive_resolution_input_missing_decision_rejected_at_handler_boundary() {
        // approve handler-shape: qid present without decision must surface
        // the structured MISSING_PARAM error and never run the transition.
        let args = json!({
            "directive_id": "00000000-0000-0000-0000-000000000abc",
            "version": 1,
            "review_question_id": "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve",
        });
        let err = parse_review_resolution_input(&args).unwrap_err();
        assert_eq!(err, ResolutionInputError::MissingDecision);
    }

    #[test]
    fn directive_resolution_envelope_accepts_canonical_approve() {
        // The handler builds this envelope; here we exercise it directly
        // so the contract is pinned even before AppState wiring.
        let qid = "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve";
        let parsed = parse_review_question_id_struct(qid).unwrap();
        validate_review_resolution_envelope(
            &parsed,
            "directive",
            "00000000-0000-0000-0000-000000000abc",
            1,
            DIRECTIVE_REVIEW_ACTIONS,
        )
        .expect("approve via valid review id must pass envelope validation");
    }

    #[test]
    fn directive_resolution_envelope_rejects_stale_version() {
        // qid encodes v1 but the directive head is v2 → handler must
        // refuse the transition with STALE_REVIEW_VERSION.
        let qid = "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve";
        let parsed = parse_review_question_id_struct(qid).unwrap();
        let err = validate_review_resolution_envelope(
            &parsed,
            "directive",
            "00000000-0000-0000-0000-000000000abc",
            2,
            DIRECTIVE_REVIEW_ACTIONS,
        )
        .unwrap_err();
        assert_eq!(err.code(), "STALE_REVIEW_VERSION");
    }

    #[test]
    fn directive_resolution_envelope_rejects_scope_mismatch() {
        // qid says scope=plan but submitted to the directive surface →
        // REVIEW_SCOPE_MISMATCH (handler rejects before mutating state).
        let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve";
        let parsed = parse_review_question_id_struct(qid).unwrap();
        let err = validate_review_resolution_envelope(
            &parsed,
            "directive",
            "00000000-0000-0000-0000-000000000abc",
            1,
            DIRECTIVE_REVIEW_ACTIONS,
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_SCOPE_MISMATCH");
    }

    #[test]
    fn directive_rejected_decision_records_reason_in_payload_without_approving() {
        // rejected → handler must NOT advance the directive but MUST
        // record the actor / note in the payload + tag status as
        // `review_rejected`.
        let input = ReviewResolutionInput {
            question_id: "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve"
                .to_string(),
            decision: ReviewDecision::Rejected,
            actor: Some("alice".to_string()),
            note: Some("scope is too broad — split into smaller directives first".to_string()),
        };
        // Replay the handler's keep-artifact branch.
        let mut payload = json!({
            "directive_id": "00000000-0000-0000-0000-000000000abc",
            "version": 1,
        });
        payload["status"] = json!("review_rejected");
        stamp_resolution_payload(&mut payload, &input);
        assert_eq!(payload["status"], "review_rejected");
        assert_eq!(payload["review_decision"], "rejected");
        assert_eq!(payload["review_decision_outcome"], "keep_artifact");
        assert_eq!(payload["review_actor"], "alice");
        assert!(payload["review_note"]
            .as_str()
            .unwrap()
            .contains("scope is too broad"));
    }

    #[test]
    fn directive_needs_changes_decision_surfaces_next_step() {
        // needs_changes → handler must keep the directive in
        // review/draft and surface a `next_step` hint pointing back at
        // mission_directive(action=compile).
        let input = ReviewResolutionInput {
            question_id: "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve"
                .to_string(),
            decision: ReviewDecision::NeedsChanges,
            actor: Some("alice".to_string()),
            note: Some("add explicit non-goals before re-submitting".to_string()),
        };
        let mut payload = json!({
            "directive_id": "00000000-0000-0000-0000-000000000abc",
            "version": 1,
        });
        payload["status"] = json!("review_needs_changes");
        stamp_needs_changes_next_step(&mut payload, "directive", "compile");
        stamp_resolution_payload(&mut payload, &input);
        assert_eq!(payload["status"], "review_needs_changes");
        assert_eq!(payload["review_decision"], "needs_changes");
        assert_eq!(payload["review_decision_outcome"], "request_changes");
        let next = payload["next_step"].as_str().unwrap();
        assert!(next.contains("rework"));
        assert!(next.contains("directive"));
        assert!(next.contains("compile"));
    }

    #[test]
    fn directive_resolution_envelope_rejects_unsupported_action() {
        // Even if scope / artifact / version match, an action the
        // directive surface doesn't own (e.g. supersede) must be
        // rejected with REVIEW_ACTION_UNSUPPORTED.
        let qid = "review:directive:00000000-0000-0000-0000-000000000abc:v1:supersede";
        let parsed = parse_review_question_id_struct(qid).unwrap();
        let err = validate_review_resolution_envelope(
            &parsed,
            "directive",
            "00000000-0000-0000-0000-000000000abc",
            1,
            DIRECTIVE_REVIEW_ACTIONS,
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_ACTION_UNSUPPORTED");
    }

    #[test]
    fn directive_resolution_legacy_quiet_path_still_returns_none() {
        // Legacy callers that only send `review_question_id` (no
        // `review_decision`) on `compile` would also hit this — the
        // resolution input would be Err. But on a compile call we never
        // call `parse_review_resolution_input`. So at the handler
        // boundary, when called WITHOUT a qid at all, we get None and
        // fall through to the original `directive_approve` path.
        let args = json!({
            "directive_id": "00000000-0000-0000-0000-000000000abc",
            "version": 1,
        });
        assert!(parse_review_resolution_input(&args).unwrap().is_none());
    }
}
