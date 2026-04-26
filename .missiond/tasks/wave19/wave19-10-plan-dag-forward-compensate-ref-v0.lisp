;; Wave 19 task contract.

(task wave19-10-plan-dag-forward-compensate-ref-v0
  :schema "missiond.task-contract.v1"
  :title "PLAN DAG forward compensate ref v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :goal "Support forward :compensate-node references alongside existing reverse :compensates rollback semantics in the PLAN DAG parser and scheduler."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Parse :compensate-node or :compensate-ref as a forward reference from a failing node to its compensation node."
     "Reject self references and unknown compensation nodes with structured parser errors."
     "If both forward and reverse declarations exist, require them to agree; do not silently choose one."
     "Keep rollback safety gates from Wave 18 intact."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]

  :commit
    (:required true
     :message "feat(plan): support forward compensate node refs"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Accepted forward-ref keys."
     "Conflict and safety behavior."
     "Acceptance command results."])
