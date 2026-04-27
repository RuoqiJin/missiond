;; Wave 21 task contract.

(task wave21-07-sonnet-distill-chain-auto-apply-v1
  :schema "missiond.task-contract.v1"
  :title "Sonnet distill chain auto-apply v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave20-06-cross-plan-distill-auto-trigger-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Allow cross-plan distill chains to invoke Sonnet automatically only when all deterministic safety rules pass and the caller explicitly enables auto_sonnet."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Add explicit auto_sonnet=true gate; default keeps Sonnet explicit/manual."
     "Invoke Sonnet only after all deterministic safety rules pass."
     "If any rule fails, return skipped and do not call Sonnet."
     "If Sonnet returns invalid output, record structured failure and preserve existing plan/workflow state."
     "Return auto_sonnet_status, safety_rule_results, model_call_status, sidecar path, and review_required."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::workflow::tests"
     "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs"]

  :commit
    (:required true
     :message "feat(workflow): gate Sonnet distill chain auto-apply"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "auto_sonnet gate."
     "Safety rules."
     "State-preservation behavior."
     "Acceptance command results."])
