use crate::ToolDefinition;
use serde_json::{json, Value};

/// Build a single property descriptor `{"type": ..., "description": ...}` —
/// optionally with an `enum` constraint. Centralising construction here keeps
/// the schema-builder readable while sidestepping `json!` macro recursion
/// limits when the property bag grows past ~30 entries.
fn prop(ty: &str, description: &str) -> Value {
    json!({"type": ty, "description": description})
}

fn prop_enum(ty: &str, description: &str, variants: &[&str]) -> Value {
    json!({
        "type": ty,
        "enum": variants,
        "description": description,
    })
}

fn prop_array_of_strings(description: &str) -> Value {
    json!({
        "type": "array",
        "items": {"type": "string"},
        "description": description,
    })
}

pub fn definitions() -> Vec<ToolDefinition> {
    let mut properties = serde_json::Map::new();

    properties.insert(
        "action".into(),
        prop_enum(
            "string",
            "manager action — see Lisp helper agent-execution-coordination :: mcp-tool-design",
            &[
                "open",
                "list",
                "claim",
                "heartbeat",
                "release",
                "deviate",
                "decide",
                "issue",
                "complete",
                "status",
                "audit",
                "repair",
                "preflight_commit",
            ],
        ),
    );
    properties.insert(
        "project".into(),
        prop(
            "string",
            "[all] project id (registry-resolved root); defaults to CWD",
        ),
    );
    properties.insert(
        "target_project".into(),
        prop(
            "string",
            "[all] alias for `project`; if both supplied `project` wins. Persisted in companion log meta when present (intent-tools.lisp :: implemented-surface mission_execution :: :workstation-dispatch-record).",
        ),
    );
    properties.insert(
        "dispatch_strategy".into(),
        prop_enum(
            "string",
            "[open] workstation-dispatch-record strategy from intent-tools.lisp. Unknown / empty values normalise to `unknown`. Persisted in companion log meta and surfaced by status/list.",
            &[
                "resident-lisp",
                "fresh-code-alignment",
                "agent-team",
                "mixed",
                "prompt-fallback",
                "unknown",
            ],
        ),
    );
    properties.insert(
        "requested_cwd".into(),
        prop(
            "string",
            "[open] working directory the dispatcher used; metadata only, persisted into companion log meta when present.",
        ),
    );
    properties.insert(
        "execution_id".into(),
        prop(
            "string",
            "[all except list] companion log basename, e.g. `intent-memory-execution`",
        ),
    );
    properties.insert(
        "parent_design".into(),
        prop(
            "string",
            "[open|list filter] frozen design lisp this companion pairs with",
        ),
    );
    properties.insert(
        "scope".into(),
        prop(
            "string",
            "[open|claim] scope description (file/path/section); claim conflicts on overlap",
        ),
    );
    properties.insert(
        "owner".into(),
        prop(
            "string",
            "[open|issue] human/agent that owns the execution or issue",
        ),
    );
    properties.insert(
        "status".into(),
        prop("string", "[list filter] match meta :status substring"),
    );
    properties.insert(
        "scope_prefix".into(),
        prop(
            "string",
            "[list filter] only entries whose scope starts with this",
        ),
    );
    properties.insert(
        "limit".into(),
        prop("integer", "[list] cap result count (1-500, default 50)"),
    );
    properties.insert(
        "claim_id".into(),
        prop(
            "string",
            "[heartbeat|release] claim id returned by claim action",
        ),
    );
    properties.insert(
        "claimer_name".into(),
        prop(
            "string",
            "[claim|heartbeat|release] caller identity; release/heartbeat must match claim owner",
        ),
    );
    properties.insert(
        "phase".into(),
        prop(
            "string",
            "[claim|deviate|complete] phase name from phase-tracker",
        ),
    );
    properties.insert(
        "lease_secs".into(),
        prop(
            "integer",
            "[claim|heartbeat] lease window in seconds (60..86400, default 1800)",
        ),
    );
    properties.insert(
        "summary".into(),
        prop("string", "[release|complete] short prose of what was done"),
    );
    properties.insert(
        "lisp_said".into(),
        prop("string", "[deviate] verbatim quote from frozen design lisp"),
    );
    properties.insert(
        "actually_found".into(),
        prop("string", "[deviate] what actually happened in code/runtime"),
    );
    properties.insert(
        "reason".into(),
        prop("string", "[deviate] why the deviation was necessary"),
    );
    properties.insert(
        "approved_by".into(),
        prop(
            "string",
            "[deviate] auto / agent-consensus / user / commander",
        ),
    );
    properties.insert(
        "context".into(),
        prop(
            "string",
            "[decide] situation requiring a small in-flight decision",
        ),
    );
    properties.insert(
        "options".into(),
        prop(
            "string",
            "[decide] alternatives considered (free-form text)",
        ),
    );
    properties.insert("chosen".into(), prop("string", "[decide] selected option"));
    properties.insert(
        "rationale".into(),
        prop("string", "[decide] why the chosen option won"),
    );
    properties.insert(
        "decided_by".into(),
        prop("string", "[decide] author of the decision"),
    );
    properties.insert(
        "severity".into(),
        prop(
            "string",
            "[issue] low|medium|high|critical (default medium)",
        ),
    );
    properties.insert(
        "desc".into(),
        prop("string", "[issue] one-line description of the blocker/risk"),
    );
    properties.insert(
        "resolution_path".into(),
        prop("string", "[issue] how to resolve (free-form)"),
    );
    properties.insert(
        "agent_name".into(),
        prop("string", "[complete] agent that finished the phase"),
    );
    properties.insert(
        "deliverables".into(),
        prop("string", "[complete] artifacts produced (free-form text)"),
    );
    properties.insert(
        "verification".into(),
        prop(
            "string",
            "[complete] how completion was verified (tests, audit, etc.)",
        ),
    );
    properties.insert(
        "changed_files".into(),
        prop_array_of_strings(
            "[complete] files actually modified in the worktree by this completion (durability-plane evidence per intent-memory.lisp :: agent-execution-coordination :: completions). Persisted verbatim into the companion log; status/audit surface them for handoff visibility.",
        ),
    );
    properties.insert(
        "staged_files".into(),
        prop_array_of_strings(
            "[complete] files staged for the scoped commit (must be a subset of the active claim scope per scoped-commit-contract :: scope-rule). Audit emits `scoped-commit-violation` when any entry escapes the claim scope.",
        ),
    );
    properties.insert(
        "commit_hash".into(),
        prop(
            "string",
            "[complete] git commit hash that durably persists the completion. Required when commit_status=committed (audit emits `commit-status-without-hash` otherwise). Daemon never executes git itself — the writer agent runs the scoped commit and reports back.",
        ),
    );
    properties.insert(
        "commit_status".into(),
        prop_enum(
            "string",
            "[complete] scoped-commit handoff state per intent-memory.lisp :: completions :commit-status-values. `not-required` for read-only work; `pending` while staging; `committed` requires commit_hash; `blocked` requires commit_blocker; `skipped` for explicit policy waivers.",
            &["not-required", "pending", "committed", "blocked", "skipped"],
        ),
    );
    properties.insert(
        "commit_blocker".into(),
        prop(
            "string",
            "[complete] human-readable reason the scoped commit could not be made. Required when commit_status=blocked (audit emits `commit-status-blocked-without-blocker` otherwise) so the next agent can resume per scoped-commit-contract :: recovery-rule.",
        ),
    );
    properties.insert(
        "enforce_scoped_commit".into(),
        prop(
            "boolean",
            "[complete] opt-in fail-fast enforcement of scoped-commit-contract at completion time (wave16-06). Defaults to false: legacy callers keep audit-only behavior — wave17-07 deliberately preserves this default so existing pipelines outside the workstation-dispatch substrate stay byte-identical. When true, action=complete rejects with structured errors before mutating the companion log: `COMMIT_HASH_REQUIRED` (commit_status=committed without commit_hash), `COMMIT_BLOCKER_REQUIRED` (commit_status=blocked without commit_blocker), `CLAIM_SCOPE_REQUIRED` (staged_files non-empty but no claims on the file), `SCOPED_COMMIT_VIOLATION` (any staged path escapes every recorded claim scope — same scopes_overlap rule as the audit-only path). Daemon never runs git itself; the writer agent runs the scoped commit and reports back. Response surfaces `scoped_commit_enforced` plus `scoped_commit_validation` summary on success. Wave 17 / Task 07: every workstation-dispatch task brief now instructs the worker to set this flag to true on completion (`mission_plan(action=execute, workstation_dispatch=true)` responses pin `scoped_commit_required=true` + `scoped_commit_policy=\"enforced-on-complete\"` so observers can assert the brief contract).",
        ),
    );
    properties.insert(
        "mode".into(),
        prop_enum(
            "string",
            "[repair] dry_run reports planned actions; apply mutates the file",
            &["dry_run", "apply"],
        ),
    );
    properties.insert(
        "cwd".into(),
        prop(
            "string",
            "[preflight_commit] working directory the writer agent is committing from. Optional override for the project root; must canonicalize to a path inside the registered project root or preflight rejects with INVALID_PARAM. Defaults to the project root when absent.",
        ),
    );
    properties.insert(
        "expected_files".into(),
        prop_array_of_strings(
            "[preflight_commit] optional hint listing the files the dispatch brief expected this writer to touch (e.g. plan node `paths`). Drift in either direction surfaces in the response: paths NOT touched land in `expected_missing`, paths touched but NOT expected land in `expected_unexpected`. Advisory only — does not flip `ok`; the scope check against active+released claims is the authoritative gate.",
        ),
    );
    // ── wave-19 / task 08 — task-contract completion metadata ──
    //
    // `task_contract_path` / `task_report_path` / `verifier_status` /
    // `verifier_notes` are optional metadata fields the writer agent can
    // attach to a completion when the dispatch flowed through a
    // task-contract v1 + report-contract v1 pair (wave19-02 / wave19-03).
    // Daemon records them verbatim into the companion log; when
    // `enforce_scoped_commit=true` AND `task_contract_path` is supplied,
    // daemon ALSO loads the contract (read-only) and validates that
    // `commit_hash` is present + every `:write-scope` entry is covered by
    // an active/released claim or an entry in `staged_files`. Daemon never
    // shells out to a mutating git command — verifier-status is reported
    // by the caller (typically by running scripts/verify-task-contract.mjs
    // out-of-process).
    properties.insert(
        "task_contract_path".into(),
        prop(
            "string",
            "[complete|preflight_commit] relative-or-absolute path to the task-contract v1 Lisp file the dispatch brief pointed at (wave19-06). Recorded verbatim into the completion entry as `:task-contract-path`. When `enforce_scoped_commit=true` is also set on `action=complete`, daemon loads the file (read-only) and asserts every `:write-scope` entry overlaps an active/released claim or a `staged_files` path; missing critical data rejects with structured `TASK_CONTRACT_*` errors. wave20-03: `action=preflight_commit` also accepts this field — daemon loads the contract (read-only) and projects the staged/changed set against `:write-scope` + `:must-not-touch`, surfacing `task_contract_status` (`loaded`/`missing`/`malformed`), `staged_out_of_scope`, `staged_forbidden`, `unstaged_in_scope`, and a contract-aware `next_step`. Preflight stays informational on contract load failure (no hard reject) — the post-commit gate (`enforce_scoped_commit=true` + scripts/task-scope-guard.mjs) is the authoritative enforcement. Absent → legacy completion / preflight behavior (no contract-level checks, audit-only handoff).",
        ),
    );
    properties.insert(
        "task_report_path".into(),
        prop(
            "string",
            "[complete] relative-or-absolute path to the report-contract v1 Lisp file the writer produced (wave19-03). Recorded verbatim into the completion entry as `:task-report-path`. Metadata only — daemon does NOT parse it; verifier_status is the authoritative outcome signal. Absent → completion entry omits `:task-report-path` (legacy shape).",
        ),
    );
    properties.insert(
        "verifier_status".into(),
        prop_enum(
            "string",
            "[complete] outcome from the writer-side verifier run (typically `node scripts/verify-task-contract.mjs --commit <hash>`). Recorded verbatim into the completion entry as `:verifier-status`. `passed` = verifier exited 0; `failed` = verifier reported errors; `skipped` = verifier intentionally not run (read-only completion); `unknown` = verifier outcome could not be determined. Daemon never runs the verifier itself.",
            &["passed", "failed", "skipped", "unknown"],
        ),
    );
    properties.insert(
        "verifier_notes".into(),
        prop(
            "string",
            "[complete] free-form prose describing the verifier outcome (e.g. error summary, warnings, command line used). Recorded verbatim into the completion entry as `:verifier-notes` when supplied.",
        ),
    );
    // ── wave-21 / task 03 — task-run verifier integration metadata ──
    //
    // These four fields are the wave21-03 counterparts to the
    // wave19-08 `verifier_status` / `verifier_notes` slots. They
    // capture the END-TO-END verifier outcome (task contract + report
    // + shared-memory completion + commit scope all proven in one
    // pass — see wave21-02 `scripts/verify-task-run.mjs`). All four
    // are optional and recorded verbatim into the companion log when
    // supplied; `verified=true` ALSO triggers a daemon-side read-only
    // cross-check that loads the report-contract off disk and asserts
    // `:schema = missiond.report-contract.v1`, `:task_id` matches the
    // task contract head id, and `:commit_hash` matches the supplied
    // `commit_hash`. Fail-fast preconditions: `verified=true` requires
    // `enforce_scoped_commit=true`, `task_contract_path`,
    // `task_report_path`, and `commit_hash` — daemon rejects with
    // `VERIFIED_REQUIRES_*` codes BEFORE any companion log mutation.
    // Daemon NEVER spawns Node here; the script-side verifier is the
    // out-of-process authority and these fields are the durable
    // record that the writer asserted it passed.
    properties.insert(
        "task_run_verifier_status".into(),
        prop_enum(
            "string",
            "[complete] outcome from the writer-side end-to-end task-run verifier (typically `node scripts/verify-task-run.mjs` from wave21-02). Recorded verbatim into the completion entry as `:task-run-verifier-status`. Distinct slot from `verifier_status` (the wave19-08 task-contract verifier outcome) so callers can record both signals on the same completion. `passed` = verifier exited 0; `failed` = verifier reported errors; `skipped` = verifier intentionally not run; `unknown` = outcome could not be determined. Daemon never runs the verifier itself.",
            &["passed", "failed", "skipped", "unknown"],
        ),
    );
    properties.insert(
        "shared_memory_path".into(),
        prop(
            "string",
            "[complete|preflight_commit] relative-or-absolute path to the wave's shared-memory ledger (`.missiond/tasks/<wave>/shared-memory.lisp`). Metadata only — daemon does NOT parse the ledger here; the wave21-02 script-side verifier consumes it. On `action=complete` recorded verbatim as `:shared-memory-path`; on `action=preflight_commit` echoed back as an advisory hint so the writer can confirm the dispatch envelope matches what the script-side verifier will load post-commit.",
        ),
    );
    properties.insert(
        "verifier_diagnostics".into(),
        prop(
            "string",
            "[complete] free-form prose describing the task-run verifier outcome (e.g. error summary, warnings, command line used, JSON diagnostic blob). Recorded verbatim into the completion entry as `:verifier-diagnostics` when supplied.",
        ),
    );
    properties.insert(
        "verified".into(),
        prop(
            "boolean",
            "[complete] writer-asserted end-to-end verification flag. WAVE 22 / TASK 02 SHIFT: this flag is now a legacy-compat fallback only. The new contract: when the writer supplies all four of `task_contract_path`, `task_report_path`, `shared_memory_path`, and `commit_hash`, daemon runs the in-tree task-run auto-verifier ITSELF (read-only file inspection — no Node spawn, no shell, no mutating git) and computes the verdict on the response as `verifier_status` / `verified_scope_summary` plus `verification_source=\"daemon-auto-verifier\"`. Failure cases reject with structured `TASK_CONTRACT_REQUIRED` / `TASK_CONTRACT_MALFORMED` / `TASK_REPORT_REQUIRED` / `TASK_REPORT_MALFORMED` / `TASK_REPORT_TASK_ID_MISMATCH` / `TASK_REPORT_COMMIT_HASH_MISMATCH` / `SHARED_MEMORY_REQUIRED` / `SHARED_MEMORY_MALFORMED` / `SHARED_MEMORY_NO_COMPLETION_FOR_TASK` codes BEFORE any companion log mutation. When the writer instead sets `verified=true` WITHOUT supplying all four paths, daemon downgrades the assertion to a legacy claim (`verification_source=\"legacy-caller-claim\"`, `verifier_status=\"unknown\"`, `verifier_diagnostics` lists the missing paths) without rejecting — backward-compat for wave21-03 callers. `false` is recorded verbatim (writer explicitly opted out); absent → legacy completion shape with no extra surface. Daemon never spawns Node; the wave21-02 `scripts/verify-task-run.mjs` remains the out-of-process truth.",
        ),
    );
    // ── wave-23 / task 04 — opt-in session-trace integration ──
    //
    // `session_trace_path` lets the writer ask MissionD to append a
    // structured (trace-event ...) form to the named file when this
    // action runs. The daemon writes via Rust I/O — no Node spawn, no
    // shell — and emits `dispatch` for `action=open`, `observation` for
    // `action=preflight_commit`, and `complete` (or `failure` when the
    // verifier verdict resolved to "failed") for `action=complete`.
    // Best-effort: append errors surface on the response as
    // `trace_warning` but never abort the primary action result.
    // Output passes `scripts/check-session-trace.mjs` validation.
    properties.insert(
        "session_trace_path".into(),
        prop(
            "string",
            "[open|preflight_commit|complete] OPT-IN — relative-or-absolute path to the wave session-trace ledger (.missiond/tasks/<wave>/session-trace.lisp). When supplied, daemon appends a structured (trace-event ...) form recording this action's facts (kind=dispatch on open, observation on preflight_commit, complete or failure on complete). Daemon writes via Rust I/O only — no Node spawn, no shell, no mutating git. Best-effort: file missing / malformed / I/O errors surface on the response as `trace_warning` without aborting the primary action. The trace task id is derived from `task_contract_path` (preferred) or `execution_id` (fallback, must match `^[a-z0-9][a-z0-9._-]*$`). Output passes `scripts/check-session-trace.mjs` validation. Absent → legacy behavior (no trace I/O, no `trace_warning` on response).",
        ),
    );

    let schema = json!({
        "type": "object",
        "required": ["action"],
        "properties": Value::Object(properties),
    });

    vec![ToolDefinition::new(
        "mission_execution",
        "agent-execution-coordination v0.5.x manager — 13 actions over \
         .missiond/v3/runtime/executions/<id>.lisp companion logs, with legacy \
         .missiond/v2/<id>.lisp fallback (open / list / claim / heartbeat / release / deviate / decide / issue / \
         complete / status / audit / repair / preflight_commit). ID 分配由 manager 原子化 (id-counters slot), \
         claim 带 lease + heartbeat,deviation/decision/issue/completion 自动编号 D/DC/I/COMP\
         ;status 给 dashboard,audit 检 paren / 单调 ID / 重叠 claim / stale claim / scoped commit \
         handoff (commit-status-without-hash, commit-status-blocked-without-blocker, scoped-commit-violation),repair 仅修\
         结构 (dry_run|apply)。preflight_commit (wave18-08): read-only `git status --porcelain=v1` \
         under the resolved project root, compares changed/staged paths vs active+released claim scopes, \
         returns `{ok, changed_files, staged_files, out_of_scope_files, expected_missing?, \
         expected_unexpected?, claim_scopes, next_step}`. Daemon NEVER runs `git add/commit/reset/checkout` — \
         only inspects. Pairs with `enforce_scoped_commit=true` on action=complete (wave16-06) which is the \
         post-commit gate; preflight catches the same SCOPED_COMMIT_VIOLATION one step earlier. \
         Wave20-03: preflight_commit also accepts `task_contract_path`; when supplied daemon loads \
         the contract (read-only) and folds `task_contract_status` (loaded|missing|malformed), \
         `staged_out_of_scope`, `staged_forbidden`, `unstaged_in_scope`, plus a contract-aware \
         `next_step` into the response so contract-level drift surfaces before the writer commits. \
         Contract load failure surfaces as a status label, not a hard reject — task-scope-guard.mjs \
         remains the post-commit authoritative gate. \
         Wave19-08: action=complete also accepts `task_contract_path` / `task_report_path` / \
         `verifier_status` / `verifier_notes` as optional metadata recorded into the completion entry. \
         When `enforce_scoped_commit=true` AND `task_contract_path` is supplied, daemon loads the \
         contract (read-only), requires `commit_hash`, and asserts every `:write-scope` entry is \
         covered by an active/released claim or a `staged_files` path — rejects with structured \
         TASK_CONTRACT_REQUIRED / TASK_CONTRACT_MALFORMED / COMMIT_HASH_REQUIRED_FOR_CONTRACT / \
         CLAIM_SCOPE_MISSING errors when critical data is missing. Daemon NEVER runs the verifier \
         itself — verifier_status is reported by the caller (e.g. via scripts/verify-task-contract.mjs). \
         Wave21-03: action=complete also accepts `task_run_verifier_status` / `shared_memory_path` / \
         `verifier_diagnostics` / `verified` as optional task-run verifier metadata (counterpart to the \
         wave19-08 contract verifier slots — see wave21-02 scripts/verify-task-run.mjs). \
         Wave22-02 SHIFT: the wave21-03 `verified=true` escape hatch is now a legacy-compat fallback. \
         When the writer supplies all four of `task_contract_path` / `task_report_path` / \
         `shared_memory_path` / `commit_hash`, daemon runs the in-tree task-run auto-verifier ITSELF \
         (read-only file inspection — no Node spawn, no shell, no mutating git) and computes the \
         verdict as `verifier_status` (passed) plus `verification_source=\"daemon-auto-verifier\"` \
         and `verified_scope_summary` (structured per-rule cross-check). Auto-verifier failures \
         surface deterministic structured codes — TASK_CONTRACT_REQUIRED / TASK_CONTRACT_MALFORMED / \
         TASK_REPORT_REQUIRED / TASK_REPORT_MALFORMED / TASK_REPORT_TASK_ID_MISMATCH / \
         TASK_REPORT_COMMIT_HASH_MISMATCH / SHARED_MEMORY_REQUIRED / SHARED_MEMORY_MALFORMED / \
         SHARED_MEMORY_NO_COMPLETION_FOR_TASK — BEFORE any companion log mutation. When the writer \
         sets `verified=true` WITHOUT supplying all four paths, daemon downgrades to \
         `verification_source=\"legacy-caller-claim\"` (no hard reject; `verifier_status=\"unknown\"` \
         and `verifier_diagnostics` lists the missing paths so callers can migrate). Daemon never \
         spawns Node here either. action=preflight_commit also echoes \
         `task_report_path` / `shared_memory_path` advisory hints when supplied. \
         Lisp 源: intent-memory.lisp :: agent-execution-coordination + \
         intent-worker.lisp :: agent-execution-manager-interface + intent-flow.lisp :: \
         F-execution-log-governance + F-scoped-commit-handoff。注意:event-bus ExecutionEvent::* 暂未发射,等域扩展落地。",
        schema,
    )]
}
