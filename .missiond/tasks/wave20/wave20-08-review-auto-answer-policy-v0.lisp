;; Wave 20 task contract.

(task wave20-08-review-auto-answer-policy-v0
  :schema "missiond.task-contract.v1"
  :title "Review auto-answer policy v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :goal "Implement a safe, explicit auto-answer policy for review questions where deterministic safety rules can answer non-destructive questions without human intervention."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-core/src/event/events/execution.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Add explicit auto_answer_policy with modes off|deterministic_safe|dry_run; default off."
     "Never auto-reject and never auto-approve destructive/archive/supersede/remove actions."
     "Return policy_result, selected_decision, safety_rule_results, and requires_human when skipped."
     "Wire unified_entry to surface the policy in dry-run smoke paths without adding a new MCP tool."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::review_gate::tests"
     "cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/review_gate.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]

  :commit
    (:required true
     :message "feat(review): add deterministic auto-answer policy"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Policy modes."
     "Safety rules."
     "Non-destructive boundaries."
     "Acceptance command results."])
