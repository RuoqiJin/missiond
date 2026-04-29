use anyhow::{anyhow, Result};
use chrono::Utc;
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::Path;

use crate::state::AppState;

use super::claim_lease::{find_claim_node, parse_claims, parse_iso, scopes_overlap};
use super::completion_gates::{
    audit_scoped_commit_handoff, check_id_monotonic, enforce_scoped_commit_completion,
    enforce_task_contract_completion,
};
use super::completion_records::{
    collect_string_list, normalize_commit_status, normalize_task_run_verifier_status,
    normalize_verifier_status, render_string_list, VALID_COMMIT_STATUSES,
    VALID_TASK_RUN_VERIFIER_STATUSES, VALID_VERIFIER_STATUSES,
};
use super::log_store::{
    allocate_id, append_to_block, companion_path, insert_id_counters_block, lisp_quote_string,
    list_block_summaries, now_iso, parse_kv_pairs, project_or_target_project, read_log_file,
    require_str, resolve_project_root, scan_max_id, sexp, touch_last_updated, update_kv_in_node,
    write_log_file, Counter, LogFile,
};
use super::log_surface::{emit_execution_event, read_dispatch_metadata_from_log};
use super::session_trace::{
    append_session_trace_event, resolve_session_trace_path, resolve_trace_task_id,
    sanitize_trace_backend, TraceEvent, TraceKind,
};
use super::task_verifier::auto_run_task_run_verifier;

