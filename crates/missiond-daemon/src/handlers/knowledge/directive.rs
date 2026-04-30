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
    apply_compile_review_gates, build_llm_auto_approve_proposal_system_prompt,
    build_llm_auto_approve_proposal_user_prompt, enforce_apply_gate_preflight,
    enforce_proposal_invariants, evaluate_llm_approve_apply_gate, evaluate_review_automation,
    llm_auto_approve_proposal_mode_was_explicit, maybe_emit_review_question_resolved,
    parse_compile_review_gate, parse_llm_approve_apply_gate_input, parse_llm_auto_approve_proposal,
    parse_llm_auto_approve_proposal_mode, parse_resolution_review_question_id,
    parse_review_automation_policy, parse_review_gate_policy, parse_review_question_id_struct,
    parse_review_resolution_input, resolution_wire_string, review_automation_policy_was_explicit,
    review_gate_policy_was_explicit, stamp_llm_approve_apply_gate_payload,
    stamp_llm_auto_approve_proposal_payload, stamp_needs_changes_next_step,
    stamp_proposal_hash_payload, stamp_resolution_payload, stamp_review_automation_payload,
    validate_review_resolution_envelope, AutomationStatus, LlmApproveApplyGateInput,
    LlmAutoApproveProposalBundle, LlmAutoApproveProposalMode, LlmAutoApproveProposalStatus,
    ParsedReviewQuestionId, ResolutionOutcome, ReviewAutomationContext, ReviewAutomationPolicy,
    ReviewDecision, ReviewResolutionInput,
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
                .with_suggestion("actions: compile|list|get|approve|archive|version_chain"),
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
            .with_suggestion("valid: compile|list|get|approve|archive|version_chain"),
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

