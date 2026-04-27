;; Wave 21 / Task 04 — autonomous workstation LLM proposal v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave21/wave21-04-autonomous-workstation-llm-proposal-v0.lisp

(report wave21-04-autonomous-workstation-llm-proposal-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave21-04-autonomous-workstation-llm-proposal-v0"
  :status done
  :commit_hash "68b84f123b5e"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests"
      :exit_code 0
      :ok true
      :notes "109 passed (75 baseline + 34 new wave21-04 tests). New tests cover: WorkstationProposalStatus + WorkstationProposalSafetyStatus + WorkstationProposalConfidence wire-string pins, WorkstationProposal::to_json applied=false invariant, WorkstationProposalBundle::unavailable + ::plan_hints_present constructors, to_response_json auto_spawn=false invariant, WorkstationProposalGate is_fully_silent / signal_summary across each signal source, build_workstation_proposal_prompt embeds plan + provenance + four-field whitelist + never-auto-spawn pin (rejects prompt-fallback as proposable), parse_workstation_proposals canonical-envelope / bare-array / fenced / unknown-field / non-string / blank / invalid-confidence / missing-evidence / dedupe / cap / garbage-json / missing-key, classify_proposal_safety target/strategy/objective/scope rules, parse tags unsupported_target, unavailable_reason 'no fallback' text pin.")
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "249 passed (228 baseline + 21 new wave21-04 tests). New tests cover: parse_workstation_inference_mode default off / blank / off-string accepted, sonnet_suggest accepted, hyphenated typo rejected with canonical names quoted; WorkstationInferenceMode wire string round-trip; refuse_workstation_inference_in_dag_mode blocks scheduler_mode=dag_v1+sonnet_suggest with INVALID_PARAM + 'single-node-execute-only' reason; refuse allows off-shaped + scheduler_mode missing combinations; plan_hints_carry_workstation_signal detects each of 10 workstation knobs (objective/summary/scope/owned/forbidden/accept/policy/tp/cwd/ds) and ignores blanks; attach_workstation_proposals_block no-op without bundle, attaches bundle + mode echo, preserves pre-existing block, skips structured errors.")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1370 passed; 0 failed (after wave21-03 committed at 308426e its daemon_never_invokes_mutating_git test was fixed; the earlier transient failure during the concurrent wave21-03 in-progress work resolved itself once their commit landed). Daemon test count grew from 1304 baseline to 1370 (+66 new tests across wave21-01..04).")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed. mission_plan schema description gains wave21-04 paragraph documenting workstation_inference_mode + workstation_proposals + auto_spawn=false invariant; exercised via test_get_tool / test_plan_actions_match_lisp / test_all_tools_count.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "workspace clean build")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 lisp files OK")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors in the three write-scope files")]

  :scope_deviations []

  :notes
    "Autonomous workstation LLM proposal v0: opt-in `workstation_inference_mode` knob (off | sonnet_suggest, default off preserves wave-15..20 byte-shape) that asks Sonnet to PROPOSE the four core workstation dispatch fields (target / dispatch_strategy / objective / scope) ONLY when caller passed no relevant args (target / dispatch_strategy / objective / scope / owned_files / target_project / requested_cwd / cwd) AND PLAN.lisp surfaced no workstation hints AND PLAN did not opt into :workstation-dispatch. Proposals are SURFACED-only on the response under `workstation_proposals` for operator review and NEVER auto-applied: each proposal carries `applied=false` on the wire and the bundle carries `auto_spawn=false` so observers can pivot on the never-spawn invariant directly with a single grep. Each proposal is validated into a structured record {field, value (string), confidence (high|medium|low), evidence (short justification), safety_status (safe|ambiguous_value|unsupported_target|invalid_strategy)}; cap pinned at WORKSTATION_PROPOSAL_CAP=6 leaves headroom for variants. Conservative target whitelist {mission_execution, mission_task_delegate, mission_flow_run}; conservative dispatch_strategy whitelist {resident-lisp, fresh-code-alignment, agent-team, mixed} (prompt-fallback / unknown deliberately excluded — never proposable; the system prompt teaches the model this and the validator tags out-of-list values with safety_status). Sonnet unavailable surfaces WorkstationProposalStatus::Unavailable + a typed unavailable_reason text pinning 'no fallback to claude -p / prompt mode in v0'; the gate-suppressed (caller / PLAN already supplied at least one signal) path surfaces WorkstationProposalStatus::PlanHintsPresent + signal_summary listing which slots fired so operators can audit the suppression. DAG mode (scheduler_mode=dag_v1) preflight rejects sonnet_suggest with INVALID_PARAM + a 'single-node-execute-only' reason mirroring the wave-20 / task 07 enforcement on the plan-field surface (per-node bundle composition is left to a future wave). The WorkstationProposalGate struct holds eight signal flags (caller_target/dispatch_strategy/objective/scope/owned_files/project_signal + plan_hints + plan_workstation_opt_in) plus is_fully_silent / signal_summary helpers so the gate stays unit-testable without standing up the full execute pipeline. attach_workstation_proposals_block splices the bundle onto every dispatch branch (executing / dispatch_skipped / dry_run / inner_error / safe-descriptor / bridge); errors and pre-existing keys are preserved untouched. The mode echo `workstation_inference_mode=\"sonnet_suggest\"` lands on the response alongside the bundle so a single grep tells observers the mode was active. compute_workstation_proposal_bundle is async and lives in plan.rs at execute scope; the bundle is computed BEFORE the dispatch path runs so it attaches uniformly to whichever branch lands. Read-only invariant: this layer NEVER calls run_workstation_dispatch* based on proposals — the dispatch decision is exactly what evaluate_dispatch_decision returned (wave-15 explicit_arg / wave-15 plan_hint / wave-16 inferred / disabled / not_applicable). Sonnet integration reuses crate::minimax_client::ChatMessage + state.sonnet (matching the wave-20 / task 07 pattern). MCP descriptor adds workstation_inference_mode enum (off | sonnet_suggest) plus a long English description pinning every invariant + a bilingual paragraph in the Chinese tool description footer. Acceptance: workstation_dispatch::tests 109 passed (34 new); plan::tests 249 passed (21 new); cargo build --workspace OK; missiond-mcp --lib 17/17; check-architecture-lisp --all-v2 20 files; git diff --check no whitespace. Pre-commit task-scope-guard --mode staged: 3 staged files all in :write-scope (no out-of-scope leakage). Post-commit verify-task-contract.mjs: 68b84f123b5e against contract OK. Full daemon test (cargo test -p missiond-daemon) reports 1370 passed; 0 failed after wave21-03 (308426e) committed its agent_execution.rs changes; the earlier transient failure on a concurrent wave21-03 in-progress test resolved once their commit landed. Report written out-of-scope per wave21 protocol; intentionally untracked."
  )