pub(super) async fn action_complete(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let phase = match require_str(args, "phase") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let agent = match require_str(args, "agent_name") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let summary = match require_str(args, "summary") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let deliverables = args
        .get("deliverables")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let verification = args
        .get("verification")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // ── scoped-commit handoff fields (intent-memory.lisp :: helper
    // agent-execution-coordination :: shared-memory-slots :: completions —
    // :fields "... changed_files / staged_files / commit_hash / commit_status").
    // All five are optional so legacy callers that omit them still write a
    // backward-compatible completion entry; only the keys actually supplied
    // are emitted into the Lisp slot. `commit_status` is normalized against
    // the canonical enum from the protocol's :commit-status-values.
    let changed_files = collect_string_list(args, "changed_files");
    let staged_files = collect_string_list(args, "staged_files");
    let commit_hash = args
        .get("commit_hash")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let commit_status_raw = args
        .get("commit_status")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let commit_status = match commit_status_raw {
        Some(s) => match normalize_commit_status(s) {
            Some(canonical) => Some(canonical.to_string()),
            None => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "commit_status must be one of {:?}, got `{}`",
                            VALID_COMMIT_STATUSES, s
                        ),
                    )
                    .with_suggestion("see intent-memory.lisp :: completions :commit-status-values"),
                ));
            }
        },
        None => None,
    };
    let commit_blocker = args
        .get("commit_blocker")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // ── wave-19 / task 08 — task-contract completion metadata.
    //
    // All four fields are optional and recorded verbatim into the
    // companion log when supplied. `verifier_status` is normalized
    // against the canonical enum so audit / dashboard consumers can key
    // off the exact string; unknown labels reject with `INVALID_PARAM`
    // BEFORE any file mutation. `task_contract_path` doubles as the
    // trigger for the contract-level enforcement gate further below
    // when paired with `enforce_scoped_commit=true`.
    let task_contract_path = args
        .get("task_contract_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let task_report_path = args
        .get("task_report_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verifier_status_raw = args
        .get("verifier_status")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let verifier_status = match verifier_status_raw {
        Some(s) => match normalize_verifier_status(s) {
            Some(canonical) => Some(canonical.to_string()),
            None => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "verifier_status must be one of {:?}, got `{}`",
                            VALID_VERIFIER_STATUSES, s
                        ),
                    )
                    .with_suggestion(
                        "see wave19-08 :: verifier-status enum (passed|failed|skipped|unknown)",
                    ),
                ));
            }
        },
        None => None,
    };
    let verifier_notes = args
        .get("verifier_notes")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // ── wave-21 / task 03 — task-run verifier completion metadata.
    //
    // `task_run_verifier_status` / `shared_memory_path` /
    // `verifier_diagnostics` / `verified` mirror the wave19-08 fields
    // but capture the END-TO-END verifier outcome (task contract +
    // report + shared-memory completion + commit scope all proven in
    // one pass — see wave21-02 :: scripts/verify-task-run.mjs). All
    // four are optional and recorded verbatim into the companion log;
    // `task_run_verifier_status` rejects unknown labels at parse time
    // so audit / dashboard consumers can key off the canonical enum.
    let task_run_verifier_status_raw = args
        .get("task_run_verifier_status")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let task_run_verifier_status = match task_run_verifier_status_raw {
        Some(s) => match normalize_task_run_verifier_status(s) {
            Some(canonical) => Some(canonical.to_string()),
            None => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "task_run_verifier_status must be one of {:?}, got `{}`",
                            VALID_TASK_RUN_VERIFIER_STATUSES, s
                        ),
                    )
                    .with_suggestion(
                        "see wave21-03 :: task-run-verifier-status enum (passed|failed|skipped|unknown)",
                    ),
                ));
            }
        },
        None => None,
    };
    let shared_memory_path = args
        .get("shared_memory_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verifier_diagnostics = args
        .get("verifier_diagnostics")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // `verified` is a tri-state at parse time: absent → None (legacy
    // shape, no extra gate), false → Some(false) (caller explicitly
    // recorded a non-verified completion), true → Some(true) (gate
    // runs). We persist the explicit `false` so audit can tell "writer
    // intentionally skipped verification" from "writer omitted the
    // field because they're a legacy caller".
    let verified_flag = args.get("verified").and_then(|v| v.as_bool());

    // ── Optional fail-fast enforcement (wave16-06).
    //
    // `enforce_scoped_commit=true` flips the existing audit-only handoff
    // checks into hard rejects at completion-time. Default `false` keeps
    // legacy callers byte-identical: they still get the audit-only path
    // wired through `mission_execution(action=audit)` later. We resolve
    // the flag here so the validation step (run BEFORE id allocation)
    // sees the caller's intent without paying the read cost twice.
    let enforce_scoped_commit = args
        .get("enforce_scoped_commit")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;

    // Run the enforcement gate BEFORE `allocate_id` mutates the
    // id-counters block — a rejected completion must not bump the
    // counter or otherwise change the durable file.
    let scoped_commit_validation = if enforce_scoped_commit {
        match enforce_scoped_commit_completion(
            &file,
            staged_files.as_deref(),
            commit_hash.as_deref(),
            commit_status.as_deref(),
            commit_blocker.as_deref(),
        ) {
            Ok(v) => Some(v),
            Err(err) => return Ok(err),
        }
    } else {
        None
    };

    // wave-19 / task 08 — contract-level enforcement gate. Runs only
    // when the caller paired `enforce_scoped_commit=true` with a
    // `task_contract_path`; otherwise the contract metadata is recorded
    // verbatim with no additional checks (legacy / opt-out behaviour).
    // Daemon never shells out — we read the file off disk and use the
    // workstation_dispatch parser to project the narrow view we need.
    let task_contract_validation = if enforce_scoped_commit && task_contract_path.is_some() {
        let path_arg = task_contract_path.as_deref().unwrap();
        match enforce_task_contract_completion(
            &file,
            &root,
            path_arg,
            commit_hash.as_deref(),
            staged_files.as_deref(),
        ) {
            Ok(v) => Some(v),
            Err(err) => return Ok(err),
        }
    } else {
        None
    };

    // wave-22 / task 02 — auto task-run verifier dispatch.
    //
    // The wave21-03 caller-supplied `verified=true` escape hatch is now
    // a legacy-compat fallback. The new contract: when the writer hands
    // every path the daemon needs for an end-to-end proof
    // (`task_contract_path`, `task_report_path`, `shared_memory_path`,
    // `commit_hash`) the daemon runs the in-tree task-run verifier
    // ITSELF and computes the verified status from the on-disk inputs
    // — no Node spawn, no shell, no mutating git, no caller assertion
    // accepted at face value. The wave21-02 script-side verifier
    // remains the out-of-process truth; this in-process projection just
    // closes the action-complete window so dashboards stop relying on
    // a writer-asserted boolean.
    //
    // Three-state `verification_source` summarises what happened:
    //   * `daemon-auto-verifier` — all four paths present, daemon ran
    //     the in-tree verifier and produced the verdict in
    //     `verifier_status` / `verified_scope_summary`.
    //   * `legacy-caller-claim` — caller passed `verified=true` but at
    //     least one of the four paths is absent. We honour the legacy
    //     posture (no hard reject), record the claim into the companion
    //     log verbatim, and surface `verifier_status="unknown"` plus a
    //     diagnostic explaining which path was missing so reviewers can
    //     migrate the caller off the escape hatch.
    //   * `none` — no auto-verifier run AND no legacy claim; absent in
    //     the response so legacy completions stay byte-identical.
    //
    // Backward compat: the wave21-03 helper `enforce_verified_completion`
    // is preserved verbatim and still callable from tests, but
    // `action_complete` no longer routes through it — the v2 dispatch
    // either runs the auto-verifier or downgrades the legacy claim.
    let auto_verifier_inputs_present = task_contract_path.is_some()
        && task_report_path.is_some()
        && shared_memory_path.is_some()
        && commit_hash.is_some();

    let mut verification_source: Option<&'static str> = None;
    let mut auto_verifier_summary: Option<Value> = None;
    let mut auto_verifier_status: Option<&'static str> = None;
    let mut auto_verifier_diagnostics: Option<String> = None;

    if auto_verifier_inputs_present {
        // unwraps are safe — we just checked all four are Some.
        let tcp = task_contract_path.as_deref().unwrap();
        let trp = task_report_path.as_deref().unwrap();
        let smp = shared_memory_path.as_deref().unwrap();
        let hash = commit_hash.as_deref().unwrap();
        match auto_run_task_run_verifier(&root, tcp, trp, smp, hash) {
            Ok(summary) => {
                auto_verifier_status = Some("passed");
                auto_verifier_summary = Some(summary);
                verification_source = Some("daemon-auto-verifier");
            }
            Err(err) => return Ok(err),
        }
    } else if verified_flag == Some(true) {
        // Legacy caller-supplied claim. Record it but flag in the
        // diagnostic which path was missing so the writer agent can
        // upgrade the next dispatch.
        let mut missing: Vec<&'static str> = Vec::new();
        if task_contract_path.is_none() {
            missing.push("task_contract_path");
        }
        if task_report_path.is_none() {
            missing.push("task_report_path");
        }
        if shared_memory_path.is_none() {
            missing.push("shared_memory_path");
        }
        if commit_hash.is_none() {
            missing.push("commit_hash");
        }
        verification_source = Some("legacy-caller-claim");
        auto_verifier_status = Some("unknown");
        auto_verifier_diagnostics = Some(format!(
            "verified=true accepted as legacy_verified_claim because the daemon-side auto-verifier requires all four of [task_contract_path, task_report_path, shared_memory_path, commit_hash]; missing: {:?}. Migrate the dispatch envelope to supply every path so the daemon can compute the verdict itself (wave22-02).",
            missing,
        ));
    }
    // Tri-state placeholder kept in sync with the wave21-03 response
    // shape: when the auto-verifier ran the response surfaces the
    // structured summary; when only the legacy claim was made it stays
    // None and the diagnostic prose above carries the explanation.
    let verified_validation: Option<Value> = auto_verifier_summary.clone();

    let id = allocate_id(&mut file, Counter::Completion)?;
    let date = now_iso();

    // Build the completion entry incrementally so the durability handoff
    // fields are appended only when supplied. The legacy 6-field shape stays
    // byte-identical when no scoped-commit metadata is provided; new callers
    // simply tack additional `:key value` pairs onto the same form.
    let mut entry = format!(
        "    ({id}\n      :phase {phase}\n      :agent {agent}\n      :summary {summary}\n      :deliverables {deliverables}\n      :verification {verification}\n      :at {date}",
        id = id,
        phase = lisp_quote_string(phase),
        agent = lisp_quote_string(agent),
        summary = lisp_quote_string(summary),
        deliverables = lisp_quote_string(deliverables),
        verification = lisp_quote_string(verification),
        date = lisp_quote_string(&date),
    );
    if let Some(ref list) = changed_files {
        entry.push_str(&format!(
            "\n      :changed-files {}",
            render_string_list(list)
        ));
    }
    if let Some(ref list) = staged_files {
        entry.push_str(&format!(
            "\n      :staged-files {}",
            render_string_list(list)
        ));
    }
    if let Some(ref hash) = commit_hash {
        entry.push_str(&format!("\n      :commit-hash {}", lisp_quote_string(hash)));
    }
    if let Some(ref status_val) = commit_status {
        entry.push_str(&format!(
            "\n      :commit-status {}",
            lisp_quote_string(status_val)
        ));
    }
    if let Some(ref blocker) = commit_blocker {
        entry.push_str(&format!(
            "\n      :commit-blocker {}",
            lisp_quote_string(blocker)
        ));
    }
    // wave-19 / task 08 — task-contract metadata. Each field skips when
    // absent so legacy callers that never set them keep the byte-identical
    // 6-field shape (or 11-field shape with scoped-commit fields).
    if let Some(ref tcp) = task_contract_path {
        entry.push_str(&format!(
            "\n      :task-contract-path {}",
            lisp_quote_string(tcp)
        ));
    }
    if let Some(ref trp) = task_report_path {
        entry.push_str(&format!(
            "\n      :task-report-path {}",
            lisp_quote_string(trp)
        ));
    }
    if let Some(ref vs) = verifier_status {
        entry.push_str(&format!(
            "\n      :verifier-status {}",
            lisp_quote_string(vs)
        ));
    }
    if let Some(ref vn) = verifier_notes {
        entry.push_str(&format!(
            "\n      :verifier-notes {}",
            lisp_quote_string(vn)
        ));
    }
    // wave-21 / task 03 — task-run verifier metadata. Each field skips
    // when absent so legacy callers (and wave19-08 callers that never
    // touched the wave21 slots) keep their byte-identical companion log
    // shape. `verified` is written as a bare `true`/`false` atom so a
    // round-trip through `parse_completions` recovers the boolean
    // without quoted-string handling.
    if let Some(ref trvs) = task_run_verifier_status {
        entry.push_str(&format!(
            "\n      :task-run-verifier-status {}",
            lisp_quote_string(trvs)
        ));
    }
    if let Some(ref smp) = shared_memory_path {
        entry.push_str(&format!(
            "\n      :shared-memory-path {}",
            lisp_quote_string(smp)
        ));
    }
    if let Some(ref vd) = verifier_diagnostics {
        entry.push_str(&format!(
            "\n      :verifier-diagnostics {}",
            lisp_quote_string(vd)
        ));
    }
    if let Some(v) = verified_flag {
        entry.push_str(&format!("\n      :verified {}", v));
    }
    entry.push(')');

    append_to_block(&mut file, "completions", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    // Same dispatch-metadata projection rationale as `action_claim` —
    // surface the trio from the companion-log meta block so completion
    // consumers can route on workstation-dispatch context without reading
    // the on-disk file. Absent / legacy meta cleanly skip-serializes
    // (see ExecutionEvent::Completed doc comment).
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Completed {
            execution_id: execution_id.to_string(),
            completion_id: id.clone(),
            phase: phase.to_string(),
            agent: agent.to_string(),
            at: date.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    let mut response = json!({
        "status": "recorded",
        "completion_id": id,
        "phase": phase,
        "agent": agent,
        "at": date,
        // Always surfaced so callers can detect at a glance which mode
        // the completion went through. `false` here means audit-only
        // (legacy / opt-out) — `true` means the durability invariants
        // were validated at write-time and the validation summary is
        // included below.
        "scoped_commit_enforced": enforce_scoped_commit,
    });
    if let Some(list) = changed_files {
        response["changed_files"] = json!(list);
    }
    if let Some(list) = staged_files {
        response["staged_files"] = json!(list);
    }
    if let Some(hash) = commit_hash {
        response["commit_hash"] = json!(hash);
    }
    if let Some(status_val) = commit_status {
        response["commit_status"] = json!(status_val);
    }
    if let Some(blocker) = commit_blocker {
        response["commit_blocker"] = json!(blocker);
    }
    if let Some(v) = scoped_commit_validation {
        response["scoped_commit_validation"] = v;
    }
    // wave-19 / task 08 — surface contract metadata + the contract-level
    // validation summary (when the gate ran). Skip-serialize semantics
    // mirror the scoped-commit fields above so the response stays
    // byte-identical for legacy callers that omit every wave19 field.
    if let Some(tcp) = task_contract_path {
        response["task_contract_path"] = json!(tcp);
    }
    if let Some(trp) = task_report_path {
        response["task_report_path"] = json!(trp);
    }
    // The wave19-08 caller-supplied `verifier_status` slot is preserved
    // verbatim when the wave22-02 auto-verifier did NOT run; otherwise
    // the daemon-computed status (set further below) wins so the
    // response surface advertises a single authoritative verdict.
    if let Some(ref vs) = verifier_status {
        response["verifier_status"] = json!(vs);
    }
    if let Some(vn) = verifier_notes {
        response["verifier_notes"] = json!(vn);
    }
    if let Some(v) = task_contract_validation {
        response["task_contract_validation"] = v;
    }
    // wave-21 / task 03 — surface task-run verifier metadata + the
    // verified-gate validation summary. Same skip-serialize semantics
    // as the wave19-08 fields above so legacy callers stay byte-
    // identical when they omit every wave21 field.
    if let Some(trvs) = task_run_verifier_status {
        response["task_run_verifier_status"] = json!(trvs);
    }
    if let Some(smp) = shared_memory_path {
        response["shared_memory_path"] = json!(smp);
    }
    // The wave21-03 caller-supplied `verifier_diagnostics` slot is
    // preserved verbatim when the wave22-02 auto-verifier did NOT run;
    // otherwise the daemon-computed diagnostic (set further below)
    // wins so reviewers see one diagnostic per response.
    if let Some(ref vd) = verifier_diagnostics {
        response["verifier_diagnostics"] = json!(vd);
    }
    if let Some(v) = verified_flag {
        response["verified"] = json!(v);
    }

    // ── wave-22 / task 02 — auto task-run verifier surface ────────────
    //
    // `verification_source` flags how the verdict was reached:
    //   * `daemon-auto-verifier` — daemon ran the in-tree verifier; the
    //     daemon-computed `verifier_status="passed"` overrides any
    //     caller-supplied wave19-08 / wave21-03 status. The structured
    //     `verified_scope_summary` records every cross-checked rule.
    //   * `legacy-caller-claim` — caller passed `verified=true` but at
    //     least one path was missing; daemon-computed status is
    //     `"unknown"` and `verifier_diagnostics` carries the migration
    //     prose pointing at the missing path(s).
    //
    // Absent `verification_source` (legacy callers) keeps the response
    // shape byte-identical to the wave21-03 surface.
    if let Some(src) = verification_source {
        response["verification_source"] = json!(src);
    }
    if let Some(status) = auto_verifier_status {
        // Daemon-computed verdict wins over the caller-supplied
        // wave19-08 / wave21-03 statuses. Reviewers can still see the
        // caller-supplied values inside `task_run_verifier_status` /
        // the companion log.
        response["verifier_status"] = json!(status);
    }
    if let Some(diag) = auto_verifier_diagnostics {
        response["verifier_diagnostics"] = json!(diag);
    }
    if let Some(scope_summary) = verified_validation {
        // wave-22 contract: the summary is exposed as
        // `verified_scope_summary`. We keep the wave21-03 shape under
        // the legacy `verified_validation` key too so existing
        // dashboards keep parsing while consumers migrate.
        response["verified_scope_summary"] = scope_summary.clone();
        response["verified_validation"] = scope_summary;
    }

    // wave23-04 — opt-in session-trace append. Records `complete` or
    // `failure` depending on the verifier verdict resolved above. The
    // entry mirrors the durable companion-log completion: it carries the
    // commit hash, report path, and changed-file list so future
    // analyzers can correlate completions with their durable artifacts
    // without re-reading the .missiond/v2/<exec>.lisp companion.
    if let Some(trace_path) = resolve_session_trace_path(args, &root) {
        match resolve_trace_task_id(args, &root, execution_id) {
            Some(task_id) => {
                // Failure when caller-supplied OR daemon-computed verifier
                // status resolved to "failed". Otherwise treat the
                // completion as a success-shaped event.
                let final_verifier_status = response
                    .get("verifier_status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let kind = match final_verifier_status.as_deref() {
                    Some("failed") => TraceKind::Failure,
                    _ => TraceKind::Complete,
                };
                let backend = sanitize_trace_backend(agent);
                // Re-read the commit / report / file metadata from args
                // since the local bindings above were consumed by the
                // response builder.
                let commit_hash_for_trace = args
                    .get("commit_hash")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    // checker requires `[0-9a-f]{4,64}` — drop anything
                    // shorter / non-hex so we don't fail validation.
                    .filter(|s| {
                        s.len() >= 4 && s.len() <= 64 && s.chars().all(|c| c.is_ascii_hexdigit())
                    });
                let report_path_for_trace = args
                    .get("task_report_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    // checker rejects absolute report paths.
                    .filter(|s| !Path::new(s).is_absolute());
                let files_for_trace = collect_string_list(args, "changed_files")
                    .or_else(|| collect_string_list(args, "staged_files"))
                    .map(|v| {
                        v.into_iter()
                            // strip absolute paths — checker rejects them
                            .filter(|p| !Path::new(p).is_absolute())
                            .collect::<Vec<_>>()
                    })
                    .filter(|v: &Vec<String>| !v.is_empty());
                let ev = TraceEvent {
                    task: task_id,
                    backend,
                    kind,
                    summary: format!(
                        "mission_execution(action=complete) phase={} agent={} completion_id={}",
                        phase, agent, id
                    ),
                    agent: None,
                    files: files_for_trace,
                    commit_hash: commit_hash_for_trace,
                    report_path: report_path_for_trace,
                };
                if let Err(w) = append_session_trace_event(&trace_path, &ev) {
                    response["trace_warning"] = json!(w.to_string());
                }
            }
            None => {
                response["trace_warning"] = json!(format!(
                    "session_trace_path supplied but execution_id `{}` is not a valid trace task id and no task_contract_path was provided",
                    execution_id
                ));
            }
        }
    }

    Ok(ToolResult::json_pretty(&response))
}

// ───────────────────────────────────────────────────────────────────────
// action: audit — paren balance + ID monotonic + claim overlap + stale +
//                 completion coverage + open-issue owners
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_audit(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let raw = std::fs::read_to_string(&path)?;
    let mut findings: Vec<Value> = Vec::new();

    if let Err(e) = sexp::check_balance(&raw) {
        findings.push(json!({
            "severity": "error",
            "kind": "paren-imbalance",
            "detail": e.to_string(),
        }));
    }

    let file = match LogFile::parse(raw) {
        Ok(f) => f,
        Err(e) => {
            findings.push(json!({
                "severity": "error",
                "kind": "parse-failed",
                "detail": e.to_string(),
            }));
            return Ok(ToolResult::json_pretty(&json!({
                "execution_id": execution_id,
                "path": path.display().to_string(),
                "ok": false,
                "findings": findings,
            })));
        }
    };

    if file.find_block("id-counters").is_none() {
        findings.push(json!({
            "severity": "warn",
            "kind": "missing-id-counters",
            "detail": "id-counters block absent; mutating actions fall back to scan-max — run action=repair to materialize",
        }));
    }

    for counter in [
        Counter::Claim,
        Counter::Deviation,
        Counter::Decision,
        Counter::Issue,
        Counter::Completion,
    ] {
        check_id_monotonic(&file, counter, &mut findings);
    }

    let claims = parse_claims(&file);
    let now = Utc::now();
    for c in &claims {
        if c.status != "active" {
            continue;
        }
        if let Some(exp) = c.lease_expires_at.as_deref().and_then(parse_iso) {
            if exp < now {
                findings.push(json!({
                    "severity": "warn",
                    "kind": "stale-claim",
                    "claim_id": c.id,
                    "claimer": c.claimer,
                    "lease_expires_at": c.lease_expires_at,
                    "detail": "lease expired with no release/heartbeat",
                }));
            }
        }
    }

    // Active claim overlaps.
    for (i, a) in claims.iter().enumerate() {
        if a.status != "active" {
            continue;
        }
        for b in claims.iter().skip(i + 1) {
            if b.status != "active" {
                continue;
            }
            if scopes_overlap(&a.scope, &b.scope) {
                findings.push(json!({
                    "severity": "error",
                    "kind": "claim-overlap",
                    "left": a.id,
                    "right": b.id,
                    "scope_left": a.scope,
                    "scope_right": b.scope,
                }));
            }
        }
    }

    // Open-issue owners.
    let issues_block = file.find_block("issues");
    if let Some(block) = issues_block {
        for child in block.children().iter().skip(1) {
            let kvs = parse_kv_pairs(&file.src, child.children());
            let status = kvs
                .get("status")
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_else(|| "open".to_string());
            if status == "resolved" || status == "closed" {
                continue;
            }
            let owner = kvs
                .get("owner")
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_default();
            if owner.is_empty() {
                let head = child.head_atom().unwrap_or("?");
                findings.push(json!({
                    "severity": "warn",
                    "kind": "open-issue-no-owner",
                    "issue_id": head,
                }));
            }
        }
    }

    // Completion coverage: each phase referenced by a completion should have
    // a phase entry. We just check the inverse — phases marked completed in
    // phase-tracker should have at least one COMP entry referencing them.
    let phase_tracker = file
        .find_block("phase-tracker")
        .map(|m| parse_kv_pairs(&file.src, m.children()))
        .unwrap_or_default();
    if let Some(current) = phase_tracker.get("current-phase") {
        if current.trim().trim_matches('"') != "nil" && !current.trim().is_empty() {
            let comps = list_block_summaries(&file, "completions", |kvs, head| {
                Some(json!({
                    "id": head,
                    "phase": kvs.get("phase").cloned().unwrap_or_default(),
                }))
            });
            let _ = comps; // informational only — no failing assertion yet
        }
    }

    // ── scoped-commit handoff durability checks ──────────────────────
    // intent-memory.lisp :: helper agent-execution-coordination ::
    // scoped-commit-contract + invariants :inv-7 — every completion that
    // claims to have committed must carry a real commit_hash, every
    // blocked completion must explain itself, and staged_files must stay
    // inside the claim scope (the active or most-recently-released claim
    // owned by the same agent). These are read-only audit findings — the
    // daemon never executes git itself; the writer agent is responsible
    // for the actual commit. See task-file
    // wave12-01-mission-execution-scoped-commit-handoff.md.
    audit_scoped_commit_handoff(&file, &claims, &mut findings);

    let ok = findings.iter().all(|f| {
        f.get("severity")
            .and_then(|v| v.as_str())
            .map(|s| s != "error")
            .unwrap_or(true)
    });

    let error_count = findings
        .iter()
        .filter(|f| f.get("severity").and_then(|v| v.as_str()) == Some("error"))
        .count() as u32;
    // Wave 20 / Task 09 — surface the workstation-dispatch trio on the
    // audit + stale-claim events. Audit is read-only so we don't write
    // back to the file; the meta block we observe is whatever the latest
    // writer left there.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Audited {
            execution_id: execution_id.to_string(),
            ok,
            findings_count: findings.len() as u32,
            error_count,
            dispatch_strategy: meta.dispatch_strategy.clone(),
            target_project: meta.target_project.clone(),
            requested_cwd: meta.requested_cwd.clone(),
        },
    )
    .await;
    for f in &findings {
        if f.get("kind").and_then(|v| v.as_str()) != Some("stale-claim") {
            continue;
        }
        let claim_id = f
            .get("claim_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let claimer = f
            .get("claimer")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let lease_expires_at = f
            .get("lease_expires_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        emit_execution_event(
            state,
            ExecutionEvent::StaleClaim {
                execution_id: execution_id.to_string(),
                claim_id,
                claimer,
                lease_expires_at,
                dispatch_strategy: meta.dispatch_strategy.clone(),
                target_project: meta.target_project.clone(),
                requested_cwd: meta.requested_cwd.clone(),
            },
        )
        .await;
    }

    Ok(ToolResult::json_pretty(&json!({
        "execution_id": execution_id,
        "path": path.display().to_string(),
        "ok": ok,
        "findings": findings,
    })))
}

// ───────────────────────────────────────────────────────────────────────
// action: repair — dry-run by default; structural fixes only
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_repair(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("dry_run");
    if mode != "dry_run" && mode != "apply" {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            format!("repair mode must be `dry_run` or `apply`, got `{}`", mode),
        )));
    }

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let raw = std::fs::read_to_string(&path)?;
    let mut file = LogFile::parse(raw)?;

    let mut actions: Vec<Value> = Vec::new();

    // 1) Synthesize missing id-counters with values derived from scan_max_id.
    if file.find_block("id-counters").is_none() {
        let claim_n = scan_max_id(&file, Counter::Claim) + 1;
        let dev_n = scan_max_id(&file, Counter::Deviation) + 1;
        let dec_n = scan_max_id(&file, Counter::Decision) + 1;
        let issue_n = scan_max_id(&file, Counter::Issue) + 1;
        let comp_n = scan_max_id(&file, Counter::Completion) + 1;
        actions.push(json!({
            "kind": "synthesize-id-counters",
            "next_claim_id": claim_n,
            "next_deviation_id": dev_n,
            "next_decision_id": dec_n,
            "next_issue_id": issue_n,
            "next_completion_id": comp_n,
        }));
        if mode == "apply" {
            insert_id_counters_block(&mut file, claim_n, dev_n, dec_n, issue_n, comp_n)?;
        }
    }

    // 2) Mark stale claims (lease expired, no release).
    let claims = parse_claims(&file);
    let now = Utc::now();
    let mut stale_ids = Vec::new();
    for c in &claims {
        if c.status != "active" {
            continue;
        }
        if let Some(exp) = c.lease_expires_at.as_deref().and_then(parse_iso) {
            if exp < now {
                stale_ids.push(c.id.clone());
            }
        }
    }
    for id in &stale_ids {
        actions.push(json!({
            "kind": "mark-stale-claim",
            "claim_id": id,
        }));
        if mode == "apply" {
            if let Some(node) = find_claim_node(&file, id).cloned() {
                update_kv_in_node(&mut file, &node, "status", &lisp_quote_string("stale"))?;
            }
        }
    }

    // 3) Rebuild derived-indexes if it exists; otherwise leave alone (the
    //    block is cache, not truth — status action recomputes anyway).
    if file.find_block("derived-indexes").is_some() {
        actions.push(json!({
            "kind": "rebuild-derived-indexes",
            "note": "regenerating from durable slots"
        }));
        if mode == "apply" {
            rebuild_derived_indexes(&mut file)?;
        }
    }

    if mode == "apply" && !actions.is_empty() {
        touch_last_updated(&mut file)?;
        write_log_file(&path, &file)?;
    }

    // Wave 20 / Task 09 — surface the workstation-dispatch trio on
    // repair events. The same `file` handle is current after any
    // apply-mode mutations above, so the meta block we observe is the
    // post-write authoritative state.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Repaired {
            execution_id: execution_id.to_string(),
            applied: mode == "apply",
            action_count: actions.len() as u32,
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "execution_id": execution_id,
        "path": path.display().to_string(),
        "mode": mode,
        "actions": actions,
        "applied": mode == "apply",
    })))
}