fn enrich_persisted_directive_sexp(sexp: &str, directive_id: &str, version: i32) -> String {
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
            ToolError::new(
                error_codes::NOT_FOUND,
                format!("directive `{}` not found", id),
            )
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

/// Build the wave-18 / task 07 deterministic safety context for the
/// directive surface. Pure projection of the directive row + caller args.
///
/// Rules of thumb:
///   * `deterministic_mode` = `compiler_model.is_none()` (dry-run leaves
///     it unset; sonnet records `claude-sonnet`). LLM-driven artifacts
///     ALWAYS block `auto_safe` — refusing to auto-approve unattended
///     LLM output is the load-bearing safety contract.
///   * `protected_source_or_target` is currently `false` for directive —
///     directive surface has no merge source/target concept (that lives
///     on capability_usage). Future work: lift `protected_pillars`
///     from references_json once the capability protected list pivots
///     into the directive layer; for v0 we keep the rule loud-but-passing.
///   * File hashing flows through caller-supplied `expected_file_sha256`
///     (none today — directive compile never round-trips a hash through
///     approve/archive). The handler still honours it if a future caller
///     supplies the hint.
///   * `additional_blockers` carries any caller-side warnings the
///     handler already detected (currently empty — the wave-15 envelope
///     validators run BEFORE this, and any failure short-circuits the
///     whole resolution path before the policy fires).
fn build_directive_automation_ctx(
    args: &Value,
    directive_compiler_model: Option<&str>,
) -> ReviewAutomationContext {
    ReviewAutomationContext {
        deterministic_mode: directive_compiler_model.is_none(),
        // Directive approve/archive never re-touches the file artifact —
        // the wave-14 file-first writer ran during compile. So no file
        // write is "attempted" at the resolution boundary.
        file_write_attempted: false,
        file_write_succeeded: false,
        actual_file_sha256: None,
        expected_file_sha256: args
            .get("expected_file_sha256")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        protected_source_or_target: false,
        additional_blockers: Vec::new(),
    }
}

// ───────────────────────────────────────────────────────────────────────
// wave-21 / task 06 — LLM auto-approve proposal v0 (directive surface)
//
// Conservative wiring on top of the existing wave-15 / 18 / 20 stack.
// The new `auto_approve_mode` knob is OPT-IN (default `off`); when
// `sonnet_suggest` is supplied we invoke Sonnet for a propose-only
// review-action recommendation and surface it under
// `llm_auto_approve_proposal*` fields on the response payload. The
// proposal NEVER drives a DB transition or bus emission in v0 —
// `applied=false` is pinned on every payload (invariant I3) and
// `requires_human=true` is forced regardless of model output.
//
// Destructive actions (`archive`) ALWAYS short-circuit to
// `destructive_blocked`: the proposal value is preserved for audit but
// the response carries `requires_human=true` AND a loud warning. This
// matches the wave-18 / 07 archive-never-auto-promoted contract for the
// deterministic policy path.
//
// Sonnet unavailable surfaces `LLM_UNAVAILABLE` status with no fallback
// proposal — invariant I4 forbids silent degradation to deterministic.
// ───────────────────────────────────────────────────────────────────────

const DIRECTIVE_REVIEW_PROPOSER_CALLER: &str = "directive_review_proposer";
const SONNET_PROPOSER_MAX_TOKENS: u32 = 1024;

/// Run the wave-21 / task 06 propose-only Sonnet pass for the directive
/// surface. Returns a fully-built [`LlmAutoApproveProposalBundle`] in
/// every code path so the caller can pivot on the bundle status without
/// branching on Result. NEVER mutates state.
async fn request_directive_auto_approve_proposal(
    state: &AppState,
    mode: LlmAutoApproveProposalMode,
    action: &str,
    artifact_id: &uuid::Uuid,
    version: i32,
    deterministic_summary: &Value,
    artifact_digest: Option<&str>,
) -> LlmAutoApproveProposalBundle {
    // Invariant I2 short-circuit — destructive actions never drive a
    // Sonnet call in v0. We surface `destructive_blocked` directly so
    // dashboards can grep for the refusal without reading per-handler
    // state.
    if crate::handlers::knowledge::review_gate::is_destructive_review_action(action) {
        return LlmAutoApproveProposalBundle::destructive_blocked(
            mode,
            action,
            DIRECTIVE_REVIEW_PROPOSER_CALLER,
            None,
            format!(
                "rule:destructive_action:`{}` is destructive; auto-approve proposal NEVER promotes (invariant I2)",
                action.to_ascii_lowercase()
            ),
        );
    }

    // Invariant I4 — Sonnet unavailable surfaces `Unavailable` with no
    // fallback proposal. We mirror the directive_compile dry-run rule
    // here for consistency with the rest of the file.
    let Some(sonnet) = state.sonnet.as_ref() else {
        return LlmAutoApproveProposalBundle::unavailable(
            mode,
            action,
            DIRECTIVE_REVIEW_PROPOSER_CALLER,
            "Sonnet gateway not initialized; LLM auto-approve proposal unavailable",
        );
    };

    let system = build_llm_auto_approve_proposal_system_prompt();
    let user = build_llm_auto_approve_proposal_user_prompt(
        "directive",
        action,
        &artifact_id.to_string(),
        version,
        deterministic_summary,
        artifact_digest,
    );
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user,
        },
    ];

    let raw = match sonnet
        .call_interactive(
            messages,
            Some(SONNET_PROPOSER_MAX_TOKENS),
            DIRECTIVE_REVIEW_PROPOSER_CALLER,
        )
        .await
    {
        Ok(s) => s,
        Err(err) => {
            return LlmAutoApproveProposalBundle::unavailable(
                mode,
                action,
                DIRECTIVE_REVIEW_PROPOSER_CALLER,
                format!("Sonnet auto-approve proposal call failed: {}", err),
            );
        }
    };

    let (proposal, parse_warnings) = parse_llm_auto_approve_proposal(&raw);
    match proposal {
        Some(mut p) => {
            // Pin the deterministic invariants (destructive_check +
            // requires_human always true in v0).
            enforce_proposal_invariants(&mut p, action);
            LlmAutoApproveProposalBundle {
                mode,
                status: LlmAutoApproveProposalStatus::Suggested,
                proposal: Some(p),
                proposal_warnings: parse_warnings,
                unavailable_reason: None,
                action: action.to_string(),
                request_caller: Some(DIRECTIVE_REVIEW_PROPOSER_CALLER.to_string()),
                model: Some(SONNET_COMPILER_MODEL.to_string()),
            }
        }
        None => LlmAutoApproveProposalBundle {
            mode,
            status: LlmAutoApproveProposalStatus::NoSuggestion,
            proposal: None,
            proposal_warnings: parse_warnings,
            unavailable_reason: None,
            action: action.to_string(),
            request_caller: Some(DIRECTIVE_REVIEW_PROPOSER_CALLER.to_string()),
            model: Some(SONNET_COMPILER_MODEL.to_string()),
        },
    }
}

/// Splice the wave-21 / task 06 bundle onto a response payload. Skips
/// the splice when the bundle is `not_invoked` (`mode=off`) so legacy
/// callers stay byte-identical with pre-wave-21.
fn attach_directive_proposal_block(payload: &mut Value, bundle: &LlmAutoApproveProposalBundle) {
    if matches!(bundle.status, LlmAutoApproveProposalStatus::NotInvoked) {
        return;
    }
    stamp_llm_auto_approve_proposal_payload(payload, bundle);
}

