;; Wave 21 / Task 03 — execution report verifier integration v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave21/wave21-03-execution-report-verifier-integration-v1.lisp

(report wave21-03-execution-report-verifier-integration-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave21-03-execution-report-verifier-integration-v1"
  :status done
  :commit_hash "308426e15c11abce72af10799d5b171e58217263"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
      :exit_code 0
      :ok true
      :notes "102 passed (17 new wave21-03 tests on top of wave20-03 baseline). New tests cover task_run_verifier_status normalizer, parse_completions wave21-03 round-trip (incl. verified=true/false atom), legacy-omits-wave21-fields, mini report reader (extracts schema/task_id/commit_hash, rejects non-report top form), task contract head-id reader, plus the eight enforce_verified_completion failure codes (VERIFIED_REQUIRES_ENFORCEMENT/TASK_CONTRACT/TASK_REPORT/COMMIT_HASH, TASK_REPORT_REQUIRED/MALFORMED, TASK_REPORT_TASK_ID_MISMATCH, TASK_REPORT_COMMIT_HASH_MISMATCH), happy-path validation summary, short<->long sha prefix overlap, and the daemon_never_invokes_mutating_git proof.")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1370 passed; 0 failed. Full daemon suite green (includes the 102 agent_execution tests above plus all unrelated crates).")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed. mission_execution schema description updated with wave21-03 task-run verifier metadata; exercised via test_get_tool / tests::test_all_tools_count.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "workspace clean build")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 lisp files OK")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors in the two write-scope files")]

  :scope_deviations []

  :notes
    "Execution report verifier integration v1: action=complete grows four optional task-run verifier fields (task_run_verifier_status, shared_memory_path, verifier_diagnostics, verified) alongside the existing wave19-08 task-contract slots. verified=true triggers a daemon-side read-only cross-check that loads the task-report file off disk via the local sexp module (no new dependency, no mutation). Preconditions enforced fail-fast BEFORE any companion log mutation: enforce_scoped_commit=true + task_contract_path + task_report_path + commit_hash all required; missing each → structured VERIFIED_REQUIRES_ENFORCEMENT / VERIFIED_REQUIRES_TASK_CONTRACT / VERIFIED_REQUIRES_TASK_REPORT / VERIFIED_REQUIRES_COMMIT_HASH error. Cross-check: report :schema must equal missiond.report-contract.v1 (TASK_REPORT_MALFORMED), report :task_id must match the (task <id> ...) head id parsed off the contract file (TASK_REPORT_TASK_ID_MISMATCH), report :commit_hash must overlap the supplied commit_hash via prefix-rule (TASK_REPORT_COMMIT_HASH_MISMATCH). Short/long sha overlap is supported (`abc1234` vs full 40-char sha both pass). Persistence: each new field is written verbatim into the completion entry as a Lisp keyword (`:task-run-verifier-status`, `:shared-memory-path`, `:verifier-diagnostics`, `:verified`); `verified` writes as a bare `true`/`false` atom. parse_completions round-trips all four. CompletionRecord struct extended with four new Option fields; legacy companion logs that omit them keep byte-identical shape (None everywhere new). Response surface mirrors fields when supplied + emits verified_validation summary on the happy path. action=preflight_commit also echoes task_report_path / shared_memory_path advisory hints (no preflight-side load). MCP schema (crates/missiond-mcp/src/tools/knowledge/agent_execution.rs) gains the four new properties + tool description appended with wave21-03 paragraph. Read-only proof: grep `Command::new(\"git\")` over agent_execution.rs returns ONE hit (the wave18-08 status read); zero git add/commit/reset/checkout/stash/push/merge/rebase/clean argv strings introduced; daemon_never_invokes_mutating_git unit test pins the invariant. Daemon NEVER spawns Node here either — wave21-02 scripts/verify-task-run.mjs is the out-of-process authority and these fields are the durable record that the writer asserted it passed plus the daemon-side cross-check that the assertion still survives a re-load from the same files."

  )
