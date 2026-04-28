;; Wave 27 task report — wave27-06-router-dispatch-descriptor-smoke-v0.
;; Schema: .missiond/tasks/schema/report-contract-v1.lisp

(report wave27-06-router-dispatch-descriptor-smoke-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave27-06-router-dispatch-descriptor-smoke-v0"
  :status done
  :commit_hash "7f65f0590a1a17252415a2d91cbbd967e3d512e2"
  :files_changed
    ["scripts/check-router-dispatch-descriptor.mjs"
     "scripts/build-router-dispatch-descriptor.mjs"
     "scripts/check-task-report.mjs"
     "scripts/render-claudecode-task.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
  :acceptance_results
    [(:command "node scripts/check-router-dispatch-descriptor.mjs --dry-fixture" :exit_code 0 :ok true :notes "27 cases / 10 categories (baseline 22 / 9; +5 wave27-06-cross-wave-invariant)")
     (:command "node scripts/build-router-dispatch-descriptor.mjs --dry-fixture" :exit_code 0 :ok true :notes "12 cases / 11 categories (baseline 10 / 10; +2 wave27-06-cross-wave-invariant)")
     (:command "node scripts/check-task-report.mjs --dry-fixture" :exit_code 0 :ok true :notes "39 fixtures (baseline 38; +1 wave27-06 positive happy-path)")
     (:command "node scripts/render-claudecode-task.mjs --dry-fixture" :exit_code 0 :ok true :notes "5 cases / 5 categories (baseline 4 / 4; +1 wave27-06-renderer-literals)")
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests" :exit_code 0 :ok true :notes "376 tests pass (baseline 375; +1 router_dispatch_descriptor_smoke_pins_wave27_invariants)")
     (:command "cargo test -p missiond-daemon" :exit_code 0 :ok true :notes "1667 tests pass (baseline 1666; +1 wave27-06 smoke)")
     (:command "cargo build --workspace" :exit_code 0 :ok true)
     (:command "node scripts/check-task-contract.mjs --all" :exit_code 0 :ok true :notes "92 tasks")
     (:command "git diff --check -- scripts/check-router-dispatch-descriptor.mjs scripts/build-router-dispatch-descriptor.mjs scripts/check-task-report.mjs scripts/render-claudecode-task.mjs crates/missiond-daemon/src/handlers/knowledge/plan.rs" :exit_code 0 :ok true)
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave27/wave27-06-router-dispatch-descriptor-smoke-v0.lisp --mode staged" :exit_code 0 :ok true :notes "5 staged files all in :write-scope, none in :must-not-touch")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave27/wave27-06-router-dispatch-descriptor-smoke-v0.lisp" :exit_code 0 :ok true :notes "verify OK against commit 7f65f0590a1a")]

  :major_decisions
    [(:decision "Layer A: pin 5 wave27-06 cross-wave invariants under recognizable wave27-06-* fixture names rather than retrofit existing wave27-01 fixtures"
                :rationale "wave27-06 names exist so a future bisect can grep `wave27-06` and confirm the smoke layer is still asserting the invariant chain end-to-end; existing wave27-01 fixtures stay as primary schema validation, the wave27-06 fixtures stay as cross-wave invariant pins"
                :trace_ref "wave27-trace-06-commit-001")
     (:decision "Layer B: 2 fixtures pinning the cross-wave invariant 5 (eligibility flipping does NOT promote the descriptor to a runtime apply signal)"
                :rationale "seed registry path proves eligible=false + no_execution=true; runtime-ready synthetic registry proves eligible=true + no_execution=true STILL holds; together they pin the wave27 promise that no_execution / runtime_replacement / dry_run_only literals NEVER vary by eligibility"
                :trace_ref "wave27-trace-06-commit-001")
     (:decision "Layer C: SINGLE exhaustive smoke test rather than 2 separate tests"
                :rationale "single attribution point for a future bisect; the test asserts 6 invariants in one shot (3 locked Bools + seed-eligibility=false + dispatch byte-identical + plan.rs self-audit) so a regression on any of them lands directly on this test name"
                :trace_ref "wave27-trace-06-commit-001")
     (:decision "Layer C self-audit: assemble forbidden-pattern strings from String::from(..) + .. AT RUNTIME and rename the holding vars (t_oa / t_an instead of token_openai / token_anthropic)"
                :rationale "wave24-06 / wave25-05 / wave26-06 lesson: literal regex strings in the audit body would self-trip the audit's own grep when the file is re-read; var names must ALSO stay clear of forbidden literals because the comment+string stripper preserves identifiers"
                :trace_ref "wave27-trace-06-commit-001")
     (:decision "Layer C self-audit: ship strip_rust_comments_and_strings helper alongside the test"
                :rationale "renderer self-audit (wave26-06 / wave27-05) is JS-side and uses regex stripper; the daemon side needs Rust-aware stripping (// line comments + /* */ block comments + double-quoted string literals); helper handles the three cases conservatively, panicking on any forbidden pattern in the surviving active source"
                :trace_ref "wave27-trace-06-commit-001")
     (:decision "Layer D: positive fixture only (full 6-field descriptor block accepted)"
                :rationale "the 5 wave27-04 negatives (no_execution=false / no_execution=\"true\" string / eligible=\"false\" string / invalid status enum / absolute path) already pin rejection behaviors; the wave27-06 fixture closes the loop by pinning the happy-path the wave27-04 negatives are designed to gate against"
                :trace_ref "wave27-trace-06-commit-001")
     (:decision "Layer E: SINGLE exhaustive fixture pinning all 6 literals"
                :rationale "the wave27-05 fixture already exercises the dispatch-descriptor sub-section structurally; the wave27-06 fixture re-asserts the literal set so a regression dropping ANY of advisory / dry-run only / no execution / MUST NOT switch backend / build-router-dispatch-descriptor / check-router-dispatch-descriptor surfaces under the wave27-06 grep with the missing literal name in the panic message"
                :trace_ref "wave27-trace-06-commit-001")
     (:decision "NO MCP schema change in this smoke task"
                :rationale "task contract requirement 7 forbids schema mutation unless required by a compile fix; no compile fix needed; the wave27-03 router_dispatch_descriptor MCP arg already exists and the smoke exercises it through attach_router_recommendation_block + parse_router_policy_mode without any new arg surface"
                :trace_ref "wave27-trace-06-commit-001")
     (:decision "NO must-not-touch crate file edits"
                :rationale "plan_dag.rs / workstation_dispatch.rs / agent_execution.rs / crates/missiond-mcp/src/tools/knowledge/plan.rs all stay untouched per :must-not-touch list; smoke layer C lives in handlers::knowledge::plan::tests inside plan.rs (the in-scope file)"
                :trace_ref "wave27-trace-06-commit-001")]

  :time_sinks
    [(:label "Layer C self-audit pattern table debug" :notes "first run failed with `openai` matching plan.rs because variable name token_openai survived the comment+string stripper; renamed all token_* vars to t_* with literals also broken across the underscore so neither the value nor the identifier name carries the literal")]

  :trace_refs
    ["wave27-trace-06-start-001"
     "wave27-trace-06-commit-001"
     "wave27-trace-06-complete-001"
     ".missiond/tasks/wave27/shared-memory.lisp#wave27-06-claim-001"
     ".missiond/tasks/wave27/shared-memory.lisp#wave27-06-completion-001"]

  :notes "Cross-wave invariants pinned (per task brief 1-10):
1. dry_run_only literal in EVERY descriptor — Layer A wave27-06-valid-blocked-seed-descriptor + wave27-06-valid-runtime-ready-eligible-descriptor; Layer B wave27-06-build-seed + wave27-06-build-runtime-ready; Layer C router_dispatch_descriptor_smoke_pins_wave27_invariants Part 1.
2. runtime_replacement=false literal in EVERY descriptor — Layer A wave27-06-runtime-replacement-true-rejected + 2 valid wave27-06 fixtures; Layer B both wave27-06 fixtures; Layer C smoke Part 1.
3. no_execution=true literal in EVERY descriptor — Layer A wave27-06-no-execution-false-rejected + 2 valid wave27-06 fixtures; Layer B both wave27-06 fixtures; Layer C smoke Part 1.
4. router_apply_eligible=true ONLY when readiness=runtime-ready+runtime_allowed=true+confidence=high+blockers empty — Layer A wave27-06-current-default-eligible-rejected (negative); Layer B wave27-06-build-runtime-ready-yields-eligible-true-no-execution-true (positive); seed registry path always eligible=false in Layer B + Layer C smoke Part 1 invariant 4.
5. Even when eligible=true the no_execution/runtime_replacement/dry_run_only literals MUST hold — Layer B wave27-06-build-runtime-ready-yields-eligible-true-no-execution-true (the core wave27-06 promise).
6. Report-checker rejects :router_dispatch_no_execution false AND :router_dispatch_no_execution \"true\" string — Layer D pre-existing wave27-04 negatives + new Layer D wave27-06 positive happy-path closes the loop.
7. mission_plan off/default mode byte-identical even with all 4 router args — pre-existing Layer C wave27-03 router_dispatch_descriptor_off_default_does_no_extra_io test (still passing).
8. CLI/Rust parity: same task+policy+registry → both engines emit consistent descriptor — Layer C smoke Part 1 with seed registry yields claudecode/current-default/eligible=false; Layer B wave27-06-build-seed yields the same agreement (claudecode current-default eligible=false 3 blockers). Both code paths import the same pure recommend() / annotateRecommendationWithReadiness() projection.
9. Renderer literals advisory / dry-run only / no execution / MUST NOT switch backend / build-router-dispatch-descriptor / check-router-dispatch-descriptor all present — Layer E wave27-06-renderer-literals exhaustive pin.
10. NO new shell-out / spawn / git mutation / network / LLM in router descriptor code path — Layer C smoke Part 3 self-audit on plan.rs active source (11-pattern table assembled at runtime from String::from(..) + .. so audit does NOT self-trip; helper strip_rust_comments_and_strings removes // line + /* */ block comments + \"...\" string literals; var names also kept clear of forbidden literals).

CLI/Rust parity proof: Layer C smoke Part 1 with seed registry path (claudecode current-default + low confidence) yields recommended_backend=claudecode + readiness=current-default + apply_eligible=false + non-empty blockers. Layer B wave27-06-build-seed-yields-eligible-false-no-execution-true with the same seedPolicy + seedRegistry + recommend() pure function yields the same triplet. Both engines agree on backend=claudecode, readiness=current-default, eligible=false.

Test counts: daemon 1666 -> 1667 (+1 router_dispatch_descriptor_smoke_pins_wave27_invariants); plan tests 375 -> 376; mcp lib unchanged.

Node fixture counts: check-router-dispatch-descriptor 22 -> 27 (+5); build-router-dispatch-descriptor 10 -> 12 (+2); check-task-report 38 -> 39 (+1); render-claudecode-task 4 -> 5 (+1).

Audit confirmation: zero new shell-out (no child_process / spawn / spawn_blocking / tokio::process / std::process::Command); zero new network (no reqwest / hyper); zero new LLM (no openai / anthropic); zero new git mutation (no git add/commit/push/reset/checkout/rm); zero new git2 surface — asserted by Layer C self-audit on the in-scope plan.rs.

Acceptance command exit codes: A=0 / B=0 / D=0 / E=0 / cargo test plan tests=0 / cargo test daemon=0 / cargo build --workspace=0 / check-task-contract --all=0 / git diff --check=0 / task-scope-guard staged=0 / verify-task-contract=0.

Time-sink note (also in :time_sinks): Layer C first run failed because the var name token_openai contained the literal openai; the stripper preserves identifier names so the regex matched. Fixed by renaming token_openai/token_anthropic to t_oa/t_an and assembling the literals at runtime so neither the value nor the identifier name carries the forbidden token.")