/// Wave-22 / task 03 :: stamp the proposal hash + apply-gate outcome
/// onto the response payload. Pure / no DB mutation. The hash is
/// always stamped when the bundle carries a proposal (so callers can
/// echo it back via `proposal_hash` under `apply_llm_auto_approve=true`
/// without re-deriving it themselves). The apply-gate block is only
/// stamped when the gate was requested (preserving wave-21 byte-shape).
fn attach_directive_apply_gate_block(
    payload: &mut Value,
    bundle: &LlmAutoApproveProposalBundle,
    input: &LlmApproveApplyGateInput,
    artifact_id: &uuid::Uuid,
    version: i32,
) -> crate::handlers::knowledge::review_gate::LlmApproveApplyGateOutcome {
    stamp_proposal_hash_payload(
        payload,
        bundle,
        &bundle.action,
        &artifact_id.to_string(),
        version,
    );
    let outcome = evaluate_llm_approve_apply_gate(
        input,
        bundle,
        &bundle.action,
        &artifact_id.to_string(),
        version,
    );
    stamp_llm_approve_apply_gate_payload(payload, &outcome);
    outcome
}

/// Read + validate the wave-21 / task 06 mode arg. Returns
/// `Ok(None)` when the caller did not opt in (mode=off OR field absent),
/// `Ok(Some(mode))` when sonnet_suggest is requested. Strict-enum: typo
/// values fail-fast with a structured error so callers never silently
/// degrade to off.
fn parse_proposer_mode_or_error(
    args: &Value,
) -> std::result::Result<Option<LlmAutoApproveProposalMode>, ToolError> {
    let mode = parse_llm_auto_approve_proposal_mode(args)
        .map_err(|msg| ToolError::new(error_codes::INVALID_PARAM, msg))?;
    if mode.is_sonnet_suggest() {
        Ok(Some(mode))
    } else {
        // Echo `Off` only when the caller explicitly supplied it (not
        // when the field was absent) so dashboards can tell the
        // difference between "caller opted out" and "caller never saw
        // the knob". For absent / off, we omit the bundle entirely to
        // preserve byte-shape.
        if llm_auto_approve_proposal_mode_was_explicit(args) {
            Ok(Some(mode))
        } else {
            Ok(None)
        }
    }
}

/// Build the deterministic summary block that feeds into the Sonnet
/// prompt. Tiny / pure / no I/O. Only the fields the model actually
/// needs surface here so the prompt stays terse.
fn directive_proposer_summary(
    automation_outcome_status: &str,
    automation_policy: &str,
    decision_present: bool,
) -> Value {
    json!({
        "review_automation_policy": automation_policy,
        "review_automation_status": automation_outcome_status,
        "explicit_decision_supplied": decision_present,
    })
}

