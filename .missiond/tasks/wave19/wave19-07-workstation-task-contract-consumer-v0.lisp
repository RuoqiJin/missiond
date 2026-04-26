;; Wave 19 task contract.

(task wave19-07-workstation-task-contract-consumer-v0
  :schema "missiond.task-contract.v1"
  :title "Workstation task-contract consumer v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave19-05-renderer-dispatch-brief-v1"]
  :dispatch-strategy "agent-team"
  :goal "Teach workstation_dispatch to prefer a task.lisp contract when one exists, and render a ClaudeCode brief from contract fields rather than re-inventing natural-language instructions."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Use agent-team if useful: 使用 agent-team提高效率."
     "Add a pure parser/loader for the narrow task-contract v1 fields needed by workstation_dispatch, or reuse existing in-Rust Lisp helpers if present."
     "Accept optional task_contract_path in the internal descriptor; when present, build the task brief from the contract and preserve existing scoped-commit handoff section."
     "Keep legacy objective/owned-files path unchanged when task_contract_path is absent."
     "If contract is malformed, return SafeDescriptor-style structured failure and do not fall back to claude -p."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]

  :commit
    (:required true
     :message "feat(workstation): consume Lisp task contracts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Contract fields consumed."
     "Malformed-contract behavior."
     "Compatibility boundaries."
     "Acceptance command results."])
