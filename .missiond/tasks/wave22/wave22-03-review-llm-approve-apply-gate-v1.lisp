;; Wave 22 task contract.

(task wave22-03-review-llm-approve-apply-gate-v1
  :schema "missiond.task-contract.v1"
  :title "Review LLM approval apply gate v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave21-06-llm-auto-approve-proposal-v0"]
  :dispatch-strategy "agent-team"
  :goal "Add an explicit apply gate for LLM approval proposals so non-destructive approvals can be applied only with caller approval, proposal hash matching, and deterministic safety checks."

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
    ["Use agent-team if useful: 使用 agent-team提高效率."
     "Add explicit apply_llm_auto_approve=true or equivalent; default remains proposal-only."
     "Require proposal_hash, caller_approved=true, non-destructive action, high confidence, and deterministic destructive_check=false before applying."
     "Never auto-reject. Never apply archive/supersede/remove/destructive actions."
     "On mismatch or missing proposal hash, return structured error and do not mutate directive/plan/review state."
     "Return apply_status, applied_decision, proposal_hash_status, and safety_rule_results."]

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
     :message "feat(review): gate applying LLM approval proposals"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Apply gate fields."
     "Safety rule matrix."
     "No auto-reject/destructive proof."
     "Acceptance command results."])