async fn action_approve(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "directive_id")?;
    let version = require_i32(args, "version")?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front. Strict-enum failure surfaces as INVALID_PARAM
    // BEFORE any DB read so caller typos never silently degrade to off.
    let proposer_mode = match parse_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front. Strict
    // shape errors fail-fast as INVALID_PARAM BEFORE any DB read.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => return Ok(ToolResult::structured_error(ToolError::new(code, msg))),
    };

    // wave-15 :: explicit resolution bridge. When the caller supplies
    // `review_question_id` + `review_decision` we validate the envelope
    // BEFORE mutating state. `Rejected` / `NeedsChanges` skip the DB
    // transition entirely (the artifact stays non-approved); `Approved`
    // proceeds with the existing manager transition.
    //
    // wave-18 / task 07 :: when a non-Manual `review_automation_policy`
    // is supplied without an explicit `review_decision` (which would
    // otherwise fail-fast with MISSING_PARAM), we promote the qid into
    // a policy-driven evaluation path. Caller-supplied decisions ALWAYS
    // win over the policy.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            // wave-18 / task 07: under a non-Manual policy with a qid
            // but no decision, route through the policy bridge instead
            // of fail-fast — the policy may auto-resolve or surface a
            // suggestion. Other parse errors (UnknownDecision) still
            // fail-fast because they signal caller typos.
            if matches!(automation_policy, ReviewAutomationPolicy::Manual)
                || !matches!(
                    e,
                    crate::handlers::knowledge::review_gate::ResolutionInputError::MissingDecision
                )
            {
                return Ok(ToolResult::structured_error(ToolError::new(
                    e.code(),
                    e.message(),
                )));
            }
            let qid = parse_resolution_review_question_id(args)
                .expect("MissingDecision implies qid was present");
            return action_approve_with_policy_only(
                state,
                id,
                version,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_approve_with_resolution(
            state,
            id,
            version,
            input,
            automation_policy,
            automation_explicit,
            proposer_mode,
            apply_gate_input,
        )
        .await;
    }

    // wave-22 / task 03 :: when the caller opted into the apply gate,
    // we INVERT the legacy unconditional approve. The DB transition is
    // gated on the LLM proposal passing all 6 strict gates. If the gate
    // skips for any reason, we leave the directive untouched and surface
    // the structured `apply_gate` block explaining why. Caller did not
    // supply a `review_decision` here (that branch was handled above);
    // they delegated authority to the LLM gate.
    //
    // Hash mismatch / missing fail-fast as structured errors BEFORE any
    // DB read or mutation per the contract. The proposer mode MUST also
    // be supplied (otherwise no proposal exists to apply) — but we let
    // the gate produce `skipped_no_proposal` rather than reject up-front
    // so the audit trail explains the misconfiguration loudly.
    if apply_gate_input.apply {
        // Build the proposal first so the preflight has something to
        // hash against. We honour the caller-supplied proposer_mode
        // (typically `sonnet_suggest`); when the caller flipped the
        // apply gate but forgot to supply the proposer mode, we still
        // route through `request_directive_auto_approve_proposal` with
        // `Off` so the bundle reports `not_invoked` and the gate
        // surfaces `skipped_no_proposal`.
        let resolved_mode = proposer_mode.unwrap_or(LlmAutoApproveProposalMode::Off);
        let summary = directive_proposer_summary("legacy_quiet", "manual", false);
        let bundle = request_directive_auto_approve_proposal(
            state,
            resolved_mode,
            "approve",
            &id,
            version,
            &summary,
            None,
        )
        .await;
        // Strict pre-flight (hash check). Structured-error path leaves
        // the directive UNMUTATED (per contract: "On mismatch or missing
        // proposal hash, return structured error and do not mutate
        // directive/plan/review state.").
        if let Err((code, msg)) = enforce_apply_gate_preflight(
            &apply_gate_input,
            &bundle,
            "approve",
            &id.to_string(),
            version,
        ) {
            return Ok(ToolResult::structured_error(ToolError::new(code, msg)));
        }
        let mut payload = json!({
            "directive_id": id,
            "version": version,
        });
        // Always stamp the proposal block + hash for audit.
        attach_directive_proposal_block(&mut payload, &bundle);
        let outcome = attach_directive_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            version,
        );
        if outcome.status.should_apply() {
            state
                .store
                .directive_approve(id, version)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("approved");
            payload["resolution_source"] = json!("llm_approve_apply_gate");
            let qid = parse_resolution_review_question_id(args);
            maybe_emit_review_question_resolved(
                &mut payload,
                &state.bus,
                qid.as_deref(),
                "approved",
                None,
            )
            .await;
        } else {
            // Gate skipped — directive STAYS at its current status. We
            // surface the next_step so the caller can either tighten
            // the gate args or fall back to an explicit decision.
            payload["status"] = json!("llm_auto_apply_skipped");
            payload["next_step"] = json!(format!(
                "apply gate did not authorise (status={}); supply explicit `review_decision=approved` to flip the directive manually OR re-run with a matching proposal_hash + caller_approved=true",
                outcome.status.as_str()
            ));
        }
        return Ok(ToolResult::json_pretty(&payload));
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
    maybe_emit_review_question_resolved(&mut payload, &state.bus, qid.as_deref(), "approved", None)
        .await;
    // wave-21 / task 06 :: propose-only Sonnet pass on the legacy path.
    // The DB transition already committed; the proposal is informational.
    if let Some(mode) = proposer_mode {
        let summary = directive_proposer_summary("legacy_quiet", "manual", false);
        let bundle = request_directive_auto_approve_proposal(
            state, mode, "approve", &id, version, &summary, None,
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: stamp the proposal hash so callers can
        // echo it back via `proposal_hash` under
        // `apply_llm_auto_approve=true` on a follow-up call without
        // re-deriving it. Apply gate is NOT requested here (we already
        // confirmed that above), so `attach_directive_apply_gate_block`
        // would be a no-op for the gate block — but the hash stamp is
        // still useful so we call it inline.
        stamp_proposal_hash_payload(&mut payload, &bundle, "approve", &id.to_string(), version);
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=approve`. Validates the
/// review envelope (scope / artifact / version / action), then performs
/// the manager transition only when the decision is `approved`.
///
/// wave-18 / task 07 :: also evaluates the deterministic
/// `review_automation_policy` and stamps the suggestion / status onto
/// the response payload. Caller-supplied `review_decision` ALWAYS wins;
/// the automation outcome is informational under that path
/// (`status=overridden_by_explicit_decision`).
async fn action_approve_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    version: i32,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    // Parse the deterministic id envelope.
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )))
        }
    };
    // Source the current artifact + version from the chain head so the
    // staleness check is anchored to the latest persisted draft / version.
    let chain = state
        .store
        .directive_get_version_chain(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let head_directive = match chain.iter().last() {
        Some(d) => d.clone(),
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("directive `{}` not found for resolution", id),
            )))
        }
    };
    let current_version = head_directive.version;
    if version != current_version {
        return Ok(ToolResult::structured_error(ToolError::new(
            "STALE_REVIEW_VERSION",
            format!(
                "approve `version=v{}` does not match directive `{}` head `v{}`",
                version, id, current_version
            ),
        )));
    }
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "directive",
        &id.to_string(),
        current_version,
        DIRECTIVE_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
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

    // wave-18 / task 07 :: surface the automation outcome AFTER stamping
    // the explicit decision so observers can see both. Skipped under
    // Manual + caller-omitted policy to keep pre-wave-18 callers
    // byte-identical.
    let mut automation_status_label = "not_evaluated".to_string();
    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert(
                "review_automation_policy".into(),
                json!(automation_policy.as_str()),
            );
        }
        let ctx = build_directive_automation_ctx(&args_v, head_directive.compiler_model.as_deref());
        let outcome = evaluate_review_automation(automation_policy, &ctx, Some(input.decision));
        automation_status_label = outcome.status.as_str().to_string();
        stamp_review_automation_payload(&mut payload, &outcome);
    }

    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;

    // wave-21 / task 06 :: propose-only Sonnet pass for the explicit-
    // resolution path. Caller-supplied `review_decision` ALWAYS wins
    // (the policy NEVER overrides explicit decisions) — this proposal
    // is informational only. Skipped under `mode=off` (default).
    //
    // wave-22 / task 03 :: when caller ALSO opted into the apply gate,
    // we stamp the gate block as INFORMATIONAL ONLY on this path. The
    // explicit `review_decision` already drove the DB transition above
    // (or refused to). The gate's verdict is recorded for audit
    // symmetry but NEVER overrides the human decision. The pre-flight
    // hash check still fail-fasts so the caller can never fool the
    // dashboard with a stale hash echo.
    if let Some(mode) = proposer_mode {
        let summary =
            directive_proposer_summary(&automation_status_label, automation_policy.as_str(), true);
        let bundle = request_directive_auto_approve_proposal(
            state,
            mode,
            "approve",
            &id,
            version,
            &summary,
            Some(&head_directive.sexp_text),
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
        // Stamp the gate block as INFORMATIONAL on this path. Caller-
        // supplied explicit `review_decision` is the authority that
        // already drove (or refused) the DB transition above, so we do
        // NOT fail-fast on hash mismatch here — that would lie about
        // state. Instead the gate surfaces `proposal_hash_status` =
        // mismatch / not_supplied so the caller sees the inconsistency
        // without us pretending to have rolled back. The hash itself
        // is still stamped so a follow-up call can echo it back.
        let _ = attach_directive_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-18 / task 07 :: policy-driven approve path. Invoked when the
/// caller supplies `review_question_id` + `review_automation_policy`
/// (non-Manual) WITHOUT an explicit `review_decision`. Evaluates the
/// deterministic safety inspector and either auto-promotes to
/// `Approved` (auto_safe + every rule passes) or surfaces the suggestion
/// without mutating (suggest, or auto_safe with any blocking rule).
///
/// NEVER calls an LLM. NEVER auto-rejects. The wave-15 envelope
/// validators run BEFORE the policy fires.
async fn action_approve_with_policy_only(
    state: &AppState,
    id: uuid::Uuid,
    version: i32,
    qid: String,
    automation_policy: ReviewAutomationPolicy,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    // Parse + validate the envelope before invoking the policy. Same
    // rejection set as the wave-15 explicit path so a malformed id can
    // never sneak past via the automation knob.
    let parsed = match parse_review_question_id_struct(&qid) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )))
        }
    };
    let chain = state
        .store
        .directive_get_version_chain(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let head_directive = match chain.iter().last() {
        Some(d) => d.clone(),
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("directive `{}` not found for resolution", id),
            )))
        }
    };
    let current_version = head_directive.version;
    if version != current_version {
        return Ok(ToolResult::structured_error(ToolError::new(
            "STALE_REVIEW_VERSION",
            format!(
                "approve `version=v{}` does not match directive `{}` head `v{}`",
                version, id, current_version
            ),
        )));
    }
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "directive",
        &id.to_string(),
        current_version,
        DIRECTIVE_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "directive_id": id,
        "version": version,
        "review_question_id": qid,
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let ctx = build_directive_automation_ctx(&args_v, head_directive.compiler_model.as_deref());
    // No caller decision here — this whole branch fires precisely when
    // the caller omitted `review_decision`.
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    if outcome.may_auto_resolve {
        // Safety inspector cleared every rule under `auto_safe`. Run the
        // existing approve transition.
        state
            .store
            .directive_approve(id, version)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["status"] = json!("approved");
        payload["resolution_source"] = json!("review_automation_policy");
    } else {
        // Suggest-only OR auto_safe blocked. NO mutation. Surface the
        // suggestion + reasons; downstream caller decides next step.
        payload["status"] = json!("review_pending_decision");
        if matches!(outcome.status, AutomationStatus::AutoSafeBlocked) {
            payload["next_step"] = json!(
                "auto_safe blocked — supply explicit `review_decision` (approved|rejected|needs_changes) to flip the directive"
            );
        } else {
            payload["next_step"] = json!(
                "suggest mode is informational — supply explicit `review_decision` to flip the directive"
            );
        }
    }

    stamp_review_automation_payload(&mut payload, &outcome);

    if outcome.may_auto_resolve {
        // Best-effort emit a Resolved event so the inbound subscriber
        // sees the auto-approval — same fire-and-forget bus contract as
        // the wave-15 explicit path.
        maybe_emit_review_question_resolved(&mut payload, &state.bus, Some(&qid), "approved", None)
            .await;
    }

    // wave-21 / task 06 :: propose-only Sonnet pass on the policy-only
    // path. The deterministic policy outcome (auto_safe / suggest /
    // blocked) is informational input to the prompt; the proposal NEVER
    // overrides the deterministic decision — both surfaces co-exist.
    //
    // wave-22 / task 03 :: when caller ALSO opted into the apply gate
    // on the policy-only path, the gate is INFORMATIONAL ONLY here. The
    // deterministic policy already drove (or refused) the DB transition
    // above. Caller-supplied apply_llm_auto_approve+caller_approved
    // does NOT override the deterministic safety inspector — they are
    // independent guards and BOTH must agree to mutate. The gate's
    // verdict is recorded for audit symmetry; the policy's verdict
    // already determined whether we ran `directive_approve`.
    if let Some(mode) = proposer_mode {
        let summary =
            directive_proposer_summary(outcome.status.as_str(), automation_policy.as_str(), false);
        let bundle = request_directive_auto_approve_proposal(
            state,
            mode,
            "approve",
            &id,
            version,
            &summary,
            Some(&head_directive.sexp_text),
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
        let _ = attach_directive_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            version,
        );
    }

    Ok(ToolResult::json_pretty(&payload))
}

async fn action_archive(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "directive_id")?;
    let version = require_i32(args, "version")?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front. Strict-enum failure surfaces as INVALID_PARAM.
    let proposer_mode = match parse_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front. archive
    // is destructive — the gate will ALWAYS skip with
    // `SkippedDestructiveAction` (invariant I2) — but we still parse
    // strict shape so caller typos surface as INVALID_PARAM here too.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => return Ok(ToolResult::structured_error(ToolError::new(code, msg))),
    };

    // wave-15 :: explicit resolution bridge — see `action_approve` above.
    // wave-18 / task 07 :: same MissingDecision-under-policy promotion as
    // approve so the policy can drive resolution under archive too.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            if matches!(automation_policy, ReviewAutomationPolicy::Manual)
                || !matches!(
                    e,
                    crate::handlers::knowledge::review_gate::ResolutionInputError::MissingDecision
                )
            {
                return Ok(ToolResult::structured_error(ToolError::new(
                    e.code(),
                    e.message(),
                )));
            }
            // archive auto-transition under auto_safe is intentionally
            // NOT supported — archiving is a destructive transition that
            // we never want auto-promoted from a policy. The handler
            // therefore surfaces the suggestion only, never mutates.
            let qid = parse_resolution_review_question_id(args)
                .expect("MissingDecision implies qid was present");
            return action_archive_with_policy_only(
                state,
                id,
                version,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_archive_with_resolution(
            state,
            id,
            version,
            input,
            automation_policy,
            automation_explicit,
            proposer_mode,
            apply_gate_input,
        )
        .await;
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
    maybe_emit_review_question_resolved(&mut payload, &state.bus, qid.as_deref(), "archived", None)
        .await;
    // wave-21 / task 06 :: archive is destructive — the proposer
    // ALWAYS short-circuits to `destructive_blocked` (invariant I2).
    if let Some(mode) = proposer_mode {
        let summary = directive_proposer_summary("legacy_quiet", "manual", false);
        let bundle = request_directive_auto_approve_proposal(
            state, mode, "archive", &id, version, &summary, None,
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: archive is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2). Stamp
        // the gate block + hash for audit symmetry.
        let _ = attach_directive_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=archive`. Same envelope
/// validation as approve; on `approved` decision we perform the archive
/// transition; on `rejected`/`needs_changes` the directive stays at its
/// current status.
///
/// wave-18 / task 07 :: also evaluates the deterministic
/// `review_automation_policy` and stamps the suggestion / status onto
/// the response payload. Caller-supplied `review_decision` ALWAYS wins.
async fn action_archive_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    version: i32,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )))
        }
    };
    let chain = state
        .store
        .directive_get_version_chain(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let head_directive = match chain.iter().last() {
        Some(d) => d.clone(),
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("directive `{}` not found for resolution", id),
            )))
        }
    };
    let current_version = head_directive.version;
    if version != current_version {
        return Ok(ToolResult::structured_error(ToolError::new(
            "STALE_REVIEW_VERSION",
            format!(
                "archive `version=v{}` does not match directive `{}` head `v{}`",
                version, id, current_version
            ),
        )));
    }
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "directive",
        &id.to_string(),
        current_version,
        DIRECTIVE_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
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

    // wave-18 / task 07 :: surface the automation outcome AFTER stamping
    // the explicit decision so observers see both. Skipped under Manual +
    // caller-omitted policy to keep pre-wave-18 callers byte-identical.
    let mut automation_status_label = "not_evaluated".to_string();
    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert(
                "review_automation_policy".into(),
                json!(automation_policy.as_str()),
            );
        }
        let ctx = build_directive_automation_ctx(&args_v, head_directive.compiler_model.as_deref());
        let outcome = evaluate_review_automation(automation_policy, &ctx, Some(input.decision));
        automation_status_label = outcome.status.as_str().to_string();
        stamp_review_automation_payload(&mut payload, &outcome);
    }

    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    // wave-21 / task 06 :: archive is destructive — proposer ALWAYS
    // surfaces `destructive_blocked` regardless of caller decision.
    if let Some(mode) = proposer_mode {
        let summary =
            directive_proposer_summary(&automation_status_label, automation_policy.as_str(), true);
        let bundle = request_directive_auto_approve_proposal(
            state,
            mode,
            "archive",
            &id,
            version,
            &summary,
            Some(&head_directive.sexp_text),
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: archive is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` regardless of any other
        // gate field. Caller-supplied `review_decision` is the
        // authority; the gate is informational on this path.
        let _ = attach_directive_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-18 / task 07 :: policy-driven archive path. Fires when the
/// caller supplies `review_question_id` + `review_automation_policy`
/// (non-Manual) WITHOUT an explicit `review_decision`.
///
/// IMPORTANT: archive auto-transition is intentionally NEVER promoted
/// under `auto_safe`. Archiving is destructive (the directive cannot
/// flow into plan compile from the archived state); we never want a
/// safety inspector to silently retire a draft. The handler therefore
/// surfaces the suggestion only and refuses to mutate, regardless of
/// whether every safety rule passes.
async fn action_archive_with_policy_only(
    state: &AppState,
    id: uuid::Uuid,
    version: i32,
    qid: String,
    automation_policy: ReviewAutomationPolicy,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&qid) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )))
        }
    };
    let chain = state
        .store
        .directive_get_version_chain(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let head_directive = match chain.iter().last() {
        Some(d) => d.clone(),
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("directive `{}` not found for resolution", id),
            )))
        }
    };
    let current_version = head_directive.version;
    if version != current_version {
        return Ok(ToolResult::structured_error(ToolError::new(
            "STALE_REVIEW_VERSION",
            format!(
                "archive `version=v{}` does not match directive `{}` head `v{}`",
                version, id, current_version
            ),
        )));
    }
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "directive",
        &id.to_string(),
        current_version,
        DIRECTIVE_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "directive_id": id,
        "version": version,
        "review_question_id": qid,
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let mut ctx = build_directive_automation_ctx(&args_v, head_directive.compiler_model.as_deref());
    // Pin the loud "destructive" blocker so even auto_safe + every rule
    // passing still surfaces a clear refusal — archive is destructive
    // and we never want the policy to silently retire a draft.
    ctx.additional_blockers.push(
        "destructive_action:archive transitions are never auto-promoted by the automation policy"
            .to_string(),
    );
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    payload["status"] = json!("review_pending_decision");
    payload["next_step"] = json!(
        "supply explicit `review_decision` (approved|rejected|needs_changes) to archive — archive is destructive and never auto-promoted"
    );

    stamp_review_automation_payload(&mut payload, &outcome);

    // wave-21 / task 06 :: archive is destructive — proposer ALWAYS
    // surfaces `destructive_blocked`. The deterministic policy already
    // refused to mutate above; the LLM proposer mirrors that refusal.
    if let Some(mode) = proposer_mode {
        let summary =
            directive_proposer_summary(outcome.status.as_str(), automation_policy.as_str(), false);
        let bundle = request_directive_auto_approve_proposal(
            state,
            mode,
            "archive",
            &id,
            version,
            &summary,
            Some(&head_directive.sexp_text),
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: archive is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2).
        let _ = attach_directive_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            version,
        );
    }

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
// wave-16 :: subscriber-side resolution bridge
//
// Called by `bus::v2_subscribers::spawn_review_resolution_sub` after the
// pure planner classified the inbound `QuestionEvent::Resolved` event as a
// directive route. We re-validate the envelope (so a stale qid resolved
// against a since-updated directive bails loudly) and, ONLY for an
// `Approved` decision on a transition action, perform the same DB
// transition as the explicit caller-side bridge. We never re-publish a
// Resolved bus event — the inbound event we just consumed IS that signal.
// `Rejected` / `NeedsChanges` / `compile`-action ids never mutate state.
// ───────────────────────────────────────────────────────────────────────

/// Outcome of routing a `QuestionEvent::Resolved` event through the
/// directive-side bridge. Surfaced to the subscriber so it can record
/// observability without re-doing the match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectiveSubscriberOutcome {
    /// Decision was `Approved` on `approve` action and the directive was
    /// transitioned to Approved (idempotent: no-op if already Approved).
    Approved,
    /// Decision was `Approved` on `archive` action and the directive was
    /// transitioned to Archived.
    Archived,
    /// Decision was `Rejected` or `NeedsChanges`; we left the directive
    /// at its current status.
    KeptArtifact { decision: ReviewDecision },
    /// Action was `compile` — there is no manager transition tied to
    /// the compile path itself (compile happens before review). We just
    /// log the resolution decision.
    CompileNoOp { decision: ReviewDecision },
    /// The qid envelope's `artifact_id` did not parse as a UUID.
    ArtifactIdNotUuid { artifact_id: String, error: String },
    /// Directive row was not found for the qid's artifact_id.
    NotFound { artifact_id: uuid::Uuid },
    /// Envelope failed re-validation (scope / version / action). Carries
    /// the error code so the subscriber can log a structured warning.
    EnvelopeRejected { code: &'static str, message: String },
    /// Underlying DB transition failed; the original `Resolved` event has
    /// already been consumed, so we surface the error as observability.
    DbError { detail: String },
}

/// Re-route a `QuestionEvent::Resolved` event whose envelope was parsed
/// as `scope=directive` through the same validators as the explicit
/// caller-side bridge. Performs the manager transition for an `Approved`
/// decision on an action in [`DIRECTIVE_REVIEW_ACTIONS`] except
/// `compile`. Pure side-effects: at most one DB write; no bus publish.
pub(crate) async fn handle_review_resolved_event(
    state: &AppState,
    parsed: &ParsedReviewQuestionId,
    decision: ReviewDecision,
) -> DirectiveSubscriberOutcome {
    let id = match uuid::Uuid::parse_str(&parsed.artifact_id) {
        Ok(u) => u,
        Err(e) => {
            return DirectiveSubscriberOutcome::ArtifactIdNotUuid {
                artifact_id: parsed.artifact_id.clone(),
                error: e.to_string(),
            }
        }
    };
    let chain = match state.store.directive_get_version_chain(id).await {
        Ok(c) => c,
        Err(e) => {
            return DirectiveSubscriberOutcome::DbError {
                detail: format!("directive_get_version_chain: {}", e),
            }
        }
    };
    let current_version = match chain.iter().last() {
        Some(d) => d.version,
        None => return DirectiveSubscriberOutcome::NotFound { artifact_id: id },
    };
    if let Err(e) = validate_review_resolution_envelope(
        parsed,
        "directive",
        &id.to_string(),
        current_version,
        DIRECTIVE_REVIEW_ACTIONS,
    ) {
        return DirectiveSubscriberOutcome::EnvelopeRejected {
            code: e.code(),
            message: e.message(),
        };
    }
    if matches!(decision.outcome(), ResolutionOutcome::KeepArtifact)
        || matches!(decision.outcome(), ResolutionOutcome::RequestChanges)
    {
        return DirectiveSubscriberOutcome::KeptArtifact { decision };
    }
    // PerformTransition path. compile-action ids carry no manager
    // transition (compile happened pre-review); approve / archive do.
    match parsed.action.as_str() {
        "compile" => DirectiveSubscriberOutcome::CompileNoOp { decision },
        "approve" => match state.store.directive_approve(id, current_version).await {
            Ok(_) => DirectiveSubscriberOutcome::Approved,
            Err(e) => DirectiveSubscriberOutcome::DbError {
                detail: format!("directive_approve: {}", e),
            },
        },
        "archive" => match state
            .store
            .directive_update_status(id, current_version, DirectiveStatus::Archived)
            .await
        {
            Ok(_) => DirectiveSubscriberOutcome::Archived,
            Err(e) => DirectiveSubscriberOutcome::DbError {
                detail: format!("directive_update_status(archived): {}", e),
            },
        },
        // validate_review_resolution_envelope above already rejected
        // anything outside DIRECTIVE_REVIEW_ACTIONS, so this branch is
        // unreachable. Defensive fallback: treat as compile no-op.
        _ => DirectiveSubscriberOutcome::CompileNoOp { decision },
    }
}

// ───────────────────────────────────────────────────────────────────────
// tests — pure functions only (no LLM, no DB)
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
