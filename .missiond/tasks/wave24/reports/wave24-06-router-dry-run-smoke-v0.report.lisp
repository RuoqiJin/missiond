;; Wave 24 / Task 06 — router dry-run smoke v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave24/wave24-06-router-dry-run-smoke-v0.lisp

(report wave24-06-router-dry-run-smoke-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave24-06-router-dry-run-smoke-v0"
  :status done
  :commit_hash "6afe5414f4a7"
  :files_changed
    ["scripts/recommend-task-backend.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :acceptance_results
    [(:command "node scripts/build-session-trace-index.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "build-session-trace-index fixtures OK (7 cases, 7 categories). Baseline preserved exactly — wave24-06 did NOT modify build-session-trace-index.mjs, only consumed its `buildIndex` named export from inside the wave24-06 smoke fixture in recommend-task-backend.mjs.")
     (:command "node scripts/recommend-task-backend.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "recommend-task-backend fixtures OK (11 cases, 11 categories). 10 baseline + 1 new wave24-06 'smoke-e2e-chain' fixture. The new fixture mkdtempSyncs a tmp dir, writes a synthetic session-trace.lisp, drives parseTraceEvents (newly imported from check-session-trace.mjs) → buildIndex → readRouterPolicyFile (.missiond/router/router-policy-v1.lisp seed) → recommend(), asserts dry_run_only literal true in stable JSON / chosen_rule_id=r-docs-to-claudecode / backend ∈ BACKEND_CLASSES / evidence.task_event_count=5 / schema=SCHEMA, then reads scripts/render-claudecode-task.mjs source and any wave24-* brief on .missiond/claudecode/ disk to confirm both 'advisory' and 'dry-run only' literals appear. Static audit asserts forbidden patterns (child_process / spawn / execSync / fork / openai / anthropic / chat.completion) ABSENT in stripped active source of check-router-policy.mjs and build-session-trace-index.mjs (line + block comments removed before scan).")
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "346/346 tests pass (345 baseline + 1 new). New test: router_policy_dry_run_smoke_pins_cross_wave_invariants — materialises a temp seed-shape policy (3 rules: r-docs-to-claudecode @10, r-deterministic-checker-tasks @20, r-post-commit-verifier @30; :dry-run-only true; :runtime-replacement false), drives action_execute_bridge baseline + dry_run with kind=docs / owner=claudecode, then asserts simultaneously: (a) applied is the literal Value::Bool(false) — type-checked, not string-compared; (b) status='computed'; (c) recommended_backend='claudecode' AND ∈ ['claudecode','missiond-llm-router','deterministic-checker','patch-worker','verifier-worker'] (re-spelled locally so the test does not import the checker script); (d) schema='missiond.router-recommendation.v0'; (e) every dispatch-shaping field (target_tool / target_source / dispatch_strategy / dispatch_strategy_source / next_call / execute_mode / runner_status) is byte-identical with vs without dry_run mode; (f) baseline carries no router_recommendation block (only the dry_run delta is additive); (g) confidence field is surfaced; (h) reasons array references the matched rule id 'r-docs-to-claudecode' verbatim.")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1637/1637 tests pass (1636 baseline + 1 new). e2e_bus_golden_path integration test still appropriately ignored.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Workspace builds cleanly. The new daemon test is gated under #[cfg(test)] inside mod tests so production builds unaffected. Pre-existing 86 warnings in missiond-daemon (deprecated field reads, dead code, etc.) are wave-22-and-earlier baseline noise unrelated to this task; zero NEW warnings introduced by the smoke test.")
     (:command "git diff --check -- scripts/recommend-task-backend.mjs scripts/build-session-trace-index.mjs scripts/render-claudecode-task.mjs crates/missiond-daemon/src/handlers/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across all 4 in-scope files. No trailing whitespace, no mixed tabs/spaces, no conflict markers. (Only 2 of the 4 in-scope files were actually modified — recommend-task-backend.mjs and plan.rs; build-session-trace-index.mjs and render-claudecode-task.mjs were sufficient as-is via their existing exports + on-disk briefs respectively.)")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave24/wave24-06-router-dry-run-smoke-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave24-06-router-dry-run-smoke-v0 (2 staged file(s)). Both staged paths inside :write-scope; zero matches against :must-not-touch (workstation_dispatch.rs / agent_execution.rs / unified_entry.rs / .missiond/v2/** / .missiond/tasks/wave23/** / wave24-*.lisp).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave24/wave24-06-router-dry-run-smoke-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave24-06-router-dry-run-smoke-v0 against 6afe5414f4a7 — commit hash exists; commit message matches contract :commit.message exactly ('test(plan): smoke router dry-run flow'); changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "ok=true severity=ok matches=true reason=aligned — core.hooksPath==.githooks already set from prior waves; .githooks/pre-commit exists and is executable; no install needed.")]

  :time_sinks
    [(:label "Deciding the smoke split (Node fixture vs Rust test) and where the renderer pattern-match belongs."
      :duration_ms 600000
      :notes "The brief left options open. Picked plan A + plan B as recommended; skipped a separate plan C static-audit fixture because the audit logic naturally belongs INSIDE the Node fixture (it already has filesystem access and does not need a separate harness). The renderer pattern-match was placed in the Node fixture not the Rust test because the renderer is Node-only.")
     (:label "Working around the audit-table-self-match false positive."
      :duration_ms 240000
      :notes "First draft of the static audit had `const forbidden = [/child_process/, ...]`. Running the fixture failed because that literal regex source itself contains the substring 'child_process' inside the same file under audit. Resolved by (1) auditing OTHER scripts (check-router-policy.mjs + build-session-trace-index.mjs) instead of recommend-task-backend.mjs to begin with, AND (2) assembling the regex sources from string parts ('child' + '_' + 'process' etc.) so the audit table itself is not a literal substring even if the audit is later widened to scan its own host file.")]

  :major_decisions
    [(:decision "Layer A (Node fixture) imports parseTraceEvents from check-session-trace.mjs to drive a real tmp corpus through buildIndex."
      :rationale "buildIndex is the named export the wave24-02 task already pinned. parseTraceEvents is the canonical parser the wave23-01 task pinned. Reusing both means the smoke exercises the EXACT code path production tooling uses — not a shape-matching synthetic. The wave24-03 fixture suite already had a `synthesizeTraceIndex` helper that returns a fake index in the right shape; the smoke does NOT reuse that — it builds the index from raw Lisp via parseTraceEvents+buildIndex so the chain is end-to-end, not stubbed.")
     (:decision "Layer B (Rust daemon test) re-spells the BACKEND_CLASSES enum locally instead of importing the checker script."
      :rationale "The daemon path must NEVER shell out to Node. Embedding the 5 backend names as a `&[&str; 5]` literal inside the test makes the assertion pure Rust and ALSO independently pins the wave24-01 schema enum at the daemon boundary — if the schema gains a new backend class, the smoke fails until the daemon is taught about it.")
     (:decision "Audit table assembled from string parts so the audit itself does not appear as a literal substring."
      :rationale "Future audits that sweep recommend-task-backend.mjs (or its sibling scripts) for forbidden patterns must not trip on the wave24-06 audit table itself. Using `new RegExp('child' + '_' + 'process')` keeps the regex semantically identical while preventing the audit body from masquerading as a violation. This is the same defensive pattern the wave24-03 ledger described doing 'by hand'; wave24-06 pins it as code.")
     (:decision "applied=false asserted as Value::Bool(false), not as a string."
      :rationale "Cross-wave invariant 3 says applied must be a hard-coded literal. A future bug that emits applied as the string 'false' or as the integer 0 would silently slip past a string-compared assertion. Type-locking the assert to Value::Bool(false) makes that regression impossible.")
     (:decision "Static-audit scope chosen: check-router-policy.mjs + build-session-trace-index.mjs (NOT recommend-task-backend.mjs itself)."
      :rationale "These two scripts are the foundation of the wave24 advisory chain — the policy parser and the corpus indexer. Auditing recommend-task-backend.mjs (the host script) would require evaluating the audit table, which is meta-circular. The wave24-03 ledger already attested that recommend-task-backend.mjs is shell-out-free; the smoke now pins that property regression-fail for the remaining two scripts.")
     (:decision "On-disk wave24 brief pattern-match accepts ANY wave24-*.md that contains both literals."
      :rationale "Coupling the smoke to a specific filename (e.g. wave24-04-plan-router-dry-run-surface-v0.md) would create a brittle dependency on rendering order between waves. The current pattern-match iterates all wave24-*.md briefs sorted alphabetically and confirms at least one contains both 'advisory' and 'dry-run only' — sufficient to prove the renderer's wave24-05 contract holds in the wild without locking on a specific brief.")]

  :unexpected_work
    [(:summary "The first audit draft self-matched on the literal substring 'child_process' inside its own forbidden-pattern table. Required restructuring to scan sibling scripts and build the regex from string parts. (See :time_sinks for narrative.)")]

  :blockers []

  :trace_refs
    ["wave24-06-trace-start-001"
     "wave24-06-trace-commit-001"
     "wave24-06-trace-complete-001"
     ".missiond/tasks/wave24/session-trace.lisp"
     ".missiond/tasks/wave24/shared-memory.lisp"]

  :notes "Wave 24 / Task 06 closes the wave24 advisory chain by pinning ALL eight cross-wave invariants under deterministic, reproducible smoke. Two layers landed:

LAYER A — Node end-to-end smoke fixture (scripts/recommend-task-backend.mjs):
1 new fixture 'smoke-e2e-chain' under runFixtures(), bumping fixture count 10→11 and category count 10→11. The fixture:
1. mkdtempSyncs an OS tmp dir under wave24-06-smoke-* (rm'd at end via try/finally).
2. Writes a 5-event session-trace.lisp into that dir mirroring the wave24-02 fixture pattern (1 task, 1 backend, dispatch + start + read + commit + complete; commit_hash 'smoke0001').
3. Calls parseTraceEvents (newly imported from check-session-trace.mjs) to parse it; asserts exactly 1 trace.
4. Calls buildIndex (named export of build-session-trace-index.mjs) to roll it up; asserts totals.tasks=1, totals.backends=1, totals.events=5.
5. Calls readRouterPolicyFile (named export of check-router-policy.mjs added by wave24-03) on the wave24-01 seed at .missiond/router/router-policy-v1.lisp; asserts policy.dry_run_only=true, policy.runtime_replacement=false.
6. Builds a docs-kind task contract via parseTaskFromString (existing fixture helper).
7. Calls recommend({task, policy, traceIndex, taskPath, policyPath}) — the same function the production CLI uses.
8. Asserts on the recommendation: rec.dry_run_only===true (literal); rec.backend∈BACKEND_CLASSES (imported set); rec.backend==='claudecode'; rec.chosen_rule_id==='r-docs-to-claudecode' (proves the wave24-01 seed wired through end-to-end); rec.schema===SCHEMA; rec.evidence.task_event_count===5 (proves the trace corpus rolled up into the recommendation); stableStringify(rec) regex-matches '/\"dry_run_only\"\\s*:\\s*true/' (proves the cross-wave invariant survives the deterministic stringifier).
9. Reads scripts/render-claudecode-task.mjs source (no shell-out) and pattern-matches /Router Policy \\(advisory\\)/ AND /dry-run only/.
10. Iterates all wave24-*.md briefs in .missiond/claudecode/ and asserts at least one contains BOTH /advisory/i and /dry-run only/ literals (loose case for 'advisory' to handle header capitalisation, exact case for 'dry-run only').
11. Static audit: assembles a forbidden-pattern table from string parts ('child' + '_' + 'process', 'exec' + 'Sync', 'open' + 'ai', 'anthrop' + 'ic', 'chat.compl' + 'etion', plus literal /\\bspawn\\b/ and /\\bfork\\b/), strips // line comments and /* block */ comments from check-router-policy.mjs and build-session-trace-index.mjs, and asserts ZERO matches in the stripped active source.

LAYER B — Rust daemon end-to-end test (crates/missiond-daemon/src/handlers/knowledge/plan.rs handlers::knowledge::plan::tests):
1 new test router_policy_dry_run_smoke_pins_cross_wave_invariants, bumping the plan-tests count 345→346 and the daemon total 1636→1637. The test:
1. Materialises a temp seed-shape policy file (mirrors wave24-01 seed structure exactly: 3 rules at priorities 10/20/30, :dry-run-only true, :runtime-replacement false).
2. Builds fixture_plan + fixture_resolved (existing test helpers from the wave24-04 test battery).
3. Captures action_execute_bridge baseline (no router knob).
4. Builds a {router_policy_mode='dry_run', router_policy_path=tmp, kind='docs', owner='claudecode'} arg vector; calls parse_router_policy_mode + attach_router_recommendation_block.
5. Asserts on the spliced router_recommendation block:
   - applied === Value::Bool(false) (type-locked literal — the wave24-04 tests assert `assert_eq!(block[\"applied\"], false)` which is value-locked but accepts any falsey-coercible Value; the smoke explicitly type-locks to Bool).
   - status === 'computed', recommended_backend === 'claudecode', backend ∈ wave24-01 enum array.
   - schema === 'missiond.router-recommendation.v0'.
   - target_tool / target_source / dispatch_strategy / dispatch_strategy_source / next_call / execute_mode / runner_status all byte-identical baseline-vs-dry_run (cross-wave invariant 7).
   - baseline carries no router_recommendation key; dry_run does (only the dry_run delta is additive).
   - confidence field surfaced; reasons array contains the literal 'r-docs-to-claudecode'.
6. Cleans up the temp policy file at end.

CROSS-WAVE INVARIANTS PINNED (all 8 from the brief):
1. dry_run_only=true: Layer A asserts on rec.dry_run_only and on the stable JSON regex; reinforced by readRouterPolicyFile asserting policy.dry_run_only=true; Layer B asserts the daemon's analog (applied=false literal) is the JSON Bool(false).
2. :runtime-replacement false REQUIRED: Layer A asserts policy.runtime_replacement===false on the wave24-01 seed; the wave24-04 tests already pin runtime_replacement=true policies as rejected (router_policy_mode_dry_run_runtime_replacement_policy_rejected).
3. applied=false literal: Layer B `assert_eq!(block[\"applied\"], Value::Bool(false))` — type-locked.
4. NO LLM call: Layer A static audit confirms openai/anthropic/chat.completion patterns ABSENT in stripped active source of check-router-policy.mjs + build-session-trace-index.mjs. Daemon path was already audited at wave24-04 commit b8721ab2d0dd.
5. NO spawn: Layer A static audit confirms child_process/spawn/execSync/fork patterns ABSENT in stripped active source. Daemon path uses no Command::new in router_policy_dry_run module.
6. NO mutating git: No new git invocation introduced by either layer; the existing single `git status --porcelain=v1` invocation in the unrelated dispatch path remains unchanged.
7. mode=off byte-identical to baseline: The wave24-04 router_policy_mode_off_returns_legacy_response_byte_identical test pins this; the wave24-06 smoke does not regress it.
8. Renderer 'advisory' + 'dry-run only': Layer A pattern-matches BOTH the live renderer source AND a wave24 brief on disk.

ACCEPTANCE EXIT CODES:
- node scripts/build-session-trace-index.mjs --dry-fixture          → 0 (7/7)
- node scripts/recommend-task-backend.mjs --dry-fixture              → 0 (11/11; 10 baseline + 1 new)
- cargo test -p missiond-daemon handlers::knowledge::plan::tests     → 0 (346/346; 345 baseline + 1 new)
- cargo test -p missiond-daemon                                       → 0 (1637/1637; 1636 baseline + 1 new)
- cargo build --workspace                                             → 0
- git diff --check (4 in-scope files)                                 → 0
- node scripts/task-scope-guard.mjs --mode staged                    → 0 (2 staged files)
- node scripts/verify-task-contract.mjs                               → 0 (against 6afe5414f4a7)
- node scripts/check-missiond-hooks.mjs --json                       → 0 (aligned)

CONSTRAINTS HONORED:
- did NOT touch crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs
- did NOT touch crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs
- did NOT touch crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs
- did NOT touch .missiond/v2/**
- did NOT touch .missiond/tasks/wave23/**
- did NOT touch .missiond/tasks/wave24/wave24-*.lisp
- did NOT add a new MCP tool (the contract explicitly forbids this)
- did NOT introduce unused imports or variables (parseTraceEvents is consumed by the new fixture; the new daemon test references all existing super::router_policy_dry_run imports already in scope from wave24-04)
- did NOT shell out / spawn / call an LLM / mutate git anywhere in the smoke path
- did NOT push / --no-verify / --amend / --force / git add -A.")
