;; Wave 28 / Task 06 — Task runner loop smoke v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave28/wave28-06-task-runner-loop-smoke-v0.lisp

(report wave28-06-task-runner-loop-smoke-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave28-06-task-runner-loop-smoke-v0"
  :status done
  :commit_hash "47177130da69"
  :files_changed
    ["scripts/check-task-runner-manifest.mjs"
     "scripts/plan-task-runner.mjs"
     "scripts/render-wave-briefs.mjs"
     "scripts/verify-task-runner-batch.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :acceptance_results
    [(:command "node scripts/check-task-runner-manifest.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "Layer A — wave28-01 checker: 22 cases / 16 categories PASS (baseline 20/15 + 2 wave28-06 fixtures: wave28-06-loop-smoke-productive-only-pinned + wave28-06-loop-smoke-archive-substring-rejected). Pins invariant 1 (same productive-only manifest validates clean) + invariant 2 (defence-in-depth archive substring rejection at the schema layer) at the wave28-01 boundary.")
     (:command "node scripts/plan-task-runner.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "Layer B — wave28-02 plan CLI: 13 fixtures PASS (baseline 12 + 1 wave28-06 fixture: wave28-06-loop-smoke-plan-determinism — category wave28-06-loop-smoke). Pins invariant 3 (verification_tier_counts.full=1 only on final smoke; local x 2; smoke=0) + invariant 6 (byte-identical determinism on re-plan) + invariant 2 (productive_only echoed) at the plan-CLI boundary.")
     (:command "node scripts/render-wave-briefs.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "Layer C — wave28-03 batch renderer: 11 cases / 10 categories PASS (baseline 9/9 + 2 wave28-06 fixtures: wave28-06-loop-smoke-renderer-heartbeat-surfaced + wave28-06-loop-smoke-renderer-pseudo-skipped — category wave28-06-loop-smoke). Pins invariant 4 (heartbeat_minutes value `10` literal echoed in thin brief; preamble OR brief carries the boilerplate guidance) + invariant 2 (productive-only emit; rendered brief ids contain no archive/backfill/index/lisp-backfill substring) at the renderer boundary.")
     (:command "node scripts/verify-task-runner-batch.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "Layer D — wave28-05 batch verifier: 9 fixtures PASS (baseline 8 + 1 wave28-06 fixture: wave28-06-loop-smoke-batch-aggregate-aligns-with-plan). Pins invariant 2 (4-node manifest with 2 productive + 2 pseudo nodes — id-substring AND :kind=backfill — yields verified_nodes=2 / total_nodes=2 / skipped_nodes=[wave99-50-helper, wave99-99-archive-prior-task-docs]; productive accounting agrees with what the wave28-02 plan CLI would batch from the same manifest).")
     (:command "cargo test -p missiond-daemon --bin missiond task_runner"
      :exit_code 0
      :ok true
      :notes "Layer E — wave28-04 daemon dry-run surface: 14 task_runner tests PASS (baseline 13 wave28-04 tests + 1 new wave28-06 test: task_runner_loop_smoke_pins_wave28_invariants). Single exhaustive Rust smoke pinning invariants 1 / 2 / 3 / 5 / 6 + the wave27-06 self-audit grep on plan.rs active source for forbidden patterns (assembled from string fragments to avoid self-trip per wave24-06 / wave25-05 / wave26-06 / wave27-06 lesson).")
     (:command "cargo test -p missiond-daemon --bin missiond"
      :exit_code 0
      :ok true
      :notes "Full daemon test suite: 1681 passed; 0 failed (baseline 1680 from wave28-04 + 1 new wave28-06 task_runner_loop_smoke_pins_wave28_invariants). No regressions in any pre-existing surface (router_recommendation / router_dispatch_descriptor / inference / apply-gate / DAG / sysinfra / store / workers).")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "MCP lib tests: 17 passed (baseline preserved). wave28-06 added zero new MCP args; the existing wave28-04 task_runner_mode + task_runner_manifest_path properties remain the only wave28 additions.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Workspace build clean. Pre-existing warnings (dead-code, sqlx-postgres future-incompat) — all from prior code. No new warnings introduced by wave28-06.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (98 tasks).")
     (:command "git diff --check -- scripts/check-task-runner-manifest.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs scripts/verify-task-runner-batch.mjs crates/missiond-daemon/src/handlers/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "No whitespace errors on any of the 5 edited files.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; pre-commit hook present and executable.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-06-task-runner-loop-smoke-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK (5 staged files) — all inside :write-scope; zero matches against :must-not-touch (plan_dag.rs / workstation_dispatch.rs / agent_execution.rs / workflow.rs / directive.rs / crates/missiond-mcp/src/tools/knowledge/plan.rs / .missiond/v2/** / .missiond/tasks/wave27/** / .missiond/tasks/wave28/wave28-*.lisp / .missiond/tasks/wave28/dispatch-plan.lisp / .missiond/claudecode/** / scripts/check-task-contract.mjs / scripts/render-claudecode-task.mjs / scripts/verify-task-run.mjs).")]

  :scope_deviations []

  :trace_refs [wave28-06-trace-start-001]

  :major_decisions
    [(:decision "Add a small number (1-2) of wave28-06-named fixtures to each of the 4 Node scripts AND one exhaustive Rust smoke test in handlers::knowledge::plan::tests, rather than a single end-to-end harness."
      :rationale "Cross-layer smoke does not require a single bash-shaped runner; the cross-layer chain is already dispatched-by-test by definition. Adding wave28-06-named fixtures inside each layer's existing dry-fixture suite means: (a) any future regression in any single layer surfaces with the layer's own runner (no need to run the smoke separately to find it); (b) the wave28-06 fixture sits next to the wave28-01..05 fixtures so a reviewer can trace the cross-layer pin to its layer-local context; (c) the existing :write-scope (5 files) is the smallest possible surface — no new scripts/runner files. The exhaustive Rust smoke test mirrors wave27-06's router_dispatch_descriptor_smoke_pins_wave27_invariants pattern.")
     (:decision "Pin productive-only invariant (invariant 2) at THREE layers (schema checker, renderer, batch verifier) instead of one."
      :rationale "Defence-in-depth: the wave28-01 schema layer rejects archive/backfill ids when productive_only=true; the wave28-03 renderer rejects them again at brief-emit time; the wave28-05 batch verifier silently skips them at accounting time. A single-layer pin would not catch a regression where (e.g.) the renderer's defence weakens but the schema checker still catches it. Three independent fixtures (wave28-06-loop-smoke-archive-substring-rejected at schema layer; wave28-06-loop-smoke-renderer-pseudo-skipped at renderer layer; wave28-06-loop-smoke-batch-aggregate-aligns-with-plan with mixed pseudo nodes at verifier layer) make the cross-layer pin explicit.")
     (:decision "Wave28-06 Rust smoke critical_path assertion = 100, not 75."
      :rationale "Initial draft assumed alpha (30) + final-smoke (45) = 75 was the longest chain. But the DAG is alpha -> beta -> final-smoke (30+25+45=100) AND alpha -> final-smoke directly (30+45=75). The longest_from memoised DFS in compute_runner_block correctly returns 100 (alpha + max(beta=70, final-smoke=45) = 100). Initial test failure caught this — the assertion now reflects the actual DAG semantics rather than a simplified linear chain.")
     (:decision "The Rust smoke self-audit assembles forbidden-pattern strings from fragments (`String::from(\"ch\") + \"ild\" + \"_\" + \"process\"`) AND uses variable names that do not contain the literal patterns."
      :rationale "wave24-06 / wave25-05 / wave26-06 / wave27-06 lesson: a literal regex like `child_process` would match this very test body when the audit greps the active source. Splitting tokens at boundaries that do NOT form recognisable substrings (`ch`+`ild` instead of `child`; `sp`+`awn` instead of `spawn`; `to`+`kio` instead of `tokio`; `Co`+`mmand` instead of `Command`; `g`+`it ` instead of `git `) keeps the audit body invisible to the audit itself. The strip_rust_comments_and_strings helper additionally removes line/block comments and string literals before the regex sweep so commentary mentions of the forbidden tokens do not self-trip.")
     (:decision "Use the same synthetic 3-node manifest (alpha + beta + final-smoke; dispatch_groups A / B / C; verification_tier local/local/full) across Layer A / Layer B / Layer E."
      :rationale "Cross-layer smoke is fundamentally about same-input-different-layer agreement. Using identical manifest shape across the layers makes the cross-layer chain explicit AND makes the assertion semantics traceable: when the wave28-02 plan CLI says verification_tier_counts.full=1, the wave28-04 daemon dry-run MUST also say full=1 for the same manifest. The wave28-03 renderer + wave28-05 batch verifier fixtures use related shapes (productive 2-node seed; mixed productive + pseudo 4-node) that mirror the same wave28-06 invariants without exact byte parity (different layers exercise different aspects).")]

  :time_sinks
    [(:label "Re-reading wave28-01 / 02 / 03 / 04 / 05 implementations end-to-end before adding fixtures"
      :notes "Cross-layer smoke required understanding each layer's existing fixture conventions: (a) wave28-01 uses a static fixtures array with name/category/source/ok shape; (b) wave28-02 uses buildFixtures() with name/category/expect/source/assert shape; (c) wave28-03 uses async fixtures.push pattern with run() callback + tmp dir setup/cleanup; (d) wave28-05 uses fixtures.push with manifest/manifestPath/loaders/expect shape backed by syntheticLoaders; (e) wave28-04 uses #[test] fns with write_temp_manifest helpers. Each new fixture follows the layer-local convention to minimise surface drift.")
     (:label "Debugging the critical_path = 100 assertion failure"
      :notes "Initial draft asserted critical_path=75 based on a simplified mental model. Test failure surfaced the actual longest_from semantics (memoised DFS over the dependents map): alpha's longest = 30 + max(beta's longest = 70, final-smoke's longest = 45) = 100. Updated assertion + comment to reflect the actual DAG traversal — this matters because a future regression that breaks the memoised DFS would now panic with the correct expected value.")
     (:label "Walking the forbidden-pattern audit assembly to ensure no self-trip"
      :notes "Wave27-06 already established the pattern; wave28-06's audit re-instances the same 11 patterns with slightly different fragment splits to avoid even the appearance of pattern duplication. Verified by running the full daemon test suite — the wave28-06 audit finishes clean, proving plan.rs active code (post-comment-strip) carries zero matches.")]

  :unexpected_work
    [(:summary "Discovered the wave28-04 baseline computation: critical_path for a diamond DAG (alpha -> beta -> final-smoke + alpha -> final-smoke) is the longest chain (alpha + beta + final-smoke = 100), not the deepest single edge (alpha + final-smoke = 75). The wave28-04 baseline test on a simpler chain (manifest_basic_body — node-a → node-b chain only, plus standalone node-c) returns 75 because there's no diamond. The wave28-06 smoke uses a diamond intentionally to exercise the memoised DFS branch.")
     (:summary "Confirmed that the wave28-03 thin brief INCLUDES the literal `heartbeat_minutes` field name AND the value `10` (per the existing render-claudecode-task.mjs line 350 logic). This let the wave28-06-loop-smoke-renderer-heartbeat-surfaced fixture assert on both — a stronger pin than the brief contract requires (which only asks for `heartbeat metadata propagation into thin brief OR shared preamble guidance`).")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Smoke surface (cross-layer fixtures over Node + Rust files inside scripts/ and crates/missiond-daemon/.../plan.rs) is the canonical claudecode beat — mirrors the wave24-06 / wave25-05 / wave26-06 / wave27-06 pattern."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this task's dispatch handoff would mark a descriptor as apply-eligible (wave27-01 eligibility-gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave28-06 ships the cross-layer smoke pinning the wave28 task-contract runner v0 surface across all 5 layers (wave28-01 manifest checker; wave28-02 plan CLI; wave28-03 batch renderer; wave28-04 daemon dry-run surface; wave28-05 batch verifier). The smoke is composed of 6 wave28-06-named fixtures distributed across the 4 Node scripts AND 1 exhaustive Rust smoke test inside handlers::knowledge::plan::tests.

     Smoke layers added:
       Layer A (scripts/check-task-runner-manifest.mjs): 2 wave28-06-named fixtures
         - wave28-06-loop-smoke-productive-only-pinned (synthetic 3-node manifest validates clean — pins invariant 1 at schema layer)
         - wave28-06-loop-smoke-archive-substring-rejected (defence-in-depth — pins invariant 2 at schema layer)
         Total fixture count: 20 → 22 (categories: 15 → 16).
       Layer B (scripts/plan-task-runner.mjs): 1 wave28-06-named fixture
         - wave28-06-loop-smoke-plan-determinism (asserts verification_tier_counts {full:1, local:2, smoke:0} for the wave28-06 manifest + byte-identical re-plan determinism — pins invariant 3 + 6 at plan-CLI layer)
         Total fixture count: 12 → 13 (categories: 11 → 12).
       Layer C (scripts/render-wave-briefs.mjs): 2 wave28-06-named fixtures
         - wave28-06-loop-smoke-renderer-heartbeat-surfaced (asserts thin brief contains literal `heartbeat_minutes` AND the value `10`; preamble OR brief carries the boilerplate — pins invariant 4 at renderer layer)
         - wave28-06-loop-smoke-renderer-pseudo-skipped (productive 2-node manifest emits exactly 2 briefs; no rendered brief id contains any forbidden substring — pins invariant 2 at renderer layer)
         Total fixture count: 9 → 11 (categories: 9 → 10).
       Layer D (scripts/verify-task-runner-batch.mjs): 1 wave28-06-named fixture
         - wave28-06-loop-smoke-batch-aggregate-aligns-with-plan (4-node manifest with 2 productive + 2 pseudo (id-substring + :kind=backfill); verified_nodes=2; skipped_nodes=[wave99-50-helper, wave99-99-archive-prior-task-docs] — pins invariant 2 at batch-verifier layer; agrees with what the wave28-02 plan CLI would batch from the same manifest)
         Total fixture count: 8 → 9.
       Layer E (crates/missiond-daemon/src/handlers/knowledge/plan.rs): 1 exhaustive Rust smoke test
         - task_runner_loop_smoke_pins_wave28_invariants — pins invariants 1 / 2 / 3 / 5 / 6 in a single test, plus the wave27-06-style self-audit grep on plan.rs active source for forbidden patterns (assembled from string fragments to avoid self-trip).
         Total task_runner test count: 13 → 14; total daemon test count: 1680 → 1681.

     Cross-layer invariants pinned (per the wave28-06 brief):
       1. Same manifest validates clean through all 5 layers — Layer A (wave28-06-loop-smoke-productive-only-pinned) at schema; Layer B (wave28-06-loop-smoke-plan-determinism) at plan CLI; Layer C (wave28-06-loop-smoke-renderer-pseudo-skipped) at renderer; Layer D (wave28-06-loop-smoke-batch-aggregate-aligns-with-plan) at batch verifier; Layer E (task_runner_loop_smoke_pins_wave28_invariants) at daemon dry-run.
       2. Productive-only — Layer A (wave28-06-loop-smoke-archive-substring-rejected — schema-level rejection); Layer C (wave28-06-loop-smoke-renderer-pseudo-skipped — renderer-level emit guard); Layer D (wave28-06-loop-smoke-batch-aggregate-aligns-with-plan — verifier-level skip accounting); Layer E (task_runner_loop_smoke_pins_wave28_invariants — every emitted batch id checked against forbidden substrings).
       3. Verification-tier — Layer B (wave28-06-loop-smoke-plan-determinism asserts {local:2, full:1, smoke:0}); Layer E (task_runner_loop_smoke_pins_wave28_invariants asserts the same counts via daemon dry-run block).
       4. Heartbeat metadata propagation — Layer C (wave28-06-loop-smoke-renderer-heartbeat-surfaced asserts literal `heartbeat_minutes` + value `10` in the thin brief; preamble OR brief carries the boilerplate).
       5. No execution: applied=false + no spawn / no git / no Node / no network — Layer E (task_runner_loop_smoke_pins_wave28_invariants asserts applied=Value::Bool(false) literal AND runs the wave27-06-style self-audit grep on plan.rs active source for child_process / spawn / spawn_blocking / tokio::process / std::process::Command / reqwest / hyper / openai / anthropic / git mutation verbs / git2::Repository::open).
       6. Determinism — Layer B (wave28-06-loop-smoke-plan-determinism asserts byte-identical plan output between two consecutive planFromManifestObject calls); Layer E (task_runner_loop_smoke_pins_wave28_invariants asserts byte-identical task_runner block on re-render via attach_task_runner_block — modulo the manifest_path field which echoes the absolute tmp path).
       7. No LLM / no network — Layer E (task_runner_loop_smoke_pins_wave28_invariants self-audit grep covers reqwest / hyper / openai / anthropic patterns alongside the no-execution patterns).

     CLI/Rust parity proof: same synthetic productive-only manifest (3 productive nodes alpha + beta + final-smoke; dispatch_groups A/B/C; verification_tier local/local/full; estimated_minutes 30/25/45; heartbeat_minutes 10/10/10) drives consistent semantics across all 5 layers — wave28-01 checker accepts, wave28-02 plan CLI emits {local:2, full:1, smoke:0} tier counts, wave28-03 renderer would emit 3 productive briefs (no pseudo-substrings), wave28-05 batch verifier would account 3 productive + 0 skipped, wave28-04 daemon dry-run emits the same productive-only batch list + same tier counts + applied=false + critical_path_minutes=100 (alpha + beta + final-smoke chain).

     Test totals: cargo test -p missiond-daemon --bin missiond → 1681 passed (1680 baseline from wave28-04 + 1 new wave28-06 task_runner_loop_smoke_pins_wave28_invariants); cargo test -p missiond-mcp --lib → 17 passed (baseline preserved). Node fixture totals: check-task-runner-manifest 22 (baseline 20 + 2); plan-task-runner 13 (baseline 12 + 1); render-wave-briefs 11 (baseline 9 + 2); verify-task-runner-batch 9 (baseline 8 + 1).

     Audit confirmation: zero new shell-out / spawn / git mutation / network / LLM call sites in any of the 5 edited files. The Layer E Rust smoke explicitly self-audits plan.rs active source (post comment + string-literal strip) for 11 forbidden-pattern fragments assembled at runtime — confirmed clean.

     Edited files (within :write-scope):
       * scripts/check-task-runner-manifest.mjs — appends 2 wave28-06-named fixtures to the existing static fixtures array (no helper signature changes; no new exports).
       * scripts/plan-task-runner.mjs — appends 1 wave28-06-named fixture to buildFixtures() with an inline assert that re-parses the same manifest source and asserts byte-identical plan output (no helper signature changes; no new exports).
       * scripts/render-wave-briefs.mjs — appends 2 wave28-06-named fixtures (async run pattern matching the existing fixtures.push convention; reuses setupTmpRepo / cleanupTmpRepo / seedTwoNodeManifest / FORBIDDEN_ID_SUBSTRINGS already in scope).
       * scripts/verify-task-runner-batch.mjs — appends 1 wave28-06-named fixture (manifest/manifestPath/loaders/expect pattern matching the existing fixtures.push convention; reuses syntheticLoaders / loadContractFromSourceShim / buildContractSource / buildReportSource / buildLedgerSource / syntheticCommit already in scope).
       * crates/missiond-daemon/src/handlers/knowledge/plan.rs — appends 1 manifest_wave28_06_loop_smoke_body() helper + 1 #[test] task_runner_loop_smoke_pins_wave28_invariants inside the existing tests module (re-uses parse_task_runner_mode / attach_task_runner_block / TaskRunnerMode / write_temp_manifest / strip_rust_comments_and_strings / fixture_plan / fixture_resolved / parse_payload / action_execute_bridge already in scope; no public surface changes; no new module).

     Pre-commit pipeline: dry-fixture suites all PASS (22 / 13 / 11 / 9) → cargo test -p missiond-daemon --bin missiond task_runner (exit=0, 14/14 PASS) → cargo test -p missiond-daemon --bin missiond (exit=0, 1681/1681 PASS) → cargo test -p missiond-mcp --lib (exit=0, 17/17 PASS, baseline preserved) → cargo build --workspace (exit=0) → check-task-contract.mjs --all (exit=0, 98 tasks) → git diff --check (exit=0) → check-missiond-hooks.mjs --json (preflight aligned) → git add (5 paths) → task-scope-guard.mjs --mode staged (OK, 5 staged) → MISSIOND_TASK_CONTRACT=... git commit -m \"test(tasks): smoke task runner loop\" (commit 47177130da69).

     Append-only ledger updates: shared-memory wave28-06-claim-001 (seq 12) before staging; session-trace wave28-06-trace-start-001 (seq 17) before reading background. Final completion entry to be appended after commit verification.

     Constraints honored: NO edits to plan_dag.rs / workstation_dispatch.rs / agent_execution.rs / workflow.rs / directive.rs / crates/missiond-mcp/src/tools/knowledge/plan.rs (must-not-touch list). NO edits to .missiond/v2/** / .missiond/tasks/wave27/** / .missiond/tasks/wave28/wave28-*.lisp / dispatch-plan.lisp / .missiond/claudecode/** / scripts/check-task-contract.mjs / render-claudecode-task.mjs / verify-task-run.mjs. Did not git add . / git push / --no-verify / --amend / --force.")
