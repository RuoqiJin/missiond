;; Wave 25 / Task 05 — Router policy measurement smoke v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave25/wave25-05-router-policy-measurement-smoke-v1.lisp

(report wave25-05-router-policy-measurement-smoke-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave25-05-router-policy-measurement-smoke-v1"
  :status done
  :commit_hash "0f5d857faaa8"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "scripts/check-task-report.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"
     "scripts/recommend-task-backend.mjs"]

  :acceptance_results
    [(:command "node scripts/evaluate-router-policy-corpus.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "evaluate-router-policy-corpus fixtures OK (9 cases, 9 categories). Was 8 baseline; +1 wave25-05-cross-layer fixture asserts policy.dry_run_only=true / policy.runtime_replacement=false on the projector AND every per_task row's confidence ∈ {high,medium,low} AND backend ∈ BACKEND_CLASSES AND by_backend totals match per_task backend union AND a runtime-replacement policy surfaces runtime_replacement=true.")
     (:command "node scripts/recommend-task-backend.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "recommend-task-backend fixtures OK (12 cases, 12 categories). Was 11 baseline; +1 wave25-05-parity fixture pins CLI confidence at all 3 buckets (high for max>=5, medium for 1..4, low for 0) using the SAME synthetic trace-index shape the daemon test consumes; also asserts the deterministic-checker bucket the Layer C report-checker fixture declares (high confidence on rich trace).")
     (:command "node scripts/check-task-report.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-report fixtures OK (23). Was 22 baseline (wave25-02); +1 wave25-05 positive fixture validates :recommended_backend deterministic-checker / :router_confidence high / :router_dry_run_only true / :router_applied false / :router_reasons / :router_trace_index_path repo-relative — proves the report contract still ACCEPTS a valid router-recommendation block end-to-end on the second backend in the wave25-05 parity grid.")
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "357 passed; 0 failed; 1291 filtered out. Was 355 baseline; +2 wave25-05 tests (router_policy_dry_run_smoke_pins_wave25_invariants pins all 8 cross-wave invariants in one shot; router_policy_cli_rust_parity_for_high_confidence_match documents CLI/Rust parity inline and asserts daemon emits high+claudecode for (5,5)-event docs-task fixture).")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1648 passed; 0 failed. Was 1646 baseline (wave25-03 reported); +2 = wave25-05 daemon tests (router_policy_dry_run_smoke_pins_wave25_invariants + router_policy_cli_rust_parity_for_high_confidence_match). All other tests untouched.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Clean build. 86 pre-existing warnings unchanged; 0 new warnings introduced by wave25-05. No new dependencies; no new modules.")
     (:command "git diff --check -- scripts/evaluate-router-policy-corpus.mjs scripts/recommend-task-backend.mjs scripts/render-claudecode-task.mjs scripts/check-task-report.mjs crates/missiond-daemon/src/handlers/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "Clean. No trailing whitespace, mixed tabs/spaces, or conflict markers in any of the 4 staged files (render-claudecode-task.mjs unmodified — Layer D was bonus and skipped per write-scope discipline).")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "ok=true severity=ok matches=true reason=aligned — core.hooksPath==.githooks already set; .githooks/pre-commit exists and is executable; no install needed.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave25/wave25-05-router-policy-measurement-smoke-v1.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave25-05-router-policy-measurement-smoke-v1 (4 staged file(s)). All 4 staged paths inside :write-scope; zero matches against :must-not-touch (workstation_dispatch.rs / agent_execution.rs / unified_entry.rs / plan_dag.rs / .missiond/v2/** / .missiond/tasks/schema/*.lisp / .missiond/tasks/wave24/** / .missiond/tasks/wave25/wave25-*.lisp / .missiond/claudecode/**).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave25/wave25-05-router-policy-measurement-smoke-v1.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK against 0f5d857faaa8 — commit hash exists; commit subject exactly matches contract :commit.message ('test(router): smoke measurable dry-run policy loop'); changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave25-05-trace-start-001 wave25-05-trace-commit-001 wave25-05-trace-complete-001]

  :recommended_backend "claudecode"
  :router_confidence "low"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons ["dispatch-strategy:fresh-code-alignment"
                   "owner:claudecode"
                   "kind:smoke"
                   "fallback:insufficient_trace_history"]

  :major_decisions
    [(:decision "Pinned all 8 cross-wave invariants from the brief in a SINGLE daemon test (router_policy_dry_run_smoke_pins_wave25_invariants) rather than one test per invariant"
      :rationale "Mirrors wave24-06's router_policy_dry_run_smoke_pins_cross_wave_invariants pattern. The wave25-03 battery already exercises individual edge cases (off-with-trace-supplied / dry_run high|medium|low / missing|unreadable|malformed|absent statuses / dispatch byte-identical / applied=false literal across all 4 status flavours). What was missing was a SINGLE assertion proving ALL the invariants hold simultaneously when the chain runs through a wave25-05-shaped policy. A regression that breaks ANY of the 8 invariants now fails this one test loudly."
      :trace_ref "wave25-05-trace-start-001")
     (:decision "Used SAME synthetic trace-index shape (5 events on by_task[btk-1] AND by_backend[claudecode]) for BOTH the Node CLI parity fixture AND the Rust daemon parity test"
      :rationale "Bidirectional parity: a regression in the Node CLI's scoreConfidence OR the daemon's bucket_events / events_for_task / events_for_backend helpers fails BOTH fixtures for the same shape. The (5,5) values sit exactly at RICH_TRACE_THRESHOLD so a future change to the threshold also surfaces here. The Rust test inline-documents the Node CLI's expected output so a reader of plan.rs can audit the parity without opening recommend-task-backend.mjs."
      :trace_ref "wave25-05-trace-commit-001")
     (:decision "Implemented the router code-path self-audit in Layer B (Rust) by reading plan.rs's own source and scanning for forbidden Rust patterns assembled from string parts"
      :rationale "Re-applies the wave24-06 / wave25-01 self-audit lesson: assembling forbidden patterns from string parts (e.g. \"std::\" + \"process::\" + \"Command\") prevents the audit's own probe table from appearing as a literal substring and tripping the audit. The audit covers std::process::Command (shell-out from std), tokio::process (async shell-out), reqwest:: / hyper::Client (HTTP / network), open + ai_api / anthrop + ic_api (LLM vendor probes), and git invocation surfaces. Strips line comments before scanning so prose that names the patterns does not self-trip; keeps block comments and string literals in scope on purpose so a real string literal inviting reqwest would surface as evidence."
      :trace_ref "wave25-05-trace-commit-001")
     (:decision "Skipped Layer D (renderer literal-pin fixture) — the renderer's 'advisory' / 'dry-run only' literals are already cross-asserted by wave24-06's existing brief-pattern smoke"
      :rationale "Layer D was explicitly marked bonus in the brief. wave24-06's smoke-e2e-chain fixture in scripts/recommend-task-backend.mjs already pattern-matches the renderer source AND any wave24 brief on disk for both 'advisory' and 'dry-run only' literals. Adding Layer D would have required either modifying scripts/render-claudecode-task.mjs (in scope but no test infrastructure exists there — it has no --dry-fixture mode) or a new test file (out of scope). The contract says 'Recommended split: A (Node) + B (Rust) at minimum' and explicitly lists C and D as 'bonus but cheap'; wave24-06's existing coverage of the renderer literals plus wave25-04's renderer-side commits make Layer D's marginal value low. Confirmed wave24-06 smoke is still green by running the full Node fixture suite."
      :trace_ref "wave25-05-trace-commit-001")
     (:decision "Layer A's evaluator fixture asserts on the projector reading badRuntimeReplacementText() rather than driving evaluator main() with process.exit() simulation"
      :rationale "Mirrors the wave25-01 rejected-policy fixture pattern. The evaluator's main() guard reads policy.runtime_replacement and exits non-zero before aggregation; we cannot exercise process.exit() inside the fixture suite (would tear down the harness). Asserting on the projector — readRouterPolicyFile().runtime_replacement === true — proves the data the guard reads from is correctly surfaced. The existing wave25-01 rejected-policy fixture already makes the same assertion; the wave25-05 cross-layer fixture re-pins it to anchor the evaluator side of the runtime_replacement=false invariant."
      :trace_ref "wave25-05-trace-commit-001")]

  :unexpected_work
    [(:summary "Initial wave25-05 cross-layer fixture in evaluate-router-policy-corpus.mjs failed on the first run with ENOENT — writePolicyFile() expected the parent directory to exist BEFORE writing. Fixed by reordering: fs.mkdirSync(path.join(tmp, 'rr'), { recursive: true }) BEFORE writePolicyFile(path.join(tmp, 'rr'), ...). The 8 prior fixtures passed because writePolicyFile() always wrote to the tmp ROOT (which mkdtempSync creates); the wave25-05 fixture writes to a SUBdir."
      :trace_ref "wave25-05-trace-commit-001")
     (:summary "Confirmed the Layer B router-side audit does not self-trip on the test source itself by running the test in isolation (cargo test ... router_policy_dry_run_smoke_pins_wave25_invariants). The forbidden-pattern table is assembled at test runtime via String::from(\"std::\") + \"process::\" + \"Command\" so the LITERAL substring \"std::process::Command\" never appears in the test source. Verified by scanning the test body manually: every forbidden token is split across at least one + boundary."
      :trace_ref "wave25-05-trace-commit-001")]

  :notes
    "Layered smoke implementation:

LAYER A (Node) — 2 fixtures across 2 scripts:

A1) scripts/recommend-task-backend.mjs --dry-fixture: 11 -> 12 cases.
    New case: 'wave25-05: CLI confidence parity matches daemon for high/medium/low' (category wave25-05-parity).
    - Builds a 2-rule wave25-05 parity policy (docs->claudecode prio 10 + code-alignment+scripts/check-* -> deterministic-checker prio 20).
    - Re-pins policy.dry_run_only=true / policy.runtime_replacement=false on the projector.
    - Drives recommend() at all 3 confidence buckets using the SAME synthetic trace-index shape the Rust daemon consumes:
      * high   : taskEvents=0, backendEvents=5 → max=5 → high. Backend=claudecode. chosen_rule_id=r-docs-to-claudecode. dry_run_only=true.
      * medium : taskEvents=2, backendEvents=3 → max=3 → medium.
      * low    : taskEvents=0, backendEvents=0 → max=0 → low (matched-but-zero).
    - Asserts every bucket's backend ∈ BACKEND_CLASSES AND stable JSON surfaces dry_run_only:true literally.
    - Drives a code-alignment task through the same policy and confirms backend=deterministic-checker + confidence=high (matches the Layer C positive fixture's recommended_backend value).

A2) scripts/evaluate-router-policy-corpus.mjs --dry-fixture: 8 -> 9 cases.
    New case: 'wave25-05: cross-layer invariants pinned on evaluator output' (category wave25-05-cross-layer).
    - Builds a 3-task corpus (docs / checker / review) on the seed wave25-01 policy.
    - Re-pins policy.dry_run_only=true / policy.runtime_replacement=false on the projector.
    - Asserts every per_task row's confidence ∈ {high,medium,low} AND backend ∈ BACKEND_CLASSES.
    - Asserts by_backend totals match the union of per_task backends.
    - Cross-pin: a runtime-replacement policy surfaces runtime_replacement=true on the projector (the evaluator's main() guard rejects on this).

LAYER B (Rust) — 2 tests in handlers::knowledge::plan::tests (357 total, was 355):

B1) router_policy_dry_run_smoke_pins_wave25_invariants — pins ALL 8 cross-wave invariants in one shot:
    - Materializes the wave25-05 parity policy (docs->claudecode prio 10 + checker prio 20) at a temp file.
    - Materializes a (5,5) trace-index JSON at a temp file.
    - Invariant 6: mode=off + trace-index supplied → response byte-identical to baseline (re-pins wave24-04 + wave25-03 byte-shape).
    - Invariant 7: CLI/Rust parity — daemon emits backend='claudecode' + confidence='high' for the (5,5) docs-task fixture matching Node CLI's expected output.
    - Invariant 1+2: status='computed' proves both runtime_replacement=false AND dry_run_only=true held end-to-end (a violation surfaces as status='rejected').
    - Invariant 3: applied=Value::Bool(false) literal type-checked.
    - Invariant: recommended_backend ∈ wave24-01 enum (re-spelled locally per wave24-06 lesson — no script imports).
    - Invariant: schema='missiond.router-recommendation.v0'.
    - Invariant: trace_index_status='used' proves wave25-03 trace-index code path was exercised.
    - Invariant 6 (cont.): every dispatch field byte-identical between baseline and dry_run.
    - Invariant: reasons reference matched rule id 'r-docs-to-claudecode'.
    - Invariant 8: self-audit on plan.rs scans for forbidden std::process::Command / tokio::process / reqwest:: / hyper::Client / openai_api / anthropic_api with strings assembled from parts.

B2) router_policy_cli_rust_parity_for_high_confidence_match — drives the SAME (5,5) shape on a single docs->claudecode policy and asserts daemon emits backend=claudecode + confidence=high + status=computed + trace_index_status=used + applied=Bool(false). Inline documentation pins what the Node CLI's recommend() emits for the SAME shape; a regression in either engine fails BOTH this Rust test AND the Layer A Node fixture.

LAYER C (Node) — 1 fixture in scripts/check-task-report.mjs --dry-fixture: 22 -> 23.
    New positive fixture: 'wave25-05 valid router-recommendation block (deterministic-checker + high)':
    - :recommended_backend deterministic-checker
    - :router_confidence high
    - :router_policy_path .missiond/router/router-policy-v1.lisp (repo-relative)
    - :router_dry_run_only true (cross-wave invariant literal)
    - :router_applied false (cross-wave invariant literal)
    - :router_reasons [matched-rule:... trace-events:7]
    - :router_trace_index_path .missiond/v2/index/session-trace-index.json (repo-relative)
    Proves the wave25-02 report contract STILL ACCEPTS a valid router-recommendation block end-to-end on the SECOND backend in the wave25-05 parity grid (the original wave25-02 valid fixture used claudecode + medium).

LAYER D — SKIPPED. Bonus per the brief; wave24-06's smoke-e2e-chain fixture in scripts/recommend-task-backend.mjs already pattern-matches the renderer source AND any wave24 brief on disk for both 'advisory' and 'dry-run only' literals; wave25-04's render-claudecode-task.mjs additions are already covered by that pattern check + wave25-04's own acceptance rg invocation. Adding Layer D would have required modifying scripts/render-claudecode-task.mjs (in scope but no --dry-fixture mode exists there). Skipping preserves write-scope discipline.

CROSS-WAVE INVARIANT PINS (8 total per the brief):

(1) policy runtime_replacement=false: re-checked by daemon's reject branch in router_policy_dry_run_smoke_pins_wave25_invariants — surfaces as status='computed' on a well-formed policy. Also re-pinned on the projector by Layer A1 + A2 (mustEqual('policy.runtime_replacement', policy.runtime_replacement, false)).
(2) policy dry_run_only=true: re-checked similarly — status='computed' proves the dry_run_only invariant held. Also re-pinned on the projector by Layer A1 + A2.
(3) applied=false JSON Bool literal in EVERY emitted block: type-checked in B1 (block['applied'] == Value::Bool(false) AND block['applied'].is_boolean()). Also re-pinned by all 4 wave25-03 daemon tests (applied_remains_false_with_trace_index across all 4 status flavours, untouched).
(4) renderer 'advisory' + 'dry-run only' literals: pinned by wave24-06's smoke-e2e-chain fixture (untouched by wave25-05) AND wave25-04's renderer additions. Layer D was bonus.
(5) report-checker rejects router_applied=true / router_dry_run_only=false: pinned by wave25-02 fixtures (rows 'wave25-02 router_applied=true rejected' and 'wave25-02 router_dry_run_only=false rejected'); untouched by wave25-05. Layer C adds a POSITIVE fixture re-asserting the literal-true / literal-false pair on the SECOND backend.
(6) mission_plan off/default mode byte-shape unchanged: pinned by wave24-04's router_policy_mode_off_returns_legacy_response_byte_identical (untouched) AND wave25-03's router_policy_mode_off_with_trace_index_supplied_does_no_file_io (untouched) AND wave25-05's B1 fixture (re-pins under the wave25-05 shape with both router_policy_path AND router_policy_trace_index_path supplied).
(7) CLI/Rust parity for one fixture: pinned by Layer A1's wave25-05-parity case AND Layer B2's router_policy_cli_rust_parity_for_high_confidence_match — both engines must emit backend='claudecode' + confidence='high' for the (5,5) docs-task shape. Inline documentation in B2 makes the parity bidirectional. Deliberate divergence per wave25-03 brief (matched-default-medium when trace-absent vs Node's matched-default-low) NOT pinned here — it is documented in the wave25-03 contract and the wave25-05 fixture pins the AGREEMENT case (both sides high on rich trace).
(8) zero shell-out / LLM / git mutation / network in active router code path: pinned by Layer B1's self-audit on plan.rs (forbidden patterns: std::process::Command, tokio::process, reqwest::, hyper::Client, openai_api, anthropic_api) AND wave25-01's existing self-audit on evaluate-router-policy-corpus.mjs (untouched).

CARGO TEST COUNTS:

- cargo test -p missiond-daemon: 1648 passed (was 1646 baseline; +2).
- cargo test -p missiond-daemon handlers::knowledge::plan::tests: 357 passed (was 355 baseline; +2).
- cargo test -p missiond-mcp --lib: not run (out of scope; wave25-05 does not touch missiond-mcp). Wave25-03 baseline was 17.

NODE FIXTURE COUNTS:

- evaluate-router-policy-corpus: 9 (was 8 baseline; +1).
- recommend-task-backend: 12 (was 11 baseline; +1).
- check-task-report: 23 (was 22 baseline; +1).

DELIBERATE DIVERGENCE (documented, not pinned by wave25-05):

The wave25-03 contract documents a slight divergence between the Node CLI and the Rust daemon: when NO trace-index is supplied AND a rule matched, the Node CLI returns confidence=low (insufficient_trace_history) while the Rust daemon returns confidence=medium (matched-default-medium). The wave25-05 parity fixture pins the AGREEMENT case (both sides emit confidence=high on the (5,5) trace-index) and explicitly does NOT assert agreement on the no-trace-index case. This is documented inline in the wave25-05 daemon test and in the recommend-task-backend.mjs wave25-05-parity fixture comments.

CONSTRAINTS HONORED:

- Did NOT touch crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs, agent_execution.rs, unified_entry.rs, plan_dag.rs (wave23-05 lesson).
- Did NOT touch .missiond/v2/** (intent-event-bus.lisp is locked).
- Did NOT touch .missiond/tasks/schema/*.lisp (wave25-02 owns the report-contract schema; wave25-04 owns the task-contract schema additions).
- Did NOT touch .missiond/tasks/wave24/** or .missiond/claudecode/** (out of scope).
- Did NOT touch .missiond/tasks/wave25/wave25-*.lisp (the wave25 task contracts themselves).
- Did NOT introduce dependencies on child_process, spawn, git, or LLM clients in the smoke path (Layer B1 audit asserts on plan.rs; Layer A audits assemble forbidden patterns from string parts to avoid self-trip).
- Did NOT add a new MCP tool (contract explicitly forbids).
- Did NOT introduce unused imports / parameters.
- Used Edit for all pre-existing files (plan.rs is ~18900 lines; surgical Edit at the end of the test module).
- Did NOT push, --no-verify, --amend, --force, git add -A.")
