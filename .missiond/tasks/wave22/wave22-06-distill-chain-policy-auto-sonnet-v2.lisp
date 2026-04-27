;; Wave 22 task contract.

(task wave22-06-distill-chain-policy-auto-sonnet-v2
  :schema "missiond.task-contract.v1"
  :title "Distill chain policy auto-Sonnet v2"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave21-07-sonnet-distill-chain-auto-apply-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Replace the dual opt-in Sonnet distill chain gate with a single explicit policy gate that still requires all deterministic safety rules and review-required output."

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
    ["Add explicit auto_sonnet_policy such as off|safe_after_rules|dry_run; default off."
     "safe_after_rules must not require a second opt-in flag, but must require all deterministic safety rules to pass."
     "Every Sonnet-produced workflow/distill output must remain review_required=true; do not auto-approve workflow reuse."
     "If Sonnet unavailable or output invalid, return structured failure and preserve plan/workflow state."
     "Return policy_status, safety_rule_results, model_call_status, review_required, and sidecar path."]

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
     :message "feat(workflow): policy-gate automatic Sonnet distill"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Policy enum."
     "Safety rule carryover."
     "Review-required proof."
     "Acceptance command results."])
