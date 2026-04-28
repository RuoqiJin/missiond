;; Wave 25 task contract.

(task wave25-05-router-policy-measurement-smoke-v1
  :schema "missiond.task-contract.v1"
  :title "Router policy measurement smoke v1"
  :kind smoke
  :status ready
  :owner "claudecode"
  :depends-on ["wave25-01-router-policy-corpus-evaluator-v0"
               "wave25-02-report-router-recommendation-fields-v0"
               "wave25-03-plan-router-trace-index-confidence-v1"
               "wave25-04-renderer-router-recommendation-command-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Add cross-layer smoke coverage proving the Wave25 measurable router loop is still advisory: evaluator, report fields, renderer commands, and mission_plan trace-index confidence agree without mutating dispatch."

  :write-scope
    ["scripts/evaluate-router-policy-corpus.mjs"
     "scripts/recommend-task-backend.mjs"
     "scripts/render-claudecode-task.mjs"
     "scripts/check-task-report.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     ".missiond/v2/**"
     ".missiond/tasks/schema/*.lisp"
     ".missiond/tasks/wave24/**"
     ".missiond/tasks/wave25/wave25-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Add deterministic fixtures or tests that run the full measurement loop without real LLM, spawn, mutating git, or network."
     "Pin invariants: runtime_replacement=false, dry_run_only=true, applied=false, renderer advisory text, report applied=false, mission_plan off/default byte-shape unchanged."
     "Pin parity for one fixture where CLI recommendation and plan dry-run confidence agree using the same trace-index evidence."
     "Keep all smoke additions in existing test/fixture style; do not add a new MCP tool."]

  :acceptance
    ["node scripts/evaluate-router-policy-corpus.mjs --dry-fixture"
     "node scripts/recommend-task-backend.mjs --dry-fixture"
     "node scripts/check-task-report.mjs --dry-fixture"
     "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "git diff --check -- scripts/evaluate-router-policy-corpus.mjs scripts/recommend-task-backend.mjs scripts/render-claudecode-task.mjs scripts/check-task-report.mjs crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :commit
    (:required true
     :message "test(router): smoke measurable dry-run policy loop"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Smoke invariants pinned."
     "CLI/Rust parity proof."
     "Acceptance command results."])

