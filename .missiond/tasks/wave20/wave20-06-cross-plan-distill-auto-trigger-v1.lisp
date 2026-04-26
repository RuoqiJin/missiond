;; Wave 20 task contract.

(task wave20-06-cross-plan-distill-auto-trigger-v1
  :schema "missiond.task-contract.v1"
  :title "Cross-plan distill auto-trigger v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :goal "Trigger cross-plan distill chain recording automatically when deterministic safety rules pass, while keeping Sonnet calls explicit."

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
    ["Add opt-in auto_trigger mode for cross-plan distill chain sidecar recording."
     "Use deterministic safety rules only; do not call Sonnet unless the existing explicit sonnet mode is requested."
     "Return trigger_status, chain_id, safety_rule_results, and sidecar path."
     "If any safety rule fails, return skipped with rule evidence; do not partially append."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::workflow::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs"]

  :commit
    (:required true
     :message "feat(workflow): auto-trigger safe distill chains"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Safety rules."
     "Response fields."
     "Sonnet non-implicit proof."
     "Acceptance command results."])
