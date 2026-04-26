;; Wave 20 / Task 07 — LLM-augmented PLAN field inference v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave20/wave20-07-llm-augmented-plan-inference-v0.lisp

(report wave20-07-llm-augmented-plan-inference-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave20-07-llm-augmented-plan-inference-v0"
  :status done
  :commit_hash "6bb935ad0d67ff03a513defdd23f29d8cb364d79"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests"
      :exit_code 0
      :ok true
      :notes "245 passed (3 wave-20 DAG-side guard tests added on top of wave-19 baseline 242)")
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "225 passed (28 wave-20 LLM-augmented inference tests added on top of wave-18 baseline 197)")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1230 passed (baseline 1160 + 70 new wave-20 tests across tasks 03/06/07)")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed (plan schema enum extension exercised via test_plan_actions_match_lisp)")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "workspace clean build")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 lisp files OK")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors in scoped files")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-07-llm-augmented-plan-inference-v0.lisp"
      :exit_code 0
      :ok true
      :notes "post-commit verifier OK against commit 6bb935ad0d67")]

  :scope_deviations []

  :notes
    "wave20-07 layered an opt-in `sonnet_suggest` mode on top of the wave-18/06 deterministic engine. New mode constant INFER_MODE_SONNET_SUGGEST + enum variant InferPlanFieldsMode::SonnetSuggest extend the strict allowlist parsed by parse_infer_plan_fields_mode (off / preview / apply_safe / sonnet_suggest). Default behaviour is unchanged: off / preview / apply_safe code paths are byte-identical to wave-18 (verified by response_block_under_deterministic_modes_omits_llm_keys + the existing 22 wave-18 tests still passing). Mutation boundaries: sonnet_suggest is a SHORT-CIRCUIT mode (status=`inference_sonnet_suggest`, runner_status=`inference_sonnet_suggest_no_dispatch`) — NEVER dispatches the underlying execute pipeline, NEVER mutates plan FSM. LLM proposals are validated into the strict structure {field, value, confidence (high|medium|low), evidence, conflict_status (none|conflicts_with_caller|conflicts_with_deterministic|overlaps_deterministic_suggestion)} and surface under plan_field_inference.llm_proposals[]; each proposal carries `applied=false` on the wire so observers can pin the never-applied invariant without reading the contract. Conflict reconciliation precedence: caller > deterministic-high > deterministic-suggestion > none. Sonnet unavailability surfaces as llm_status=`llm_unavailable` + llm_unavailable_reason — there is NO silent fallback to deterministic-only and the response is not augmented as if the LLM had run. DAG path guard: refuse_llm_inference_in_dag_mode in plan_dag.rs fails fast when scheduler_mode=dag_v1 + sonnet_suggest are combined (single-node-execute-only feature in v0); the same parser typo error propagates so callers see one consistent error vocabulary. LLM proposal validation rejects: unknown fields, missing/empty evidence, value-shape mismatch (string vs bool vs string array), unknown confidence, garbage JSON, missing `proposals` key, duplicate fields; rejections land on parse_warnings[] without aborting the response. Markdown code fences (```json … ```) are tolerated since Sonnet sometimes wraps despite the system prompt forbidding them. Six fields supported: target / dispatch_strategy / target_project / owned_files / acceptance_mode / workstation_dispatch (mirrors deterministic engine).")
