;; Wave 27 / Task 03 — mission_plan router dispatch descriptor surface v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave27/wave27-03-plan-router-dispatch-descriptor-surface-v0.lisp

(report wave27-03-plan-router-dispatch-descriptor-surface-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave27-03-plan-router-dispatch-descriptor-surface-v0"
  :status done
  :commit_hash "6e4f14db7f4ab47e9e61f651bc6d339b92c001c6"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "375 passed; 0 failed; baseline 370 (+5 new descriptor tests). New tests: router_dispatch_descriptor_off_default_does_no_extra_io / router_dispatch_descriptor_dry_run_with_seed_registry_emits_no_execution_true / router_dispatch_descriptor_dry_run_with_runtime_ready_eligible / router_dispatch_descriptor_dry_run_without_registry_path_emits_status_registry_missing / router_dispatch_descriptor_does_not_change_dispatch.")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1666 passed; 0 failed; 0 ignored; baseline 1661 (+5 from this task). All wave14..wave27 daemon tests green.")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed; 0 failed; baseline 17 (no count change — only added one descriptor property to an existing tool definition). test_all_tools_count + test_plan_actions_match_lisp + schema-build tests still pass.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Workspace build clean; 86 warnings (all pre-existing dead-code warnings unrelated to this task; no new warnings introduced by descriptor wiring).")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (92 tasks). No regressions vs the wave27-00 / wave27-01 / wave27-02 / wave27-04 baseline (also 92).")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on either staged path; trailing-newline / tab-stop hygiene clean on both modified files.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave27/wave27-03-plan-router-dispatch-descriptor-surface-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave27-03-plan-router-dispatch-descriptor-surface-v0 (2 staged file(s)) — both staged paths inside :write-scope; zero matches against :must-not-touch (scripts/** / .missiond/v2/** / .missiond/router/** / .missiond/tasks/schema/** / .missiond/tasks/wave26/** / .missiond/tasks/wave27/wave27-*.lisp / .missiond/claudecode/** / plan_dag.rs / workstation_dispatch.rs / agent_execution.rs).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave27/wave27-03-plan-router-dispatch-descriptor-surface-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave27-03-plan-router-dispatch-descriptor-surface-v0 against 6e4f14db7f4a — commit hash exists; commit message matches `feat(plan): surface router dispatch descriptors` per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave27-trace-03-start-001 wave27-trace-03-commit-001 wave27-trace-03-complete-001]

  :major_decisions
    [(:decision "MCP arg `router_dispatch_descriptor` typed as JSON `boolean` (not string \"true\" / \"false\")."
      :rationale "Wave-21 surface uses real JSON bools for the apply-gate args, and prop_enum/prop helpers in missiond-mcp readily express `boolean`. Strict bool typing avoids a stringly-typed parser plus an INVALID_PARAM branch and pairs cleanly with serde_json::Value::as_bool() on the daemon side. dispatch_descriptor_requested() is strict: only Value::Bool(true) opts in; absent / false / strings / numbers all return false so a typo can never silently emit the descriptor.")
     (:decision "Implement the descriptor as a single post-pass after compute_recommendation_block builds the recommendation."
      :rationale "compute_recommendation has 5 distinct return sites (read-error / parse-error / dry_run_only-rejected / runtime_replacement-rejected / no-match-fallback / matched). Refactoring the body into compute_recommendation_block lets compute_recommendation own the descriptor splice in ONE place — readers don't have to chase the descriptor logic through every variant. Splice logic itself is pure projection off the already-built block.")
     (:decision "Locked invariants emitted as Value::Bool literals (NEVER computed, NEVER strings)."
      :rationale "Per the wave27-01 schema's locked-invariants contract, dry_run_only / runtime_replacement / no_execution must remain single legal atoms. Hard-coding them as Value::Bool(true) / Value::Bool(false) at the descriptor-build site means a future change cannot accidentally derive them from another field — the invariants are read directly off the descriptor by downstream tooling and any drift would silently authorise live dispatch. The eligible-flip test (runtime-ready + high + zero blockers → eligible=true) explicitly re-asserts that the three locked literals stay literal Bool.")
     (:decision "Registry-absent path: surface descriptor_status=\"registry_missing\" + OMIT descriptor body."
      :rationale "The wave27-01 schema requires backend_readiness_status / backend_runtime_allowed values that we cannot honestly produce without consulting a registry. Emitting a descriptor body with `unknown` readiness for the no-registry case would conflate two materially different signals (degraded readiness vs no opt-in to readiness lookup at all). The structured top-level descriptor_status field on the recommendation block is unambiguous and round-trips cleanly through downstream report / renderer / smoke surfaces.")
     (:decision "Re-pin the wave24-04 dispatch invariant under the new descriptor code path."
      :rationale "The brief explicitly calls for re-pinning the cross-wave dispatch invariant. router_dispatch_descriptor_does_not_change_dispatch tests both A (dry_run + registry, no descriptor) and B (dry_run + registry + descriptor=true) and asserts byte-identical equality on every dispatch-shaping field (target_tool / target_source / dispatch_strategy / dispatch_strategy_source / next_call / execute_mode / runner_status). The only delta is the additive descriptor block on B, plus the locked applied=false literal on both.")]

  :time_sinks
    [(:label "Reading wave24-04 / wave25-03 / wave26-03 implementation history"
      :notes "Largest sink — needed to understand exactly which fields wave26-03 already populates on the recommendation block (backend_readiness_status / backend_runtime_allowed / router_apply_eligible / router_apply_blockers / backend_registry_status / backend_warning), what shape they take in each registry state (Used matched / Used unknown_backend / Missing / Unreadable / Malformed / Absent), and where the recommendation block enters the response (action_execute_bridge → attach_router_recommendation_block).")
     (:label "Designing the registry-absent vs registry-degraded split"
      :notes "Brief specifies registry_missing for absent-path. Degraded states (Missing / Unreadable / Malformed / unknown_backend) emit the descriptor body using the synthetic unknown / false fallbacks already populated by wave26-03's attach_backend_readiness_fields, so the descriptor schema's required-fields contract is satisfied without faking readiness. registry_path() helper extracts the path from any non-Absent variant for the source_backend_registry_path echo.")
     (:label "Writing 5 tests + verifying baselines"
      :notes "Brief specified 5 explicit test names — each test was written against the existing wave26-03 fixture helpers (fixture_plan / fixture_resolved / fixture_bridge_result / write_temp_docs_policy / write_temp_trace_index / write_temp_registry / registry_body_single / parse_payload / action_execute_bridge). All 5 pass on first run; full plan tests 370 → 375; full daemon 1661 → 1666; mcp lib unchanged at 17.")]

  :unexpected_work
    [(:summary "Refactored compute_recommendation into compute_recommendation + compute_recommendation_block. The split is internally invisible (zero behavior change) but lets the descriptor splice be a single post-pass instead of duplicating across 5 return sites. All pre-existing wave24..wave26 tests stay green without modification.")
     (:summary "Added registry_path() helper to extract the path string from any non-Absent BackendRegistryInfo variant. The Used / Missing / Unreadable / Malformed variants all carry `path: String` so this is a 4-arm match. Avoided a String allocation by returning &str (the descriptor splice immediately turns it into a Value::String).")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100)."
     "Surgical Rust edit on a 21k-line file (handlers::knowledge::plan) plus a single-property MCP schema addition is the canonical claudecode beat — no Node / network / LLM call required from the worker side."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before the wave27-01 eligibility-gate would mark a descriptor as apply-eligible (the gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave27-03 ships:
     - crates/missiond-mcp/src/tools/knowledge/plan.rs: new OPTIONAL boolean MCP arg `router_dispatch_descriptor` on mission_plan execute. Documented as ignored unless router_policy_mode=dry_run; descriptor body OMITTED + descriptor_status=registry_missing surfaced when no router_backend_registry_path supplied; otherwise a structured router_dispatch_descriptor sub-object is spliced onto the recommendation block.
     - crates/missiond-daemon/src/handlers/knowledge/plan.rs: dispatch_descriptor_requested() helper (strict — only Value::Bool(true) opts in) + attach_router_dispatch_descriptor() projector that runs AFTER wave26-03 attach_backend_readiness_fields. compute_recommendation refactored into compute_recommendation + compute_recommendation_block so the descriptor splice is a single post-pass over the fully-populated block.

     Descriptor body fields (mirror wave27-01 schema where practical):
       schema:                       \"missiond.router-dispatch-descriptor.v1\" (literal string)
       task_id:                      plan.board_task_id (e.g. \"btk-1\")
       recommended_backend:          projected from block[\"recommended_backend\"]
       router_confidence:            projected from block[\"confidence\"]
       backend_readiness_status:     projected from block[\"backend_readiness_status\"] OR \"unknown\" fallback
       backend_runtime_allowed:      projected from block[\"backend_runtime_allowed\"] OR false fallback (Value::Bool)
       router_apply_eligible:        projected from block[\"router_apply_eligible\"] OR false fallback (Value::Bool)
       router_apply_blockers:        projected from block[\"router_apply_blockers\"] OR [] fallback
       dry_run_only:                 LITERAL Value::Bool(true)   — LOCKED, hard-coded, never derived
       runtime_replacement:          LITERAL Value::Bool(false)  — LOCKED, hard-coded, never derived
       no_execution:                 LITERAL Value::Bool(true)   — LOCKED, hard-coded, never derived
       source_recommendation_schema: \"missiond.router-recommendation.v0\" (echo of router_policy_dry_run::SCHEMA)
       source_policy_path:           echo of router_policy_path arg
       source_backend_registry_path: echo of router_backend_registry_path arg

     Registry-absent semantics (router_dispatch_descriptor=true + NO router_backend_registry_path):
       Recommendation block carries a NEW top-level field descriptor_status=\"registry_missing\" (string).
       Descriptor body (router_dispatch_descriptor sub-object) is INTENTIONALLY OMITTED. Rationale: the wave27-01 schema requires backend_readiness_status / backend_runtime_allowed values we cannot honestly produce without a registry. Faking readiness would conflate degraded vs no-opt-in.

     Off-mode no-I/O proof (router_dispatch_descriptor_off_default_does_no_extra_io):
       Test supplies all THREE wave-era router args (router_policy_path / router_policy_trace_index_path / router_backend_registry_path) at non-existent paths PLUS router_dispatch_descriptor=true. The Off-path early-return in attach_router_recommendation_block predates compute_recommendation entirely, so no file I/O happens for ANY of the three paths. Asserted via byte-identical baseline comparison on the response text. Sub-test repeats the assertion with mode arg ABSENT (parse_router_policy_mode default=Off) — same invariant.

     Re-pinned dispatch byte-identical invariant (router_dispatch_descriptor_does_not_change_dispatch):
       Path A: dry_run + registry, NO descriptor.
       Path B: dry_run + registry + router_dispatch_descriptor=true.
       Asserted byte-identical equality on target_tool / target_source / dispatch_strategy / dispatch_strategy_source / next_call / execute_mode / runner_status. Recommendation core fields (status / applied / recommended_backend / confidence / backend_readiness_status / router_apply_eligible) also asserted equal. Only delta: descriptor sub-object present in B. applied=false literal asserted on both paths.

     Cross-wave invariants re-pinned by router_dispatch_descriptor_dry_run_with_runtime_ready_eligible:
       Even when the wave26-03 6-condition gate satisfies (runtime-ready + runtime_allowed=true + confidence=high + zero blockers → router_apply_eligible=true), the three LOCKED descriptor invariants STAY literal Bool literals: dry_run_only=true, runtime_replacement=false, no_execution=true. Eligibility flipping does NOT promote the descriptor to a runtime apply signal.

     Test counts (handlers::knowledge::plan::tests):
       baseline:  370 (after wave26-06 smoke pin: 370)
       new:        +5 (router_dispatch_descriptor_*)
       total:     375
     Test counts (full -p missiond-daemon):
       baseline: 1661 (wave26-06 smoke pin total)
       new:        +5
       total:    1666
     Test counts (-p missiond-mcp --lib):
       baseline:   17
       new:        +0 (only added one property to an existing definition; no new test fns)
       total:      17

     Pre-commit pipeline: cargo test plan tests (375 OK) → cargo test daemon (1666 OK) → cargo test mcp lib (17 OK) → cargo build --workspace (OK) → check-task-contract --all (92 tasks OK) → git diff --check (OK) → check-missiond-hooks --json (preflight aligned) → git add (2 paths) → task-scope-guard --mode staged (OK, 2 staged) → MISSIOND_TASK_CONTRACT=... git commit -m \"feat(plan): surface router dispatch descriptors\" (commit 6e4f14db7f4a) → verify-task-contract (OK against 6e4f14db7f4a). All append-only ledger updates: shared-memory wave27-03-claim-001 (seq 8) before staging + wave27-03-completion-001 (seq 11) after verify; session-trace wave27-trace-03-start-001 (seq 10) before reading background + wave27-trace-03-commit-001 (seq 15, with commit_hash) + wave27-trace-03-complete-001 (seq 16). Both ledgers re-validated after each append (8/16 entries respectively).

     Constraints honored: NO Node / shell-out / git from Rust; NO scripts/** / .missiond/v2/** / .missiond/router/** / .missiond/tasks/schema/** / .missiond/tasks/wave26/** / .missiond/tasks/wave27/wave27-*.lisp (other than session-trace + shared-memory which are session-trace-writable / claim-allowed and explicitly NOT in :must-not-touch) / .missiond/claudecode/** / plan_dag.rs / workstation_dispatch.rs / agent_execution.rs touched. Did not git add . / git push / --no-verify / --amend / --force. NO #[allow(...)] introduced; all parameters either consumed or prefix-underscored (none of the latter actually needed). Daemon build observed via cargo (rust-analyzer not consulted).")
