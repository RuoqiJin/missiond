;; Wave 21 task contract.

(task wave21-03-execution-report-verifier-integration-v1
  :schema "missiond.task-contract.v1"
  :title "Execution report verifier integration v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave21-02-run-verifier-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Expose task-run verification status through mission_execution complete/preflight metadata without making the daemon perform mutating git operations."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]

  :must-not-touch
    ["crates/missiond-core/src/event/events/execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "scripts/**"
     ".missiond/v2/*.lisp"]

  :requirements
    ["Add optional fields for task_run_verifier_status, task_report_path, shared_memory_path, and verifier_diagnostics where appropriate."
     "If enforce_scoped_commit=true and task_contract_path is present, allow caller to supply verified=true only when task_report_path and commit_hash are also present."
     "Daemon may perform read-only file parsing if existing helpers are local; otherwise record caller-supplied verifier status and fail fast on missing critical fields."
     "Preserve legacy complete/preflight behavior when new fields are absent."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]

  :commit
    (:required true
     :message "feat(execution): record task run verification status"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New fields."
     "Enforcement conditions."
     "Legacy compatibility notes."
     "Acceptance command results."])
