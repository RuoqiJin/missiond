use anyhow::Result;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use crate::state::AppState;

use super::completion_fields::collect_string_list;
use super::log_store::{
    companion_path, project_or_target_project, read_log_file, require_str, resolve_project_root,
};
use super::preflight_contract::apply_task_contract_projection;
use super::preflight_cwd::resolve_preflight_inspect_dir;
use super::preflight_porcelain::{parse_porcelain_status, run_git_status};
use super::preflight_scope::{
    build_preflight_summary, collect_all_claim_scopes, collect_specific_claim_scope,
};
use super::preflight_trace::append_preflight_trace_if_requested;

// ───────────────────────────────────────────────────────────────────────
// action: preflight_commit — read-only worktree audit before scoped commit
//
// Wave 18 / Task 08. The daemon may inspect git status / diff but MUST
// NEVER stage/commit/reset/checkout. The writer agent is the only actor
// that mutates the worktree; we just project worktree state vs the
// active+released claim scopes so the writer can see scope drift before
// running its scoped commit.
//
// Pairs with `enforce_scoped_commit_completion` (wave16-06) which is the
// post-commit gate; preflight catches the same violations one step
// earlier so the writer doesn't have to roll back a bad stage.
//
// Wave 20 / Task 03 augmentation: when the caller threads
// `task_contract_path` through the preflight call, daemon also loads the
// task-contract v1 (read-only) and projects the staged set against the
// contract's `:write-scope` / `:must-not-touch` patterns. Two new
// top-level fields (`staged_out_of_scope`, `staged_forbidden`) plus
// `unstaged_in_scope` and a `task_contract_status` label surface so the
// writer learns about contract-level drift one hop earlier than the
// post-commit `task-scope-guard.mjs`. Daemon still runs no mutating git
// command — `evaluate_task_contract_for_preflight` is pure file IO + a
// glob projection.
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_preflight_commit(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };

    // Resolve project root through the registry — same gate every other
    // action uses. Refusing unresolved roots is part of the wave18-08
    // safety contract: we never run git outside an explicitly registered
    // project (or the active CWD when no project is supplied).
    let root = match resolve_project_root(state, project_or_target_project(args)).await {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("cannot resolve project root: {}", e),
                )
                .with_suggestion(
                    "register the project via `mission_project(action=add, …)` or call from inside the project worktree",
                ),
            ));
        }
    };

    let inspect_dir = match resolve_preflight_inspect_dir(&root, args) {
        Ok(dir) => dir,
        Err(err) => return Ok(err),
    };

    // Expected_files hint from the workstation brief. Trimmed and
    // empty-filtered through the same helper as `staged_files` so the
    // writer doesn't need to pre-clean its list.
    let expected_files = collect_string_list(args, "expected_files");

    // Companion log read — same path resolution as every other action.
    // We need the claims block for scope comparison; opening the file
    // also doubles as a "did the writer pass a real execution_id?"
    // gate, mirroring the rejection shape of action_status.
    let path = companion_path(&root, execution_id);
    let file = match read_log_file(&path) {
        Ok(f) => f,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("companion log {} not readable: {}", path.display(), e),
                )
                .with_suggestion("confirm execution_id matches a previously opened companion log"),
            ));
        }
    };

    // Resolve which claim scope(s) we audit against. Default = union of
    // all claim scopes; explicit `claim_id` narrows to a single scope so
    // the writer can preflight against the exact claim it just acquired.
    let claim_scopes = if let Some(cid) = args
        .get("claim_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        match collect_specific_claim_scope(&file, cid) {
            Ok(scopes) => scopes,
            Err(err) => return Ok(err),
        }
    } else {
        collect_all_claim_scopes(&file)
    };

    // Read-only git status under the inspect_dir. The only mutating
    // codepath in this whole crate is `arch_maintenance_worker`, which
    // lives behind a feature flag the writer agent never reaches; this
    // action stays strictly to `git status --porcelain=v1`.
    let raw_status = match run_git_status(&inspect_dir) {
        Ok(s) => s,
        Err(err) => return Ok(err),
    };
    let entries = parse_porcelain_status(&raw_status);

    let mut summary = build_preflight_summary(&entries, &claim_scopes, expected_files.as_deref());

    // Echo the inputs so the writer agent can correlate the response
    // with the exact dispatch envelope it sent us. `cwd` is the
    // canonicalized form so any symlink / `..` resolution is visible.
    summary["execution_id"] = json!(execution_id);
    summary["cwd"] = json!(inspect_dir.to_string_lossy());
    summary["project_root"] = json!(root.to_string_lossy());
    if let Some(cid) = args.get("claim_id").and_then(|v| v.as_str()) {
        summary["claim_id"] = json!(cid);
    }
    // wave-20 / task 03 — when the caller threads `task_contract_path`
    // through preflight, daemon now loads it (read-only) and projects
    // staged/changed files against the contract's `:write-scope` +
    // `:must-not-touch` so the writer sees scope drift BEFORE running
    // `git commit`. Daemon never mutates the worktree here — load failures
    // surface as `task_contract_status="missing"` / `"malformed"` so the
    // writer can fix the path / file content without preflight hard-
    // rejecting (the post-commit gate at `action=complete` is the
    // authoritative enforcement).
    apply_task_contract_projection(&mut summary, &root, args);

    // wave-21 / task 03 — echo the task-run verifier hint paths when
    // the caller threads them through preflight. These are advisory
    // only (the daemon does not load the report at preflight time;
    // the wave21-03 verified-gate at `action=complete` is the
    // authoritative cross-check). Surfacing them here lets the writer
    // confirm the dispatch envelope matches what the script-side
    // verifier (`scripts/verify-task-run.mjs`) will load post-commit.
    if let Some(trp) = args
        .get("task_report_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        summary["task_report_path"] = json!(trp);
    }
    if let Some(smp) = args
        .get("shared_memory_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        summary["shared_memory_path"] = json!(smp);
    }

    // wave23-04 — opt-in session-trace append. Preflight is informational
    // (no commit happens here) so we record it as `observation` carrying
    // the staged + ok flag in the summary text. Best-effort: failures
    // surface as `trace_warning` without flipping the preflight verdict.
    append_preflight_trace_if_requested(args, &root, execution_id, &mut summary);

    Ok(ToolResult::json_pretty(&summary))
}
