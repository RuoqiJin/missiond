;; Wave 21 task contract.

(task wave21-04-autonomous-workstation-llm-proposal-v0
  :schema "missiond.task-contract.v1"
  :title "Autonomous workstation LLM proposal v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave21-00-archive-wave20-task-artifacts"]
  :dispatch-strategy "agent-team"
  :goal "Add an explicit Sonnet proposal mode for fully autonomous workstation spawn decisions when PLAN hints are absent, while keeping execution suggest-only in v0."

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
     "Add explicit mode such as workstation_inference_mode=\"sonnet_suggest\"; default deterministic behavior unchanged."
     "When PLAN hints are absent, ask Sonnet for dispatch target/strategy/objective/scope proposal only; never spawn automatically in v0."
     "Validate LLM output into a proposal object with field, value, confidence, evidence, and safety_status."
     "If Sonnet unavailable, return LLM_UNAVAILABLE and do not fall back to prompt mode or claude -p."
     "Surface proposals in mission_plan execute responses for operator review."]

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
     :message "feat(workstation): propose autonomous dispatch with Sonnet"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Mode name."
     "Proposal schema."
     "Non-spawn boundary proof."
     "Acceptance command results."])
