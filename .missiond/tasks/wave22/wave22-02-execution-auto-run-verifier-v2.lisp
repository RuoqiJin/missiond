;; Wave 22 task contract.

(task wave22-02-execution-auto-run-verifier-v2
  :schema "missiond.task-contract.v1"
  :title "Execution auto task-run verifier v2"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave21-03-execution-report-verifier-integration-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Remove the caller-supplied verified=true escape hatch by having mission_execution complete derive verification from task/report/memory/commit inputs when all paths are supplied."

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
    ["When task_contract_path, task_report_path, shared_memory_path, and commit_hash are all supplied, daemon should run its in-tree read-only verifier and compute verified status itself."
     "Keep caller-supplied verified=true only as legacy compatibility, and downgrade it to legacy_verified_claim in the response when full inputs are absent."
     "Return verification_source, verifier_status, verifier_diagnostics, and verified_scope_summary."
     "Do not invoke Node, shell scripts, or mutating git commands from daemon."
     "Preserve legacy complete behavior when the new fields are absent and enforce_scoped_commit=false."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]

  :commit
    (:required true
     :message "feat(execution): auto-verify task run completion"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Verifier source states."
     "Legacy verified=true behavior."
     "Read-only proof."
     "Acceptance command results."])
