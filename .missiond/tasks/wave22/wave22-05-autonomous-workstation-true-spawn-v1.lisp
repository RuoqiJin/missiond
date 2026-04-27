;; Wave 22 task contract.

(task wave22-05-autonomous-workstation-true-spawn-v1
  :schema "missiond.task-contract.v1"
  :title "Autonomous workstation true spawn v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave21-04-autonomous-workstation-llm-proposal-v0"
               "wave22-02-execution-auto-run-verifier-v2"]
  :dispatch-strategy "agent-team"
  :goal "Promote autonomous workstation proposals to real mission_task_delegate dispatch only under explicit auto_spawn gate and verified task-contract scope."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Use agent-team if useful: 使用 agent-team提高效率."
     "Add explicit auto_spawn=true gate; default remains proposal-only."
     "Require proposal_hash match, high confidence, validated task_contract_path, non-empty write-scope, hooks/preflight status acceptable, and no forbidden scope overlap."
     "Dispatch only through mission_task_delegate / workstation substrate; never claude -p."
     "If any gate fails, return SafeDescriptor-style structured failure and do not spawn."
     "Return auto_spawn_status, spawn_target, task_contract_path, proposal_hash_status, and gate_results."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests"
     "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(workstation): gate autonomous task dispatch"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "auto_spawn gate matrix."
     "Substrate path proof."
     "No claude -p fallback proof."
     "Acceptance command results."])
