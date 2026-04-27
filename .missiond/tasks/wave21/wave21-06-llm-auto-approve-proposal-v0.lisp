;; Wave 21 task contract.

(task wave21-06-llm-auto-approve-proposal-v0
  :schema "missiond.task-contract.v1"
  :title "LLM auto-approve proposal v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave20-08-review-auto-answer-policy-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Add explicit Sonnet-assisted auto-approve proposal mode for directive/plan/review gates, without applying approvals automatically in v0."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
     "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/directive.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Add explicit mode such as auto_approve_mode=\"sonnet_suggest\"; default off."
     "LLM proposal must include decision, confidence, evidence, non_goal_check, destructive_check, and requires_human."
     "Never auto-reject and never auto-approve destructive/archive/supersede/remove actions."
     "If Sonnet unavailable, return LLM_UNAVAILABLE with no fallback."
     "Do not mutate directive/plan/review state in v0; response is proposal only."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::review_gate::tests"
     "cargo test -p missiond-daemon handlers::knowledge::directive::tests"
     "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/review_gate.rs crates/missiond-daemon/src/handlers/knowledge/directive.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/directive.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(review): propose LLM auto approvals"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Mode name."
     "Proposal schema."
     "No-mutation proof."
     "Acceptance command results."])
