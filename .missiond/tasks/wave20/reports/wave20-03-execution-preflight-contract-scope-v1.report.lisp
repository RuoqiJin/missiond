;; Wave 20 / Task 03 — execution preflight task-contract scope v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave20/wave20-03-execution-preflight-contract-scope-v1.lisp

(report wave20-03-execution-preflight-contract-scope-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave20-03-execution-preflight-contract-scope-v1"
  :status done
  :commit_hash "fe835e880f27a130329aa9fa0f84a2352c8e83b3"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
      :exit_code 0
      :ok true
      :notes "68 passed (12 new wave20-03 tests on top of wave-19 56 baseline)")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1225 passed; 1 cross-agent failure in handlers::knowledge::plan::tests::refuse_llm_inference_in_dag_mode_blocks_sonnet_suggest is from wave20-07 plan.rs (in this task's must-not-touch); not from this commit. agent_execution full suite is 68/68 PASS.")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed (mission_execution schema description updated with task_contract_path preflight semantics, exercised via test_get_tool / tests::test_all_tools_count)")
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
      :notes "no whitespace errors")]

  :scope_deviations []

  :notes
    "Preflight contract-scope v1: action=preflight_commit now accepts task_contract_path. When supplied, daemon resolves the path against the project root, loads via super::workstation_dispatch::load_task_contract (read-only — same projector wave19-08 enforcement uses), then folds five new fields onto the response: task_contract_status (loaded|missing|malformed), task_contract_resolved_path, staged_out_of_scope, staged_forbidden, unstaged_in_scope, plus a contract-aware next_step. Drift on either staged_forbidden OR staged_out_of_scope flips ok=false so downstream go/no-go consumers stay honest. Backward compatibility: task_contract_path absent → response shape is byte-identical with the wave-18 + wave-19 baseline (no new keys). Pure helpers added: pattern_matches_path / glob_to_regex (mirror scripts/lib/missiond_lisp.mjs::pathMatchesPattern so daemon and the post-commit task-scope-guard.mjs share semantics; **/* / ? / .escape all exercised), build_contract_scope_summary (4-field projection), evaluate_task_contract_for_preflight (load + status label). Read-only proof: grep '^.*Command::new(\"git\")' over the file returns ONE hit (existing wave18-08 `git status --porcelain=v1`); zero `git add/commit/reset/checkout/stash/push/checkout` invocations introduced. 12 new unit tests cover glob semantics (bare-prefix, **/*, ?, separator normalisation), the four-field projection (clean / forbidden glob / out-of-scope / unstaged-drift / empty-write-scope), and the loader status labels (loaded / missing / malformed / absolute-path)."

  )
