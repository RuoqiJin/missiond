;; Wave 24 shared-memory ledger.
;; Schema: .missiond/tasks/schema/shared-memory-v1.lisp
;;
;; Append-only. Agents add entries while they hold a live :claim for their
;; task id. Editing or removing prior entries is forbidden; append a
;; (correction ...) entry instead.

(shared-memory wave24
  :schema "missiond.shared-memory.v1"
  :wave wave24
  :created-at "2026-04-28T09:00:00+08:00"
  :sequence 1

  (observation
    :id wave24-bootstrap-001
    :task wave24-00-archive-wave23-artifacts
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T09:00:00+08:00"
    :touched []
    :summary "Bootstrap entry: Wave 24 turns session traces into a dry-run router policy loop while keeping ClaudeCode as the default runtime backend.")

  (claim
    :id wave24-00-claim-001
    :task wave24-00-archive-wave23-artifacts
    :agent claudecode
    :seq 2
    :at "2026-04-28T09:30:00+08:00"
    :touched [".missiond/tasks/wave23/**"
              ".missiond/claudecode/wave23-*.md"]
    :summary "Claim wave24-00-archive-wave23-artifacts: archive untracked Wave 23 task contracts, rendered briefs, reports, shared-memory.lisp and session-trace.lisp. Honors must-not-touch: crates/** scripts/** .missiond/v2/** .missiond/tasks/wave24/**.")

  (completion
    :id wave24-00-completion-001
    :task wave24-00-archive-wave23-artifacts
    :agent claudecode
    :seq 3
    :at "2026-04-28T09:40:00+08:00"
    :touched [".missiond/tasks/wave23/shared-memory.lisp"
              ".missiond/tasks/wave23/wave23-00-archive-wave22-task-artifacts.lisp"
              ".missiond/tasks/wave23/wave23-02-renderer-report-trace-fields-v1.lisp"
              ".missiond/tasks/wave23/wave23-03-task-run-verifier-trace-v1.lisp"
              ".missiond/tasks/wave23/wave23-04-execution-session-trace-integration-v0.lisp"
              ".missiond/tasks/wave23/wave23-05-plan-workstation-session-trace-v0.lisp"
              ".missiond/tasks/wave23/wave23-06-trace-summary-analyzer-v0.lisp"
              ".missiond/tasks/wave23/wave23-07-router-policy-draft-from-trace-v0.lisp"
              ".missiond/tasks/wave23/wave23-08-lisp-backfill-wave23-status.lisp"
              ".missiond/tasks/wave23/wave23-09-parallel-dispatch-index.lisp"
              ".missiond/tasks/wave23/reports/wave23-00-archive-wave22-task-artifacts.report.lisp"
              ".missiond/tasks/wave23/reports/wave23-01-session-trace-schema-v0.report.lisp"
              ".missiond/tasks/wave23/reports/wave23-02-renderer-report-trace-fields-v1.report.lisp"
              ".missiond/tasks/wave23/reports/wave23-03-task-run-verifier-trace-v1.report.lisp"
              ".missiond/tasks/wave23/reports/wave23-04-execution-session-trace-integration-v0.report.lisp"
              ".missiond/tasks/wave23/reports/wave23-05-plan-workstation-session-trace-v0.report.lisp"
              ".missiond/tasks/wave23/reports/wave23-06-trace-summary-analyzer-v0.report.lisp"
              ".missiond/claudecode/wave23-00-archive-wave22-task-artifacts.md"
              ".missiond/claudecode/wave23-02-renderer-report-trace-fields-v1.md"
              ".missiond/claudecode/wave23-03-task-run-verifier-trace-v1.md"
              ".missiond/claudecode/wave23-04-execution-session-trace-integration-v0.md"
              ".missiond/claudecode/wave23-05-plan-workstation-session-trace-v0.md"
              ".missiond/claudecode/wave23-06-trace-summary-analyzer-v0.md"
              ".missiond/claudecode/wave23-07-router-policy-draft-from-trace-v0.md"
              ".missiond/claudecode/wave23-08-lisp-backfill-wave23-status.md"
              ".missiond/claudecode/wave23-09-parallel-dispatch-index.md"]
    :summary "Committed wave24-00 (commit a9840a5323e5: 26 files / +2245). Archived 9 wave23 task contracts (00, 02-09; wave23-01 was already tracked from a prior commit), 9 rendered ClaudeCode briefs (matching set), 7 wave23 reports (00-06; tasks 07/08/09 did not emit reports — wave23-07/08/09 reports intentionally absent), and the wave23 shared-memory.lisp ledger. wave23 session-trace.lisp was already tracked. Acceptance: check-task-contract --all → 66 OK; check-task-memory wave23 → 16 entries OK; check-session-trace wave23 → 1 event OK; git diff --check clean; task-scope-guard --mode staged → 26 files OK; verify-task-contract → OK against a9840a5. Hooks preflight aligned (core.hooksPath==.githooks). Did NOT touch crates/**, scripts/**, .missiond/v2/**, .missiond/tasks/wave24/** (only the explicitly-permitted wave24 ledger appends per :session-trace-writable true and shared-memory protocol).")

  (claim
    :id wave24-01-claim-001
    :task wave24-01-router-policy-schema-v1
    :agent claudecode
    :seq 4
    :at "2026-04-28T10:00:00+08:00"
    :touched [".missiond/tasks/schema/router-policy-v1.lisp"
              ".missiond/router/router-policy-v1.lisp"
              "scripts/check-router-policy.mjs"]
    :summary "Claim wave24-01-router-policy-schema-v1: add router-policy v1 Lisp schema + seed policy + read-only checker (with --json / --dry-fixture). Schema enumerates backend classes claudecode/missiond-llm-router/deterministic-checker/patch-worker/verifier-worker; checker REJECTS policies missing :dry-run-only true or :runtime-replacement false (cross-wave invariant: router output is advisory only). No runtime dispatch integration in this task.")

  (claim
    :id wave24-02-claim-001
    :task wave24-02-trace-corpus-index-v0
    :agent claudecode
    :seq 5
    :at "2026-04-28T10:30:00+08:00"
    :touched ["scripts/build-session-trace-index.mjs"
              "scripts/analyze-session-trace.mjs"
              "scripts/check-session-trace.mjs"]
    :summary "Claim wave24-02-trace-corpus-index-v0: add read-only scripts/build-session-trace-index.mjs that scans .missiond/tasks/**/session-trace.lisp via Node fs (no shell glob) and emits stable JSON with totals/by_task/by_backend/by_wave/bottleneck_tags/source_files. Reuses parseTraceEvents from check-session-trace.mjs and matches wave23-06 analyzer thresholds (long-running >=1.8e6 ms, high-retry >=3, many-failures >=2, no-completion = dispatched but never complete). No router recommendations here; pure indexer. Honors must-not-touch: crates/** .missiond/v2/** .missiond/tasks/schema/*.lisp .missiond/tasks/wave23/** .missiond/tasks/wave24/wave24-*.lisp .missiond/claudecode/**.")

  (completion
    :id wave24-01-completion-001
    :task wave24-01-router-policy-schema-v1
    :agent claudecode
    :seq 6
    :at "2026-04-28T10:50:00+08:00"
    :touched [".missiond/tasks/schema/router-policy-v1.lisp"
              ".missiond/router/router-policy-v1.lisp"
              "scripts/check-router-policy.mjs"]
    :refs [wave24-01-claim-001]
    :summary "Committed wave24-01 (commit 988f7d88b467: 3 files / +1102). Schema + seed policy (3 rules: docs→claudecode, deterministic-checker, post-commit verifier) + read-only checker (scripts/check-router-policy.mjs with --json / --dry-fixture). Backend enum: claudecode/missiond-llm-router/deterministic-checker/patch-worker/verifier-worker. Cross-wave invariant ENFORCED: checker rejects any policy missing :dry-run-only true or :runtime-replacement false. Acceptance: --dry-fixture 19/19 across 13 categories OK; seed policy validates 3 rules OK; check-task-contract --all 66 OK; git diff --check clean; task-scope-guard staged 3 files OK; verify-task-contract OK. Hooks preflight aligned. Did NOT touch crates/**, .missiond/v2/**, .missiond/tasks/wave23/**, other wave24 contracts, or .missiond/claudecode/**.")

  (completion
    :id wave24-02-completion-001
    :task wave24-02-trace-corpus-index-v0
    :agent claudecode
    :seq 7
    :at "2026-04-28T11:00:00+08:00"
    :touched ["scripts/build-session-trace-index.mjs"]
    :refs [wave24-02-claim-001]
    :summary "Committed wave24-02 (commit e61088b8d300: 1 file / +910). New scripts/build-session-trace-index.mjs is a read-only corpus indexer that scans .missiond/tasks/**/session-trace.lisp via Node fs.readdirSync recursion (no shell glob), reuses parseTraceEvents from check-session-trace.mjs, and emits stable sorted JSON with top-level keys: bottleneck_tags, by_backend, by_task, by_wave, schema, source_files, thresholds, totals. Bottleneck thresholds match wave23-06 analyzer (long-running >=1.8e6 ms, high-retry >=3, many-failures >=2, no-completion = dispatched but never complete). NO file writes in production path; the only fs.writeFileSync/fs.mkdtempSync is inside runFixtures, scoped to an OS tmp dir that is rm'd at end. Acceptance: --dry-fixture 7/7 cases pass (clean / multi-wave / bottleneck-tagged / no-completion / multi-backend / empty-corpus / deterministic-stable-json); --json wave23 trace OK; check-session-trace --dry-fixture 22/22 cases green; check-task-contract --all 66 OK; git diff --check clean; task-scope-guard staged 1 file OK; verify-task-contract OK against e61088b8d300; analyze-session-trace --dry-fixture 6/6 still green (file untouched). Did NOT touch crates/**, .missiond/v2/**, .missiond/tasks/schema/*.lisp, .missiond/tasks/wave23/**, other wave24 contracts, or .missiond/claudecode/**. analyze-session-trace.mjs and check-session-trace.mjs were NOT extended (existing parseTraceEvents export was sufficient). No router recommendations emitted (per contract) — pure indexer.")

  (claim
    :id wave24-03-claim-001
    :task wave24-03-router-recommendation-cli-v0
    :agent claudecode
    :seq 8
    :at "2026-04-28T11:30:00+08:00"
    :touched ["scripts/recommend-task-backend.mjs"
              "scripts/check-router-policy.mjs"]
    :summary "Claim wave24-03-router-recommendation-cli-v0: add read-only deterministic CLI scripts/recommend-task-backend.mjs that consumes a task contract + router-policy + optional trace-index JSON and emits an explainable backend recommendation. Extends wave24-01 check-router-policy.mjs with named exports (projectRule/projectPolicy/readRouterPolicyFile) so the CLI doesn't re-parse Lisp; build-session-trace-index.mjs already exports buildIndex so it was NOT modified. Implements 8 predicate heads (kind/dispatch_strategy/dispatch-strategy/owner/status/path-glob/any/all) and selection by lowest priority wins. Hard guarantees: dry_run_only is ALWAYS true in output; no shell-out, no LLM, no git, no HTTP; no-match fallback recommends claudecode/low/insufficient_trace_history. Honors must-not-touch: crates/** .missiond/v2/** .missiond/tasks/schema/*.lisp .missiond/tasks/wave23/** .missiond/tasks/wave24/wave24-*.lisp .missiond/claudecode/**.")

  (completion
    :id wave24-03-completion-001
    :task wave24-03-router-recommendation-cli-v0
    :agent claudecode
    :seq 9
    :at "2026-04-28T11:55:00+08:00"
    :touched ["scripts/recommend-task-backend.mjs"
              "scripts/check-router-policy.mjs"]
    :refs [wave24-03-claim-001]
    :summary "Committed wave24-03 (commit d6d8e102e1aa: 2 files / +1144 / -1). New scripts/recommend-task-backend.mjs is a read-only deterministic CLI that consumes a task contract + router-policy + optional trace-index JSON and emits an explainable JSON recommendation with top-level keys: backend, chosen_rule_id, confidence, dry_run_only, evidence, matched_rules, non_goals, policy_path, rejected_rules, schema, task_id, task_path. Confidence rules: high if matched + max(task,backend) events >=5, medium if 1..4, low if 0/no-match. No-match fallback recommends claudecode + confidence low + evidence.reason=insufficient_trace_history. Cross-wave invariant re-checked: any policy with :runtime-replacement true exits non-zero before any matching. dry_run_only is hard-coded literal true on EVERY output. check-router-policy.mjs extended with three named exports: projectRule (single-rule projection), projectPolicy (whole-policy projection sorted by priority asc), readRouterPolicyFile (file -> policy projection). build-session-trace-index.mjs was NOT modified (its existing buildIndex named export was sufficient). Acceptance: --dry-fixture 10/10 across 10 categories OK (matched-high, matched-medium, matched-low, fallback, priority-ordering, rejected-explain, invariant, deterministic, malformed-task edge, runtime-replacement edge); recommend-task-backend --task wave24-01.lisp --policy router-policy-v1.lisp --json correctly recommends deterministic-checker via r-deterministic-checker-tasks rule; check-router-policy --dry-fixture 19/19 still green (CLI surface unchanged); check-task-contract --all 66 OK; git diff --check clean; task-scope-guard staged 2 files OK; verify-task-contract OK against d6d8e102e1aa. Audit: grep -E 'child_process|spawn|fetch|http|https|exec|git' returns 2 hits both in comments (line 24 documents 'no shell-out, no LLM, no HTTP, no git' invariant; line 316 contains 'legitimately' as substring). Did NOT touch crates/**, .missiond/v2/**, .missiond/tasks/schema/*.lisp, .missiond/tasks/wave23/**, other wave24 contracts, or .missiond/claudecode/**.")

  (claim
    :id wave24-05-claim-001
    :task wave24-05-renderer-router-context-v0
    :agent claudecode
    :seq 10
    :at "2026-04-28T12:30:00+08:00"
    :touched ["scripts/render-claudecode-task.mjs"
              ".missiond/tasks/schema/task-contract-v1.lisp"]
    :summary "Claim wave24-05-renderer-router-context-v0: surface router-policy context as an advisory dry-run section in rendered ClaudeCode briefs without making Markdown load-bearing. Plan: (1) add optional :router-policy-path field to task-contract-v1.lisp alongside :session-trace-writable; (2) add renderRouterPolicy(task) helper to render-claudecode-task.mjs that auto-detects task.routerPolicyPath then falls back to .missiond/router/router-policy-v1.lisp; section text contains literal 'advisory' AND 'dry-run only' and never instructs ClaudeCode to switch backend; (3) place section between Session Trace and Commit, mirroring wave23-02 auto-detect pattern. Renderer remains pure file-read + text-write — no shell-out to recommend-task-backend.mjs. Honors must-not-touch: crates/**, .missiond/v2/**, .missiond/tasks/wave23/**, .missiond/tasks/wave24/wave24-*.lisp, .missiond/claudecode/wave23-*.md.")

  (claim
    :id wave24-04-claim-001
    :task wave24-04-plan-router-dry-run-surface-v0
    :agent claudecode
    :seq 11
    :at "2026-04-28T13:00:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Claim wave24-04-plan-router-dry-run-surface-v0: expose dry-run-only router recommendation block on mission_plan(action=execute). Pure Rust deterministic helper mirrors wave24-03 Node CLI algorithm (8 predicate heads, lowest-priority-wins selection, no-match fallback to claudecode/low). NEW args router_policy_mode (off|dry_run; apply/auto/unknown -> INVALID_PARAM) and router_policy_path (default .missiond/router/router-policy-v1.lisp). applied=false hard-coded literal; runtime-replacement=true policies emit status=rejected. Runtime dispatch path NOT touched: dispatch_strategy/target/workstation_dispatch/auto_spawn/evidence side-effects all unchanged when mode=off (byte-identical legacy response) or dry_run (advisory block appended after dispatch resolution; same target/strategy). No shell-out, no Node spawn, no scripts/ invocation. Honors must-not-touch: workstation_dispatch.rs / agent_execution.rs / unified_entry.rs / plan_dag.rs / scripts/** / .missiond/v2/** / .missiond/tasks/** (only the explicitly-permitted wave24 ledger appends per :session-trace-writable true and shared-memory protocol).")

  (completion
    :id wave24-05-completion-001
    :task wave24-05-renderer-router-context-v0
    :agent claudecode
    :seq 12
    :at "2026-04-28T13:15:00+08:00"
    :touched ["scripts/render-claudecode-task.mjs"
              ".missiond/tasks/schema/task-contract-v1.lisp"]
    :refs [wave24-05-claim-001]
    :summary "Committed wave24-05 (commit 294a92a18318: 2 files / +60 / -4). Added optional :router-policy-path field to task-contract-v1.lisp alongside :session-trace-writable, with explicit MUST-contain-'advisory'-and-'dry-run only' contract and never-shell-out-to-recommend-task-backend boundary documented in field-contract + renderer-contract :machine-context-rendered + :backward-compatibility entries. Added renderRouterPolicy helper to render-claudecode-task.mjs; section sits between Session Trace and Commit; auto-detect precedence is task.routerPolicyPath then .missiond/router/router-policy-v1.lisp; section omitted entirely when neither resolves on disk. Rendered output of wave24-01 brief contains literal 'advisory' (line 102 header + line 106 bullet) and 'dry-run only' (line 106 bullet) verbatim, and the bullet explicitly states 'runtime dispatch is unchanged — ClaudeCode remains the live backend for this task'. wave23-02 Session Trace section is byte-identical (sed-compared lines 87-99 of stored brief vs lines 87-99 of fresh stdout render). Acceptance: render-claudecode-task --stdout exit 0; check-task-contract --all 66 OK; check-task-report --dry-fixture 16/16 OK (preserves wave23-02 worker-explanation fixtures including :time_sinks/:major_decisions/:unexpected_work/:blockers/:trace_refs); git diff --check clean for both files; task-scope-guard staged 2 files OK; verify-task-contract OK against 294a92a18318. Renderer audit (`grep -nE 'child_process|spawn|execSync|fork|recommend-task-backend'`): only one hit at line 311 — a comment forbidding shell-out. No new imports added; renderer remains pure file-read + text-write. Did NOT touch crates/**, .missiond/v2/**, .missiond/tasks/wave23/**, other wave24 contracts, or .missiond/claudecode/wave23-*.md.")

  (completion
    :id wave24-04-completion-001
    :task wave24-04-plan-router-dry-run-surface-v0
    :agent claudecode
    :seq 13
    :at "2026-04-28T13:35:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :refs [wave24-04-claim-001]
    :summary "Committed wave24-04 (commit b8721ab2d0dd: 2 files / +1662 / -1). Added private mod router_policy_dry_run inside plan.rs containing: (1) RouterPolicyMode enum (Off|DryRun) + parse_router_policy_mode validator that rejects apply/auto/unknown/non-string with structured INVALID_PARAM at preflight (BEFORE plan lookup); (2) attach_router_recommendation_block helper that splices a router_recommendation block into the response only when mode=DryRun (no-op for Off ⇒ byte-identical response to wave-15..23); (3) pure Rust Lisp tokenizer + parser for the wave24-01 router-policy v1 schema (atoms / strings / keywords / lists / brackets / line comments); (4) 8 predicate heads (kind / dispatch_strategy / dispatch-strategy / owner / status / path-glob / any / all) with custom glob matcher (no regex crate added); (5) lowest-priority-wins selector mirroring scripts/recommend-task-backend.mjs algorithm. Cross-wave invariants: applied=false hard-coded literal in EVERY emitted block; runtime-replacement=true policies emit status='rejected'; missing :dry-run-only true emits status='rejected'; I/O / parse failures emit status='error'; no-match falls back to claudecode/low/insufficient_trace_history. Confidence model option (a) per the brief: matched ⇒ medium, fallback ⇒ low (no trace-index loader needed in the daemon). Wired into action_execute via a single new validation call right after execute_mode validation and a single new attach call right before the function returns. The runtime dispatch path (target / dispatch_strategy / next_call / workstation_dispatch / auto_spawn / evidence) is byte-identical with vs without the dry_run flag — verified by router_policy_mode_dry_run_does_not_change_dispatch test asserting equality on every dispatch-shaping field. MCP schema (crates/missiond-mcp/src/tools/knowledge/plan.rs) gains 2 optional properties: router_policy_mode (enum off|dry_run) + router_policy_path (string). 14 new tests under handlers::knowledge::plan::tests: router_policy_mode_default_off_emits_no_block / router_policy_mode_off_returns_legacy_response_byte_identical / router_policy_mode_apply_returns_invalid_param / router_policy_mode_auto_returns_invalid_param / router_policy_mode_unknown_returns_invalid_param / router_policy_mode_dry_run_emits_block_with_applied_false / router_policy_mode_dry_run_does_not_change_dispatch / router_policy_mode_dry_run_no_match_falls_back_to_claudecode_low / router_policy_mode_dry_run_first_priority_match_wins / router_policy_mode_dry_run_runtime_replacement_policy_rejected / router_policy_mode_dry_run_missing_dry_run_only_rejected / router_policy_mode_dry_run_unreadable_policy_emits_error_status / router_policy_mode_dry_run_predicate_path_glob_matches_owned_files / router_policy_mode_dry_run_predicate_any_or_clause. Acceptance: cargo test handlers::knowledge::plan::tests 345/345 OK (331 baseline + 14 new); cargo test -p missiond-daemon 1636/1636 OK (1622 baseline + 14 new); cargo test -p missiond-mcp --lib 17/17 OK; cargo build --workspace OK; check-task-contract --all 66 OK; git diff --check clean; task-scope-guard staged 2 files OK; verify-task-contract OK against b8721ab2d0dd. Daemon NEVER shells out, NEVER spawns Node, NEVER calls scripts/* — pure Rust deterministic mirror of the wave24-03 CLI algorithm. Did NOT touch workstation_dispatch.rs, agent_execution.rs, unified_entry.rs, plan_dag.rs, scripts/**, .missiond/v2/**, or .missiond/tasks/** (only the explicitly-permitted wave24 ledger appends per :session-trace-writable true and shared-memory protocol).")

  (claim
    :id wave24-06-claim-001
    :task wave24-06-router-dry-run-smoke-v0
    :agent claudecode
    :seq 14
    :at "2026-04-28T14:00:00+08:00"
    :touched ["scripts/recommend-task-backend.mjs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
    :summary "Claim wave24-06-router-dry-run-smoke-v0: pin the wave24 advisory chain (trace-index → recommendation → renderer → daemon dry-run surface) end-to-end with deterministic smoke coverage. Two layers: (A) Node smoke fixture inside scripts/recommend-task-backend.mjs --dry-fixture that exercises the FULL chain in-process — synthesizes a tmp trace corpus, calls buildIndex (named export of build-session-trace-index.mjs), reads the wave24-01 seed policy via readRouterPolicyFile, calls recommend(), and asserts applied=false / dry_run_only=true / chosen backend matches the schema enum / loaded renderer source contains literal 'advisory' AND 'dry-run only'; (B) one new daemon end-to-end test in handlers::knowledge::plan::tests pinning the cross-wave invariants in a single shape (applied=false literal, recommended_backend in enum, dispatch fields byte-identical to baseline, schema field present, status='computed'). Stays read-only: no shell-out, no LLM, no spawn, no git mutation. Honors must-not-touch: workstation_dispatch.rs / agent_execution.rs / unified_entry.rs / .missiond/v2/** / .missiond/tasks/wave23/** / wave24-*.lisp (only the explicitly-permitted wave24 ledger appends per :session-trace-writable true and shared-memory protocol).")

  (completion
    :id wave24-06-completion-001
    :task wave24-06-router-dry-run-smoke-v0
    :agent claudecode
    :seq 15
    :at "2026-04-28T14:30:00+08:00"
    :touched ["scripts/recommend-task-backend.mjs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
    :refs [wave24-06-claim-001]
    :summary "Committed wave24-06 (commit 6afe5414f4a7d5064540b529bb602ba215496461: 2 files / +429). Two-layer smoke pinning the wave24 advisory chain end-to-end. Layer A — scripts/recommend-task-backend.mjs gains 1 new fixture 'smoke-e2e-chain' (category bumps 10→11 categories, fixtures 10→11): synthesises a tmp session-trace corpus, runs parseTraceEvents (imported from check-session-trace.mjs) → buildIndex (named export of build-session-trace-index.mjs), reads the wave24-01 seed via readRouterPolicyFile, calls recommend() on a docs task, asserts dry_run_only=true literal in stable JSON / chosen_rule_id=r-docs-to-claudecode / backend ∈ BACKEND_CLASSES / evidence.task_event_count=5 / schema=SCHEMA. Reads renderer source AND any on-disk wave24 brief to confirm both 'advisory' (case-insensitive in brief, literal 'Router Policy (advisory)' in renderer) and 'dry-run only' literals. Static audit asserts forbidden patterns (child_process / spawn / execSync / fork / openai / anthropic / chat.completion) ABSENT in active source (line and block comments stripped) of check-router-policy.mjs and build-session-trace-index.mjs; the audit table is assembled from string parts so the audit body itself does not appear as a literal substring in recommend-task-backend.mjs. Layer B — crates/missiond-daemon/src/handlers/knowledge/plan.rs gains 1 new daemon test router_policy_dry_run_smoke_pins_cross_wave_invariants under handlers::knowledge::plan::tests (count 345→346): materialises a temp seed-shape policy, drives action_execute_bridge baseline + dry_run, asserts applied is the literal Value::Bool(false) (not string-compared) / recommended_backend ∈ ['claudecode','missiond-llm-router','deterministic-checker','patch-worker','verifier-worker'] / status='computed' / schema='missiond.router-recommendation.v0' / target_tool / target_source / dispatch_strategy / dispatch_strategy_source / next_call / execute_mode / runner_status all byte-identical baseline-vs-dry_run / baseline carries no router_recommendation block / reasons array references the matched rule id 'r-docs-to-claudecode'. Acceptance: node scripts/build-session-trace-index.mjs --dry-fixture 7/7 OK (baseline preserved); node scripts/recommend-task-backend.mjs --dry-fixture 11/11 OK (10 baseline + 1 new); cargo test -p missiond-daemon handlers::knowledge::plan::tests 346/346 OK (345 baseline + 1 new); cargo test -p missiond-daemon 1637/1637 OK (1636 baseline + 1 new); cargo test -p missiond-mcp --lib 17/17 OK; cargo build --workspace OK; git diff --check clean for all 4 in-scope files; task-scope-guard --mode staged OK on 2 files; verify-task-contract OK against 6afe5414f4a7; check-missiond-hooks --json reason=aligned. Cross-wave invariants pinned by the smoke layers: (1) dry_run_only=true [Node + Rust schema], (2) :runtime-replacement false REQUIRED [carried via wave24-04 dry_run_runtime_replacement_policy_rejected, smoke does not regress it], (3) applied=false literal in EVERY response [Rust assert_eq!(applied, Value::Bool(false))], (4) NO LLM [audit + ABSENT openai/anthropic/chat.completion patterns], (5) NO spawn [audit + ABSENT child_process/spawn/execSync/fork patterns], (6) NO mutating git [no new git invocation introduced], (7) mode=off byte-identical to baseline [carried via wave24-04 router_policy_mode_off_returns_legacy_response_byte_identical], (8) renderer 'advisory' + 'dry-run only' literals [Node fixture reads renderer source + wave24 brief]. No new MCP tool added. Daemon NEVER shells out, NEVER spawns Node, NEVER calls scripts/* — pure Rust deterministic mirror of the wave24-03 CLI algorithm preserved. Did NOT touch workstation_dispatch.rs, agent_execution.rs, unified_entry.rs, .missiond/v2/**, .missiond/tasks/wave23/**, or wave24-*.lisp (only the explicitly-permitted wave24 ledger appends per :session-trace-writable true and shared-memory protocol)."))
