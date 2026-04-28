;; Wave 26 task report.
;; Schema: missiond.report-contract.v1

(report wave26-06-router-readiness-smoke-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave26-06-router-readiness-smoke-v1"
  :status done
  :commit_hash "4bfa710e4489170d0258691ebf1297c8c0bceebf"
  :files_changed
    ["scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"
     "scripts/check-task-report.mjs"
     "scripts/render-claudecode-task.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
  :acceptance_results
    [(:command "node scripts/recommend-task-backend.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "20/20 cases / 20 categories OK (wave25 baseline 17 + 3 wave26-06 smoke fixtures: readiness-eligible-smoke, readiness-current-default-blocked-smoke, readiness-static-audit). Static audit scans recommend + evaluate + check-task-report + render scripts for forbidden patterns (assembled from string parts to avoid self-trip).")
     (:command "node scripts/evaluate-router-policy-corpus.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "12/12 cases / 12 categories OK (wave25/wave26-02 baseline 11 + 1 wave26-06 corpus-aggregates-readiness-smoke pinning apply_eligible_count=0 for seed-shape current-default registry under high-confidence trace).")
     (:command "node scripts/check-task-report.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "31/31 fixtures OK (wave26-04 baseline 30 + 1 wave26-06 positive control: wave26-06-readiness-all-fields-claudecode-smoke validates the canonical wave26-01 seed-shape report with claudecode current-default + apply_eligible=false + canonical 'apply gate requires runtime-ready; current-default is NOT sufficient' blocker text).")
     (:command "node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave26/wave26-02-router-recommendation-readiness-v1.lisp > /tmp/wave26-router-smoke.md"
      :exit_code 0
      :ok true
      :notes "Render of wave26-02.lisp -> /tmp/wave26-router-smoke.md succeeds. rg literal grep on the rendered output: 20 hits across the 5 required patterns (advisory, dry-run only, router-backend-registry, --backend-registry, MUST NOT switch backend). Layer D smoke (--dry-fixture) renders a synthetic in-memory task with both router fields and asserts the same 5 literals plus both router file paths verbatim plus the recommend command WITH --backend-registry: 2/2 fixtures OK.")
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "370/370 OK (wave26-03 baseline 368 + 2 new wave26-06 daemon tests: router_policy_dry_run_smoke_pins_wave26_invariants exercises BOTH polarities back-to-back plus Off+both-router-args invariant 7 plus dispatch-byte-identical re-pin plus self-audit on plan.rs; router_policy_cli_rust_parity_for_readiness pins CLI/Rust agreement on backend_readiness_status='current-default' + backend_runtime_allowed=true + router_apply_eligible=false for seed-shape registry + (8,8)-event trace + docs->claudecode rule).")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1661/1661 OK (wave26-03 baseline 1659 + 2 new wave26-06 daemon tests). Zero regressions across the full daemon suite.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Workspace builds cleanly. Pre-existing warnings unchanged (sqlx-postgres 0.8.0 future-incompat note + 86 missiond-daemon warnings from prior waves; no new warnings introduced by wave26-06).")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "83 task contracts validate OK. wave26-06 contract unchanged (must-not-touch list explicitly forbids editing wave26-* lisp files).")
     (:command "git diff --check -- scripts/recommend-task-backend.mjs scripts/evaluate-router-policy-corpus.mjs scripts/check-task-report.mjs scripts/render-claudecode-task.mjs crates/missiond-daemon/src/handlers/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean on all 5 write-scope files (no whitespace errors, no merge conflict markers).")]
  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["dispatched as the wave26-06 task contract :owner = claudecode and :dispatch-strategy = fresh-code-alignment"
     "router policy is consulted for telemetry only — runtime dispatch is unchanged for this task"]
  :router_trace_index_path ".missiond/v2/observability/trace-index.json"
  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["backend claudecode readiness_status=current-default (apply gate requires runtime-ready; current-default is NOT sufficient)"]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"
  :major_decisions
    [(:decision "Pinned cross-wave invariant 1+2 (router-policy :runtime-replacement=false / :dry-run-only=true) via daemon status=computed assertion AND Node-side policy projection check (Layer A2 fixture asserts policy.dry_run_only=true + policy.runtime_replacement=false on the parsed seed projection)."
      :rationale "The two invariants are guarded at policy-validation time in BOTH engines; reaching status=computed in the daemon proves the policy was accepted (it would fall back if either invariant were violated). The Node-side projection assertion adds a direct, source-readable pin so a future regression that loosens the validator surfaces in the smoke before a worker can drive runtime replacement."
      :trace_ref "wave26-06-trace-commit-001")
     (:decision "Pinned cross-wave invariant 3 (applied=Bool(false) literal) at THREE surfaces: daemon Layer B asserts Value::Bool(false) on the dry_run + apply_eligible=true positive case proving applied stays false EVEN when the gate opens; recommend Layer A1 asserts the recommend() surface NEVER carries an `applied` field at all (applied lives on mission_plan + report-contract surfaces only); report-checker Layer C wave26-06 positive fixture pins :router_applied false in a real report shape."
      :rationale "Three independent pins ensure a future regression that lets `applied` drift to true (or to the string 'true') is caught at whichever layer is changed first. The recommend-surface check pins the contract that only mission_plan / report-contract own the applied field — recommend() is purely advisory."
      :trace_ref "wave26-06-trace-commit-001")
     (:decision "Pinned cross-wave invariant 4 (router_apply_eligible=true ONLY for runtime-ready) in BOTH polarities across 3 layers: Node Layer A1 has explicit eligible-smoke (true) + current-default-blocked-smoke (false) cases; Node Layer A2 has corpus-aggregates-readiness-smoke (apply_eligible_count=0 for seed-shape) — complements the wave26-02 fixture that uses synthetic runtime-ready (apply_eligible_count=1); daemon Layer B router_policy_dry_run_smoke_pins_wave26_invariants exercises BOTH polarities back-to-back in a single test (seed-shape -> Bool(false), runtime-ready -> Bool(true)) so a regression that breaks either polarity fails loudly."
      :rationale "The seed registry has claudecode current-default + runtime_allowed=true + 0 blockers + high confidence — every condition EXCEPT readiness_status passes. This is the trickiest invariant because 'current-default with full permissions' is the most plausible accidental opt-in path. Both polarities pinned in EVERY engine + parity test pin them together so a future seed promotion (current-default -> runtime-ready) requires deliberate coordination across layers."
      :trace_ref "wave26-06-trace-commit-001")
     (:decision "Pinned cross-wave invariant 8 (CLI/Rust parity) via Layer B router_policy_cli_rust_parity_for_readiness + Layer A1 wave26-06-readiness-current-default-blocked-smoke. Both engines drive the SAME shape (docs task + (8,8)-event trace + seed-shape registry) and assert agreement on backend_readiness_status='current-default' + backend_runtime_allowed=true + router_apply_eligible=false."
      :rationale "Inline documentation in both tests names the OTHER engine's expected output. A divergence on either side fails BOTH tests so the parity is bidirectional — neither engine can drift without the other surfacing the regression. Mirrors the wave25-05 parity pattern (router_policy_cli_rust_parity_for_high_confidence_match) extended to the readiness surface."
      :trace_ref "wave26-06-trace-commit-001")
     (:decision "Pinned cross-wave invariant 9 (zero shell/LLM/git/network) via static audits at THREE locations: Node Layer A1 wave26-06-readiness-static-audit scans all 4 router scripts (recommend + evaluate + check-task-report + render); renderer Layer D wave26-06-renderer-static-audit scans the renderer module separately; daemon Layer B audits plan.rs. EVERY forbidden-pattern table is assembled from string parts (e.g. 'child' + '_' + 'process', 'std::' + 'process::' + 'Command') so the audit source itself does not appear as a literal substring."
      :rationale "wave24-06 / wave25-01 / wave25-05 lesson: a self-audit that scans for `child_process` MUST not contain that literal in its own source or it self-trips. Three independent audit locations ensure shell-out / LLM / git / network can't sneak into ANY layer of the readiness path without the smoke catching it."
      :trace_ref "wave26-06-trace-commit-001")
     (:decision "Layer D added a `--dry-fixture` mode to render-claudecode-task.mjs (previously the renderer had no self-test mode) plus a top-level `import os from 'node:os'` for the dry-fixture's mkdtempSync. The fixture writes a synthetic task to OS tmp dir then drives the SAME loadSingleTask + renderTask path production uses, so any regression in the production render surfaces in the smoke."
      :rationale "The brief allowed either `--dry-fixture` or rg-on-acceptance-output; --dry-fixture is the cleaner choice because it keeps the smoke pure-in-process and self-contained (no dependency on the actual wave26-02 task contract being unmodified). The os import was unavoidable for mkdtempSync — alternative (lazy require with createRequire) was tried then rejected as overengineered for one stdlib import."
      :trace_ref "wave26-06-trace-commit-001")]
  :trace_refs
    ["wave26-06-trace-start-001"
     "wave26-06-trace-commit-001"
     "wave26-06-trace-complete-001"]
  :notes "Cross-wave smoke for the wave26 backend readiness loop. All 9 invariants pinned across 5 layers (A1 + A2 + B + C + D). Test count deltas: recommend 17 -> 20 (+3); evaluate 11 -> 12 (+1); check-task-report 30 -> 31 (+1); renderer NEW --dry-fixture 0 -> 2 (+2); daemon 1659 -> 1661 (+2). CLI/Rust parity proven on the seed-shape current-default registry: Node Layer A1 wave26-06-readiness-current-default-blocked-smoke and daemon router_policy_cli_rust_parity_for_readiness BOTH assert the same backend_readiness_status='current-default' + backend_runtime_allowed=true + router_apply_eligible=false for the same shape (docs task + (8,8)-event trace + seed-shape registry + docs->claudecode rule). Audit (cross-wave invariant 9): zero new spawn/child_process/exec/fork/openai/anthropic/fetch/https/git-mutation calls in any modified file — every static-audit fixture asserts this on its target file with patterns assembled from string parts to avoid self-trip (wave24-06 / wave25-01 / wave25-05 lesson)."
)
