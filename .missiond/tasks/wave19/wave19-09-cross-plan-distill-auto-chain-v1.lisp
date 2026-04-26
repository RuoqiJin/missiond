;; Wave 19 task contract.

(task wave19-09-cross-plan-distill-auto-chain-v1
  :schema "missiond.task-contract.v1"
  :title "Cross-plan distill auto-chain v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :goal "Remove the explicit chain_id requirement for safe cross-plan distill chain recording by deriving a deterministic chain id from plan/workflow context."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Add an opt-in auto_chain mode for cross-plan distill chain recording; default must preserve current explicit behavior."
     "Derive deterministic chain_id from project root, plan_id, workflow name/id, and source evidence hash where available."
     "Never call Sonnet implicitly; keep sonnet only explicit."
     "Persist sidecar append-only; do not add migrations."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::workflow::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs"]

  :commit
    (:required true
     :message "feat(workflow): derive cross-plan distill chain ids"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Derived chain id inputs."
     "Backward compatibility notes."
     "Acceptance command results."])
