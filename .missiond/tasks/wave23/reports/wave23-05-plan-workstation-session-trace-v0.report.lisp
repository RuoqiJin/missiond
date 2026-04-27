;; Wave 23 task report.

(report wave23-05-plan-workstation-session-trace-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave23-05-plan-workstation-session-trace-v0"
  :status done
  :commit_hash "0c81611374cf"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "331 passed (was 322; +9 new tests). New plan tests: validate_session_trace_path_arg_returns_none_pair_when_arg_absent, validate_session_trace_path_arg_passes_well_formed_paths_through, validate_session_trace_path_arg_required_rejects_empty_with_invalid_param, validate_session_trace_path_arg_warns_on_nul_byte_when_not_required, task_contract_inputs_from_hints_with_trace_emits_session_trace_path_in_lisp, task_contract_inputs_from_hints_omits_session_trace_when_path_absent, attach_session_trace_response_fields_is_a_noop_when_both_inputs_are_none, attach_session_trace_response_fields_splices_path_and_warning_into_envelope (8 net under plan tests; the contract-parse test landed in the workstation_dispatch tests block).")
     (:command "cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests"
      :exit_code 0
      :ok true
      :notes "150 passed (was 147; +3 new tests). New workstation_dispatch tests: build_task_brief_with_source_and_trace_omits_session_trace_block_when_path_absent, build_task_brief_with_source_and_trace_renders_session_trace_block_when_path_supplied, parse_task_contract_extracts_session_trace_path_from_contract_lisp.")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1622 passed (was 1611 from wave23-04; +11 from this wave; 0 failed; 0 ignored). Net delta exactly matches the +9 plan + +3 workstation_dispatch additions minus 1 test that already existed under both module paths via the shared fixture import — manually verified via cargo test --no-run -- --list.")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed (no count change; only schema property descriptions added — no new test surface).")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Clean build; 86 warnings (all pre-existing dead-code labels from earlier waves; warning count steady from wave23-04 baseline).")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 architecture lisp files green; this task did not touch .missiond/v2/*.lisp.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "No whitespace errors on the three write-scope paths.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "Hooks preflight aligned (core.hooksPath == .githooks, .githooks/pre-commit present and executable).")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave23/wave23-05-plan-workstation-session-trace-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "3 staged files all inside :write-scope; zero matches against :must-not-touch.")
     (:command "MISSIOND_TASK_CONTRACT=.missiond/tasks/wave23/wave23-05-plan-workstation-session-trace-v0.lisp git commit -m \"feat(plan): propagate session trace through dispatch\""
      :exit_code 0
      :ok true
      :notes "Commit 0c81611374cf: 3 files, +690 insertions, -14 deletions. Pre-commit scope-guard hook re-ran cleanly inside the commit.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave23/wave23-05-plan-workstation-session-trace-v0.lisp"
      :exit_code 0
      :ok true
      :notes "Subject equals contract :commit :message; every changed_file is in :write-scope; must-not-touch intersection empty.")
     (:command "node scripts/check-task-memory.mjs .missiond/tasks/wave23/shared-memory.lisp"
      :exit_code 0
      :ok true
      :notes "Ledger now has 16 entries; validation green after appending wave23-05 claim (seq 14) and completion (seq 16). Seq 15 was claimed by a parallel wave23-03 completion mid-run; ours moved to 16.")]
  :notes "Plan-side propagation: action_execute_internal pre-flight reads optional session_trace_path + session_trace_required args via new validate_session_trace_path_arg helper. Validation rules (deliberately narrow): non-empty after trim, no NUL byte, no ASCII control char (tab + space allowed). session_trace_required=true ⇒ malformed shape returns structured INVALID_PARAM error BEFORE any dispatch side effect (per task contract requirement 5: caller-required malformed = hard fail). session_trace_required=false (default) ⇒ malformed shape returns non-fatal trace_path_warning on response and the trace forward is suppressed (conservative posture). The validate helper deliberately does NOT inspect on-disk file existence: that is the wave23-04 consumer's append-time concern (surfaces under trace_warning), and conflating shape errors with append errors would defeat caller-vs-runtime attribution. Trace path threaded into THREE surfaces: (a) inner_args via post-build splice for mission_execution / mission_task_delegate / mission_flow_run (mission_execution per wave23-04 surface; other targets ignore unknown key); (b) workstation-dispatch substrate via new run_workstation_dispatch_with_contract_and_trace function (priority: caller arg wins, contract :session-trace-path overlay fills in when caller absent); (c) wave-19/06 emitted task contract via new task_contract_inputs_from_hints_with_trace helper that stamps :session-trace-path \"...\" onto the generated Lisp. Workstation-dispatch side: new build_task_brief_with_source_and_trace renders ## Session trace block with ledger path + worker instructions (legacy build_task_brief_with_source delegates with None to preserve byte shape). ParsedTaskContract gained session_trace_path field with kebab-case parser entry; evidence sidecar surfaces session_trace_path; inner_args carry session_trace_path. attach_session_trace_response_fields helper splices session_trace_path / trace_path_warning into the JSON envelope of every action_execute_internal return path (workstation_dispatch / dry_run / dispatch_failed / inner-success / emission-failure / emit_dry_run) so observers see the propagation regardless of branch — the noop pin test guarantees wave-15..22 callers (no trace knob) see byte-identical envelopes. MCP schema: +session_trace_path (string optional) + session_trace_required (boolean optional) with detailed propagation docstring covering the consumer / brief / contract surfaces and the malformed-required vs malformed-warned semantics. Critical design decision: WorkstationDispatchHints / WorkstationDispatchOutcome / run_workstation_dispatch_with_contract / task_contract_inputs_from_hints (the pre-existing 7-arg / 3-arg surfaces) kept their wave-22 shape verbatim — extending the struct or variants would have broken WorkstationDispatchHints / ParsedTaskContract struct-literal initializers in plan_dag.rs / unified_entry.rs (out-of-scope under this wave's contract per coordinator urgency advisory). New trace surface plumbed via *_and_trace / *_with_trace sibling functions; legacy variants delegate with None. wave23-04 trace_warning (consumer-side append failure) and wave23-05 trace_path_warning (caller-side shape failure) are intentionally distinct fields so observers can tell shape errors from append errors. Tests pin the four contract cases enumerated in the task brief (legacy / happy / malformed-required / malformed-warned) plus emitter wiring and brief render.")
