;; Wave 27 shared-memory ledger.
;; Schema: .missiond/tasks/schema/shared-memory-v1.lisp
;;
;; Append-only. Agents add entries while they hold a live :claim for their
;; task id. Editing or removing prior entries is forbidden; append a
;; (correction ...) entry instead.

(shared-memory wave27
  :schema "missiond.shared-memory.v1"
  :wave wave27
  :created-at "2026-04-28T10:56:00+08:00"
  :sequence 1

  (observation
    :id wave27-bootstrap-001
    :task wave27-00-archive-wave26-artifacts
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T10:56:00+08:00"
    :touched []
    :summary "Bootstrap entry: Wave 27 keeps router backend replacement disabled, but introduces a router dispatch descriptor layer so readiness/apply-blocker facts can become a machine-checkable handoff before any future runtime consumer.")

  (claim
    :id wave27-00-claim-001
    :task wave27-00-archive-wave26-artifacts
    :agent claudecode
    :seq 2
    :at "2026-04-28T11:17:31+08:00"
    :touched [".missiond/tasks/wave26/**"
              ".missiond/claudecode/wave26-*.md"]
    :summary "Claim wave27-00 to archive untracked Wave 26 task contracts, rendered briefs, reports, shared-memory and session-trace ledgers per write-scope; wave26-07 backfill commit 33a213a satisfies the lone dependency.")

  (completion
    :id wave27-00-completion-001
    :task wave27-00-archive-wave26-artifacts
    :agent claudecode
    :seq 3
    :at "2026-04-28T11:18:26+08:00"
    :touched [".missiond/tasks/wave26/**"
              ".missiond/claudecode/wave26-*.md"]
    :summary "Archived 27 Wave 26 artifacts (9 briefs, 7 reports, 9 task lisps, shared-memory, session-trace) at commit 76410472ca638e5a54ac52f91020631f92c92c44. All acceptance commands exit=0; wave26-07/wave26-08 (Codex-owned) reports intentionally absent.")

  (claim
    :id wave27-01-claim-001
    :task wave27-01-router-dispatch-descriptor-schema-v0
    :agent claudecode
    :seq 4
    :at "2026-04-28T11:23:05+08:00"
    :touched [".missiond/tasks/schema/router-dispatch-descriptor-v1.lisp"
              "scripts/check-router-dispatch-descriptor.mjs"]
    :summary "Claim wave27-01 to introduce router-dispatch-descriptor v1 schema + read-only checker; descriptor is a handoff contract derived from router recommendation + backend readiness, with hard-locked no-execution invariants (dry_run_only=true, runtime_replacement=false, no_execution=true). Wave27-00 archive completion 76410472 satisfies depends-on.")

  (completion
    :id wave27-01-completion-001
    :task wave27-01-router-dispatch-descriptor-schema-v0
    :agent claudecode
    :seq 5
    :at "2026-04-28T11:35:42+08:00"
    :touched [".missiond/tasks/schema/router-dispatch-descriptor-v1.lisp"
              "scripts/check-router-dispatch-descriptor.mjs"]
    :refs ["wave27-trace-01-commit-001" "wave27-trace-01-complete-001"]
    :summary "wave27-01 done at commit f451b044abf37f36ce6353f5c4c21f0a8eb18c97 — schema missiond.router-dispatch-descriptor.v1 (13 required + 4 optional fields) + checker (--json --stdin --dry-fixture, 22 fixtures across 9 categories, named exports for wave27-02..06). Locked invariants: :dry_run_only true, :runtime_replacement false, :no_execution true (literal atoms only). Eligibility-gate: router_apply_eligible=true requires runtime-ready + runtime_allowed=true + confidence=high + zero blockers (current-default REJECTED). Acceptance: dry-fixture exit=0, check-task-contract --all exit=0, git diff --check exit=0; task-scope-guard staged OK; verify-task-contract OK.")

  (claim
    :id wave27-02-claim-001
    :task wave27-02-router-dispatch-descriptor-cli-v0
    :agent claudecode
    :seq 6
    :at "2026-04-28T11:40:00+08:00"
    :touched ["scripts/build-router-dispatch-descriptor.mjs"]
    :summary "Claim wave27-02 to ship the read-only router dispatch descriptor CLI: parses task contract + router-policy (+ optional trace-index, required backend-registry), reuses recommend()/readBackendRegistryFile()/validateDescriptorObject() in-process, emits descriptor matching missiond.router-dispatch-descriptor.v1 (Lisp default, --json optional). Hard invariants kept literal: dry_run_only=true, runtime_replacement=false, no_execution=true. wave27-01 commit f451b044abf3 satisfies depends-on.")

  (claim
    :id wave27-04-claim-001
    :task wave27-04-report-router-dispatch-descriptor-fields-v0
    :agent claudecode
    :seq 7
    :at "2026-04-28T11:41:30+08:00"
    :touched [".missiond/tasks/schema/report-contract-v1.lisp"
              "scripts/check-task-report.mjs"]
    :summary "Claim wave27-04 to extend report-contract v1 with 6 optional flat router-dispatch-descriptor fields (router_dispatch_descriptor_path / _status / _backend / _eligible / _no_execution / _blockers). Strict additive — 31 existing fixtures stay byte-identical green; 7 new fixtures (legacy + valid + 5 negatives) added. Reuses wave25-02 helpers (validateRouterEnumField, validateRouterRepoRelativePath, validateRouterLiteralBool) and wave26-04 helpers (validateRouterLiteralBoolEither, validateRouterApplyBlockers). Cross-wave invariant locked: :router_dispatch_no_execution must be literal atom true (false AND strings rejected). wave27-01 commit f451b044abf3 satisfies depends-on.")

  (claim
    :id wave27-03-claim-001
    :task wave27-03-plan-router-dispatch-descriptor-surface-v0
    :agent claudecode
    :seq 8
    :at "2026-04-28T11:42:02+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Claim wave27-03 to expose router_dispatch_descriptor block on mission_plan execute dry-run. Add OPTIONAL boolean MCP arg router_dispatch_descriptor; build descriptor from existing wave26-03 recommendation/readiness fields with hard-coded Value::Bool(true/false) literals for dry_run_only/runtime_replacement/no_execution. Off/default mode + registry_missing path remain byte-identical / structured. wave27-01 completion f451b044 satisfies depends-on.")

  (completion
    :id wave27-04-completion-001
    :task wave27-04-report-router-dispatch-descriptor-fields-v0
    :agent claudecode
    :seq 9
    :at "2026-04-28T11:44:30+08:00"
    :touched [".missiond/tasks/schema/report-contract-v1.lisp"
              "scripts/check-task-report.mjs"]
    :refs ["wave27-trace-04-commit-001" "wave27-trace-04-complete-001"]
    :summary "wave27-04 done at commit afb5ffbc3d794dcd17b29c52ebb2741bfa4c135a — added 6 optional flat report fields (router_dispatch_descriptor_path / _status / _backend / _eligible / _no_execution / _blockers) under feat(tasks): record router dispatch descriptors in reports. Helpers reused: wave25-02 validateRouterEnumField + validateRouterRepoRelativePath + validateRouterLiteralBool('true'); wave26-04 validateRouterLiteralBoolEither. New helper: validateRouterDispatchBlockers (mirrors validateRouterApplyBlockers). Fixtures 31 -> 38 (31 prior byte-identical green; 7 new: legacy / valid / no_execution=false / no_execution=quoted-string / eligible=quoted-string / bad descriptor_status enum / abs descriptor_path). Cross-wave invariant locked: :router_dispatch_no_execution literal atom true ONLY. Acceptance exit codes: dry-fixture=0 (38/38), --all=0 (57 reports), check-task-contract --all=0 (92 tasks), git diff --check=0; task-scope-guard staged OK; verify-task-contract OK against afb5ffbc3d79.")

  (completion
    :id wave27-02-completion-001
    :task wave27-02-router-dispatch-descriptor-cli-v0
    :agent claudecode
    :seq 10
    :at "2026-04-28T11:46:00+08:00"
    :touched ["scripts/build-router-dispatch-descriptor.mjs"]
    :refs ["wave27-trace-02-commit-001" "wave27-trace-02-complete-001"]
    :summary "wave27-02 done at commit 14fdf5a2317f2f0c1a2aba1f9ef168c04db9ba16 under feat(router): build dispatch descriptors. Read-only CLI scripts/build-router-dispatch-descriptor.mjs (1235 LOC) builds router-dispatch-descriptor v1 records by importing recommend()+annotateRecommendationWithReadiness() (recommend-task-backend), readBackendRegistryFile()+projectRegistry() (check-router-backend-registry), readRouterPolicyFile()+projectPolicy() (check-router-policy), and validateDescriptorObject()+SCHEMA+DESCRIPTOR_HEAD (check-router-dispatch-descriptor). CLI flags: --task (req), --policy (req), --backend-registry (req for descriptor mode), --trace-index (opt), --json (opt; default Lisp), --dry-fixture. Locked literals dry_run_only=true / runtime_replacement=false / no_execution=true are HARD-CODED — never derived from inputs. router_apply_eligible preserved verbatim from annotateRecommendationWithReadiness. Failure handling: registry missing → unknown / runtime_allowed=false / eligible=false + 'registry_missing' blocker; runtime-replacement policy → exit 1 before recommend(). Pre-validates against wave27-01 schema before emit. 10 dry-fixture cases / 10 categories all pass. Live build (wave26-02 task / seed policy / seed registry) → claudecode current-default low-confidence eligible=false + 3 blockers; default Lisp pipe to checker --stdin exit 0. Acceptance exit codes: dry-fixture=0, check-task-contract --all=0 (92 tasks), git diff --check=0; task-scope-guard staged OK; verify-task-contract OK against 14fdf5a2317f. Audit grep zero shell-out / git / fetch / LLM call sites.")

  (completion
    :id wave27-03-completion-001
    :task wave27-03-plan-router-dispatch-descriptor-surface-v0
    :agent claudecode
    :seq 11
    :at "2026-04-28T11:49:33+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :refs ["wave27-trace-03-commit-001" "wave27-trace-03-complete-001"]
    :summary "wave27-03 done at commit 6e4f14db7f4ab47e9e61f651bc6d339b92c001c6 under feat(plan): surface router dispatch descriptors. Added OPTIONAL JSON-bool MCP arg router_dispatch_descriptor (only honored when router_policy_mode=dry_run; absent / non-bool / false ignored). Daemon: dispatch_descriptor_requested() helper + attach_router_dispatch_descriptor() projector that runs AFTER wave26-03 attach_backend_readiness_fields. compute_recommendation refactored into compute_recommendation + compute_recommendation_block so the descriptor splice is a single post-pass. Locked invariants emitted as Value::Bool literals (dry_run_only=true, runtime_replacement=false, no_execution=true) — never computed, never strings. Registry-absent path: top-level descriptor_status='registry_missing' surfaced on recommendation block; descriptor body OMITTED to avoid faking readiness. Off-mode early-return predates the descriptor branch (zero file I/O even with non-existent paths supplied — proven by router_dispatch_descriptor_off_default_does_no_extra_io test using /this/path/does/not/exist/* paths for policy + trace-index + registry, byte-identical baseline). 5 new tests under handlers::knowledge::plan::tests; daemon total 1666 (baseline 1661, +5); mcp lib 17 (no count change). Acceptance exit codes: cargo test plan tests=0 (375), cargo test daemon=0 (1666), cargo test mcp lib=0 (17), cargo build --workspace=0, check-task-contract --all=0 (92 tasks), git diff --check=0; task-scope-guard staged OK; verify-task-contract OK against 6e4f14db7f4a.")

  (claim
    :id wave27-05-claim-001
    :task wave27-05-renderer-router-dispatch-descriptor-context-v0
    :agent claudecode
    :seq 12
    :at "2026-04-28T11:55:31+08:00"
    :touched ["scripts/render-claudecode-task.mjs"
              ".missiond/tasks/schema/task-contract-v1.lisp"]
    :summary "Claim wave27-05 to extend the task renderer with router dispatch descriptor context: append two new build-router-dispatch-descriptor.mjs commands (default Lisp + pipe-to-checker) inside the existing wave24-05/wave25-04/wave26-05 'Router Policy (advisory)' section when BOTH policy AND registry resolve, plus a new sub-bullet group enumerating the 6 wave27-04 optional report fields with MAY-language. New 'no execution' literal lives next to the existing 'advisory' / 'dry-run only' / 'MUST NOT switch backend' literals. Schema renderer-contract gains a wave27-05 entry documenting the descriptor surface and the decision NOT to add a new optional task field (descriptors are generated on-demand from existing inputs — task + policy + registry — by wave27-02 CLI). Two new --dry-fixture cases (literals + static audit). Renderer never shells out. wave27-04 commit afb5ffbc + wave27-02 commit 14fdf5a2 + wave27-01 commit f451b044 satisfy depends-on.")

  (completion
    :id wave27-05-completion-001
    :task wave27-05-renderer-router-dispatch-descriptor-context-v0
    :agent claudecode
    :seq 13
    :at "2026-04-28T11:58:00+08:00"
    :touched ["scripts/render-claudecode-task.mjs"
              ".missiond/tasks/schema/task-contract-v1.lisp"]
    :refs ["wave27-trace-05-commit-001" "wave27-trace-05-complete-001"]
    :summary "wave27-05 done at commit 17cb401f10746f659389de159ba7381c2fe560da under feat(tasks): render router dispatch descriptor context. Decision: NO new optional task-contract field added — the dispatch descriptor is an ephemeral generated artifact (wave27-02 CLI builds it on demand from task + policy + registry inputs); a static :router-dispatch-descriptor-path field would create stale-by-design state. Rationale documented in renderer-contract status string + new :machine-context-rendered + :backward-compatibility entries. renderRouterPolicy() extended in place to append a NEW dispatch-descriptor sub-section AFTER the wave26-05 recommend-task-backend block, but ONLY when BOTH policy + registry resolve (when only the policy resolves the wave25-04 + wave26-05 surface stays byte-identical, no new sub-section appears). Sub-section preamble carries 'advisory' / 'dry-run only' / 'no execution' / 'MUST NOT switch backend' literals verbatim. TWO command lines emitted: (1) default Lisp `build-router-dispatch-descriptor.mjs --task <task> --policy <policy> --backend-registry <registry>`, (2) same command piped into `check-router-dispatch-descriptor.mjs --stdin` (no --json on pipe form per wave27-02 finding — checker only parses Lisp on stdin). renderReportContract() extended with sub-bullet group enumerating all 6 wave27-04 dispatch-descriptor fields with MAY-language (descriptor_path / descriptor_status enum / backend enum / eligible literal bool / no_execution literal `true` only — cross-wave invariant — / blockers vector). Two new --dry-fixture cases (literals + static-audit); fixture count 2 -> 4. Acceptance exit codes: --dry-fixture=0 (4/4 across 4 categories), wave27-02 brief --stdout render=0, rg all 5 patterns present=0, check-task-contract --all=0 (92 tasks), git diff --check=0; live pipe smoke build|check --stdin=0 (1 descriptor); task-scope-guard staged OK (2 files); verify-task-contract OK against 17cb401f1074.")

  (claim
    :id wave27-06-claim-001
    :task wave27-06-router-dispatch-descriptor-smoke-v0
    :agent claudecode
    :seq 14
    :at "2026-04-28T12:08:00+08:00"
    :touched ["scripts/check-router-dispatch-descriptor.mjs"
              "scripts/build-router-dispatch-descriptor.mjs"
              "scripts/check-task-report.mjs"
              "scripts/render-claudecode-task.mjs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
    :summary "Claim wave27-06 to add cross-layer smoke pinning the wave27 dispatch-descriptor invariants across 5 layers: (A) check-router-dispatch-descriptor.mjs +5 dry-fixtures (22 -> 27) covering valid-blocked-seed-descriptor / valid-runtime-ready-eligible-descriptor / no_execution false rejected / runtime_replacement true rejected / current-default+eligible=true rejected; (B) build-router-dispatch-descriptor.mjs +2 dry-fixtures (10 -> 12) proving seed-registry yields eligible=false+no_execution=true while synthetic runtime-ready yields eligible=true+no_execution=true (cross-wave invariant 5); (C) plan.rs +1 exhaustive smoke test in handlers::knowledge::plan::tests pinning all 3 locked literal-bools + dispatch byte-identical baseline + plan.rs self-audit forbidden-pattern grep (375 -> 376); (D) check-task-report.mjs +1 fixture (38 -> 39) — full 6-field descriptor block accepted; (E) render-claudecode-task.mjs +1 fixture (4 -> 5) — synthetic task with policy+registry renders all 6 wave27-06 literals: 'advisory' / 'dry-run only' / 'no execution' / 'MUST NOT switch backend' / 'build-router-dispatch-descriptor' / 'check-router-dispatch-descriptor'. Static audit assembles forbidden-pattern strings from parts to avoid self-trip (wave24-06 / wave25-05 / wave26-06 lesson). NO new MCP tools; NO must-not-touch crate file edits (plan_dag / workstation_dispatch / agent_execution / mcp/plan stay untouched). All 4 wave27-06 dependencies (-02 / -03 / -04 / -05) committed.")

  (completion
    :id wave27-06-completion-001
    :task wave27-06-router-dispatch-descriptor-smoke-v0
    :agent claudecode
    :seq 15
    :at "2026-04-28T12:35:30+08:00"
    :touched ["scripts/check-router-dispatch-descriptor.mjs"
              "scripts/build-router-dispatch-descriptor.mjs"
              "scripts/check-task-report.mjs"
              "scripts/render-claudecode-task.mjs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
    :refs ["wave27-trace-06-commit-001" "wave27-trace-06-complete-001"]
    :summary "wave27-06 done at commit 7f65f0590a1a17252415a2d91cbbd967e3d512e2 under test(router): smoke dispatch descriptor chain. 5 layers wired in single commit. Layer A (check-router-dispatch-descriptor.mjs): 22 -> 27 (+5 wave27-06 fixtures, +1 category wave27-06-cross-wave-invariant). Layer B (build-router-dispatch-descriptor.mjs): 10 -> 12 (+2 wave27-06 fixtures, +1 category). Layer C (plan.rs handlers::knowledge::plan::tests): 375 -> 376 — single exhaustive test router_dispatch_descriptor_smoke_pins_wave27_invariants pins all 3 locked literal-Bool invariants (dry_run_only/runtime_replacement/no_execution) + seed-registry eligibility=false + dispatch byte-identical with-vs-without descriptor + plan.rs self-audit forbidden-pattern grep (11 patterns: child_process / spawn / spawn_blocking / tokio::process / std::process::Command / reqwest:: / hyper:: / openai / anthropic / git verbs / git2 — all assembled from String::from(...) + ... runtime concat so audit body does NOT self-trip; helper strip_rust_comments_and_strings removes // line comments + /* */ block comments + \"...\" string literals; var names also kept clear of forbidden literals). Layer D (check-task-report.mjs): 38 -> 39 (+1 wave27-06 happy-path fixture pinning all 6 wave27-04 descriptor fields populated cleanly). Layer E (render-claudecode-task.mjs): 4 -> 5 (+1 wave27-06 fixture pinning ALL 6 literals: advisory / dry-run only / no execution / MUST NOT switch backend / build-router-dispatch-descriptor / check-router-dispatch-descriptor). Daemon test count 1666 -> 1667 (full suite). Acceptance exit codes: --dry-fixture A=0 / B=0 / D=0 / E=0; cargo test plan tests=0 (376); cargo test daemon=0 (1667); cargo build --workspace=0; check-task-contract --all=0 (92 tasks); git diff --check=0 (5 in-scope files); task-scope-guard staged OK (5 files); verify-task-contract OK against 7f65f0590a1a. Zero must-not-touch crate edits (plan_dag / workstation_dispatch / agent_execution / mcp/plan stay untouched). Self-audit confirms zero new shell-out / spawn / git mutation / network / LLM client landed in plan.rs active source. wave27-02 / wave27-03 / wave27-04 / wave27-05 dependencies satisfied."))