fn rebuild_derived_indexes(file: &mut LogFile) -> Result<()> {
    let claims = parse_claims(file);
    let now = Utc::now();
    let active_ids: Vec<String> = claims
        .iter()
        .filter(|c| {
            c.status == "active"
                && c.lease_expires_at
                    .as_deref()
                    .and_then(parse_iso)
                    .map(|exp| exp >= now)
                    .unwrap_or(true)
        })
        .map(|c| c.id.clone())
        .collect();

    let open_issue_ids = list_block_summaries(file, "issues", |kvs, head| {
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| "open".to_string());
        if status == "resolved" || status == "closed" {
            None
        } else {
            Some(Value::String(head.to_string()))
        }
    });

    let unresolved_dev_ids = list_block_summaries(file, "deviations", |kvs, head| {
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| "open".to_string());
        if status == "resolved" || status == "closed" {
            None
        } else {
            Some(Value::String(head.to_string()))
        }
    });

    let latest_decisions = list_block_summaries(file, "decisions", |_kvs, head| {
        Some(Value::String(head.to_string()))
    });
    let completed_phases = list_block_summaries(file, "completions", |kvs, _head| {
        Some(Value::String(
            kvs.get("phase")
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_default(),
        ))
    });

    let render_list = |items: &[Value]| -> String {
        let parts: Vec<String> = items
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(lisp_quote_string)
            .collect();
        if parts.is_empty() {
            "()".to_string()
        } else {
            format!("({})", parts.join(" "))
        }
    };

    let block = match file.find_block("derived-indexes").cloned() {
        Some(b) => b,
        None => return Ok(()),
    };
    let active_lit = render_list(
        &active_ids
            .iter()
            .map(|s| Value::String(s.clone()))
            .collect::<Vec<_>>(),
    );
    let issues_lit = render_list(&open_issue_ids);
    let dev_lit = render_list(&unresolved_dev_ids);
    let dec_lit = render_list(&latest_decisions);
    let phases_lit = render_list(&completed_phases);

    update_kv_in_node(file, &block, "active-claims", &active_lit)?;
    let block2 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block2, "open-issues", &issues_lit)?;
    let block3 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block3, "unresolved-deviations", &dev_lit)?;
    let block4 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block4, "latest-decisions", &dec_lit)?;
    let block5 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block5, "completed-phases", &phases_lit)?;
    Ok(())
}
