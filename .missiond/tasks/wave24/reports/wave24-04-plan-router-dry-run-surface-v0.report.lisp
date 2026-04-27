;; Wave 24 / Task 04 — mission_plan router dry-run surface v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave24/wave24-04-plan-router-dry-run-surface-v0.lisp

(report wave24-04-plan-router-dry-run-surface-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave24-04-plan-router-dry-run-surface-v0"
  :status done
  :commit_hash "b8721ab2d0dd"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "345/345 tests pass (331 baseline + 14 new). New tests under handlers::knowledge::plan::tests: router_policy_mode_default_off_emits_no_block (absent arg ⇒ Off; attach is byte-identical no-op), router_policy_mode_off_returns_legacy_response_byte_identical (explicit \"off\" same as default; no router_recommendation key in payload), router_policy_mode_apply_returns_invalid_param (apply rejected at preflight with INVALID_PARAM; error message echoes offending value), router_policy_mode_auto_returns_invalid_param (auto rejected), router_policy_mode_unknown_returns_invalid_param (typos like \"dryrun\" / \"DRY_RUN\" / non-string bool rejected), router_policy_mode_dry_run_emits_block_with_applied_false (block has applied=false literal + status/recommended_backend/confidence/reasons/policy_source/schema), router_policy_mode_dry_run_does_not_change_dispatch (target_tool/target_source/dispatch_strategy/dispatch_strategy_source/next_call/execute_mode/runner_status all byte-identical with vs without dry_run; only delta is the additive router_recommendation block), router_policy_mode_dry_run_no_match_falls_back_to_claudecode_low (kind=ops off-policy ⇒ recommended_backend=claudecode confidence=low reasons contains insufficient_trace_history), router_policy_mode_dry_run_first_priority_match_wins (two rules match at priority 5 and 50; backend comes from the priority-5 rule; reasons surface BOTH matched rule ids), router_policy_mode_dry_run_runtime_replacement_policy_rejected (cross-wave invariant: :runtime-replacement true ⇒ status=\"rejected\" applied=false reasons mentions runtime-replacement; even with a matching rule), router_policy_mode_dry_run_missing_dry_run_only_rejected (cross-wave invariant: :dry-run-only false ⇒ status=\"rejected\"), router_policy_mode_dry_run_unreadable_policy_emits_error_status (I/O failure ⇒ status=\"error\" applied=false recommended_backend=claudecode), router_policy_mode_dry_run_predicate_path_glob_matches_owned_files (path-glob predicate matches via owned_files; non-matching paths fall through to fallback), router_policy_mode_dry_run_predicate_any_or_clause (any-clause matches kind=review and kind=smoke; non-matching kind=ops falls back).")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1636/1636 tests pass (1622 baseline + 14 new). e2e_bus_golden_path integration test still appropriately ignored. Wave-23-05 baseline of 1622 + 14 new = 1636.")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17/17 tests pass — mcp lib baseline of 17 unchanged. The 2 new schema entries (router_policy_mode + router_policy_path) are picked up by the existing test_get_tool / test_directive_plan_workflow_surfaces_registered registrations without requiring new tests in this crate.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Workspace builds cleanly. Zero new warnings introduced by the router_policy_dry_run module — verified via `cargo build -p missiond-daemon 2>&1 | grep -E 'warning:.*router_policy|warning:.*Sexp|warning:.*GlobRegex|router_policy_dry_run'` returning no hits. The 86 pre-existing warnings in missiond-daemon (Deprecated field reads, dead code etc.) are all wave-22-and-earlier baseline noise unrelated to this task.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (66 tasks). The wave24-04 contract itself plus all other contracts validate; no schema changes in this task.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across both in-scope crate paths. No trailing whitespace, no mixed tabs/spaces, no conflict markers.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave24/wave24-04-plan-router-dry-run-surface-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave24-04-plan-router-dry-run-surface-v0 (2 staged file(s)). Both staged paths are inside :write-scope; zero matches against :must-not-touch (workstation_dispatch.rs / agent_execution.rs / unified_entry.rs / plan_dag.rs / scripts/** / .missiond/v2/** / .missiond/tasks/**).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave24/wave24-04-plan-router-dry-run-surface-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave24-04-plan-router-dry-run-surface-v0 against b8721ab2d0dd — commit hash exists; commit message matches contract :commit.message exactly ('feat(plan): expose router dry-run recommendation'); changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "ok=true severity=ok matches=true reason=aligned — core.hooksPath==.githooks already set from prior waves; .githooks/pre-commit exists and is executable; no install needed.")]

  :scope_deviations []

  :trace_refs [wave24-04-trace-start-001 wave24-04-trace-commit-001 wave24-04-trace-complete-001]

  :unexpected_work
    [(:summary "Originally planned to use the wave24-01 seed policy file path (.missiond/router/router-policy-v1.lisp) as the test fixture for the no-match / shape / dispatch-unchanged tests. cargo test runs from the crate dir (crates/missiond-daemon), where that relative path does not resolve — the I/O fails and the recommendation falls into status=\"error\" instead of the expected status=\"computed\" fallback. Refactored those three tests to materialise a temp policy file via std::env::temp_dir() so the assertions are independent of the daemon's working directory. The DEFAULT_POLICY_PATH constant is still asserted (it still equals \".missiond/router/router-policy-v1.lisp\") to pin the documented default, but the test bodies don't depend on it being readable from the test cwd.")
     (:summary "Did NOT add a trace-index loader to the daemon recommendation helper. The brief offered two confidence-policy options: (a) skip trace evidence and emit medium for matched / low for fallback, or (b) parse the local default trace index. Picked option (a) per the brief's own recommendation — keeps the surface pure-Rust, deterministic, and additive. The wave24-03 Node CLI continues to support the richer trace-index path; the daemon helper documents this design choice in the module-level comment.")]

  :notes
    "Architecture (everything additive; runtime dispatch path untouched):

