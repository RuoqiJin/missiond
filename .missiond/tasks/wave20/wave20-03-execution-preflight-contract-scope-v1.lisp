;; Wave 20 task contract.

(task wave20-03-execution-preflight-contract-scope-v1
  :schema "missiond.task-contract.v1"
  :title "Execution preflight task-contract scope v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave20-01-task-scope-index-guard-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Teach mission_execution preflight_commit to use task_contract_path as the scope source, so daemon-side read-only preflight catches index pollution before completion."

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
    ["Add optional task_contract_path handling to action=preflight_commit."
     "Read only task-contract v1 fields needed for scope: :write-scope, :must-not-touch, :commit."
     "Compare git status / staged files against contract scope; do not run mutating git commands."
     "Return structured fields: task_contract_status, staged_out_of_scope, staged_forbidden, unstaged_in_scope, next_step."
     "Preserve legacy preflight behavior when task_contract_path is absent."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]

  :commit
    (:required true
     :message "feat(execution): preflight task contract scope"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New preflight fields."
     "Read-only git command proof."
     "Legacy compatibility notes."
     "Acceptance command results."])