1. **Pre-flight validation** at the top of `action_execute` (right after `execute_mode` validation): `router_policy_dry_run::parse_router_policy_mode(args)` returns `RouterPolicyMode::{Off, DryRun}` or a structured `INVALID_PARAM` ToolResult. Default (absent / null / empty / \"off\") ⇒ Off. Anything else (\"apply\", \"auto\", \"dryrun\", non-string bool, etc.) ⇒ INVALID_PARAM with explicit suggestion 'expected one of: \"off\", \"dry_run\"'. Validation runs BEFORE plan lookup so a typo cannot route through any DB read or substrate dispatch.

2. **Recommendation block attachment** at the very end of `action_execute`, AFTER all other attach helpers (`attach_workstation_proposals_block`, `attach_workstation_auto_spawn_gate_block`, `attach_inference_block`, `attach_apply_gate_block`, `attach_persisted_apply_block`): `router_policy_dry_run::attach_router_recommendation_block(result, mode, args, &resolved, &plan)`. When `mode=Off`, returns `result` unchanged ⇒ wave-15..23 byte-shape preserved. When `mode=DryRun` and the result is not a structured error, computes the recommendation and splices a `router_recommendation` JSON object into the response payload (additively; never overwrites a pre-existing block).

3. **Pure Rust Lisp parser** for the wave24-01 router-policy v1 schema. Hand-written tokenizer (atoms / strings / keywords / lists / brackets / line comments) + recursive-descent parser. Top-level form must be `(router-policy <id> ...)`; supports `:dry-run-only` / `:runtime-replacement` keyword/atom pairs at the header level and `(rule ...)` subforms with `:id` / `:priority` / `:when` / `:recommend (:backend X :reasoning Y)`. Rules are sorted by priority ascending on construction so the selector is just `matched[0]`.

4. **Predicate evaluator** mirrors the wave24-03 Node CLI exactly:
   - `kind` / `dispatch_strategy` / `dispatch-strategy` (aliases) / `owner` / `status`: exact-match against the live execute context. dispatch_strategy reads from `resolved.dispatch_strategy` (which already encodes explicit_arg > plan_hint > parallelism > default precedence). status reads from args.status if present, else `plan.status.as_str()`.
   - `path-glob <pattern>`: true if any path in `args.owned_files` matches. The matcher is a hand-rolled recursive descent that mirrors scripts/lib/missiond_lisp.mjs `globToRegExp`: `**` matches any sequence including `/`; `*` matches any non-`/` sequence; `?` matches a single non-`/` char; literal chars match literally. No regex crate added.
   - `any` / `all`: logical OR / AND over child clauses. Top-level `:when` list is implicit-`all`.
   - Unknown predicate heads fail closed (returns Err) — wave24-01 checker rejects them upstream but the daemon helper re-checks defensively.

5. **Selection algorithm**: iterate rules in priority-ascending order; collect matched rules; first matched wins; backend = winning rule's `:recommend.backend`; confidence = `medium` for matched, `low` for fallback (option (a) per the brief — no trace-index loader added to the daemon).

6. **Cross-wave invariants** (every emitted block):
   - `applied` is hard-coded LITERAL `false`. Never computed conditionally. The constant is encoded in `error_block` / `rejected_block` / `computed_block` helpers and asserted in router_policy_mode_dry_run_emits_block_with_applied_false test (`block[\"applied\"] == false`).
   - Policy missing `:dry-run-only true` ⇒ `status=\"rejected\"`. Asserted in router_policy_mode_dry_run_missing_dry_run_only_rejected.
   - Policy declaring `:runtime-replacement true` ⇒ `status=\"rejected\"`. Asserted in router_policy_mode_dry_run_runtime_replacement_policy_rejected. Both invariants are enforced regardless of whether a rule would have matched — the rejection path runs BEFORE evaluation.
   - I/O failures (missing file, malformed Lisp) ⇒ `status=\"error\"`. Asserted in router_policy_mode_dry_run_unreadable_policy_emits_error_status.
   - No-match fallback ⇒ `recommended_backend=\"claudecode\"` + `confidence=\"low\"` + reasons contains literal `insufficient_trace_history`. Asserted in router_policy_mode_dry_run_no_match_falls_back_to_claudecode_low.

7. **Dispatch-unchanged proof**: router_policy_mode_dry_run_does_not_change_dispatch builds two ToolResult instances from the same plan + ResolvedExec. The first has no router knob; the second has `router_policy_mode=\"dry_run\"` with a temp policy. The test asserts byte-equality on every dispatch-shaping field: `target_tool`, `target_source`, `dispatch_strategy`, `dispatch_strategy_source`, `next_call`, `execute_mode`, `runner_status`. The ONLY delta is the additive `router_recommendation` block (asserted absent on baseline, present on dry_run).

Response block shape (top-level keys, returned by the helper):
- `applied` (boolean: ALWAYS false — cross-wave invariant)
- `confidence` (string: high|medium|low — wave24-04 uses medium for matched, low for fallback)
- `policy_source` (string — the input arg verbatim; mirrors the caller-supplied `router_policy_path` or the documented default `.missiond/router/router-policy-v1.lisp`)
- `reasons` (array of strings — matched rule ids + priorities + reasoning, plus fallback / error / rejection messages)
- `recommended_backend` (string — one of the 5 backend classes claudecode / missiond-llm-router / deterministic-checker / patch-worker / verifier-worker)
- `schema` (string: 'missiond.router-recommendation.v0' — same schema label as the wave24-03 CLI)
- `status` (string: computed | rejected | error)

MCP schema additions (crates/missiond-mcp/src/tools/knowledge/plan.rs):
- `router_policy_mode` — string enum [\"off\", \"dry_run\"]; the prop_enum constraint guarantees future-incompatible values are flagged at the JSON-Schema layer too. The Rust validator still rejects them with a structured INVALID_PARAM at runtime so the contract is enforced top-to-bottom.
- `router_policy_path` — string; default documented in the description as `.missiond/router/router-policy-v1.lisp` (the wave24-01 seed). When `router_policy_mode=\"off\"` (the default), the path is never read — no I/O happens in that path.

Hard guarantees — pure Rust, no shell-out, no Node spawn:
- The daemon NEVER calls `scripts/recommend-task-backend.mjs`, `scripts/check-router-policy.mjs`, or any other shell script.
- No `std::process::Command`, no `tokio::process::Command`, no `system()` calls inside `router_policy_dry_run`.
- The only filesystem operation is `std::fs::read_to_string(&policy_path)` for the policy file. No writes; no directory listings; no atomic-rename dance.
- Failure modes (I/O / parse / invariant violation) all surface on the recommendation block via `status` field; they NEVER alter the dispatch path or fail the execute call.

Workflow protocol (wave24 conventions followed):
- Hooks preflight: aligned (core.hooksPath==.githooks); checked via `node scripts/check-missiond-hooks.mjs --json` returning ok=true severity=ok matches=true reason=aligned. No install needed.
- Shared-memory ledger entries appended (claim seq=11 → completion seq=13; wave24-05 inserted a completion at seq=12 between my claim and completion as a parallel agent, which is permitted by append-only). Each entry uses unique :id (wave24-04-claim-001 / wave24-04-completion-001) and :touched lists only my actually-modified files.
- Session-trace ledger entries appended (start seq=15, commit seq=18 with :commit_hash b8721ab2d0dd, complete seq=19). All entries reference my prior trace ids in :trace_refs. wave24-05's parallel events landed at seqs 16-17, which is permitted by the strictly-increasing :seq rule.
- Pre-commit task-scope-guard (--mode staged) reported 2 staged files, both inside :write-scope, zero matches against :must-not-touch.
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation; the .githooks/pre-commit hook re-ran the same guard and confirmed the staged scope.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit b8721ab2d0dd against the contract — message exactly 'feat(plan): expose router dry-run recommendation', changed_files ⊆ write-scope, must-not-touch ∩ ∅, all green.

Constraints honored: did NOT touch crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs; did NOT touch agent_execution.rs; did NOT touch unified_entry.rs; did NOT touch plan_dag.rs; did NOT touch scripts/**; did NOT touch .missiond/v2/**; did NOT touch .missiond/tasks/** (only the explicitly-permitted wave24 ledger appends per :session-trace-writable true and shared-memory protocol — those entries are out-of-scope of the commit and remain untracked for the next wave's archive task per the established protocol). NO git add . / git stash / git reset / git checkout / amend / push / --no-verify / git add -A.")
