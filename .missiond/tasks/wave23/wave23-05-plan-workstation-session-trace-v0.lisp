;; Wave 23 task contract.

(task wave23-05-plan-workstation-session-trace-v0
  :schema "missiond.task-contract.v1"
  :title "Plan/workstation session-trace propagation v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave23-04-execution-session-trace-integration-v0"]
  :dispatch-strategy "agent-team"
  :goal "Propagate session_trace_path through mission_plan and workstation_dispatch so generated task contracts, dispatch descriptors, and completion paths share one factual trace ledger."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Use agent-team if useful: 使用 agent-team提高效率."
     "Add optional session_trace_path forwarding in mission_plan execute paths."
     "When emitting task contracts, include trace path metadata or response fields so downstream completion can append to the same trace."
     "workstation_dispatch should include session_trace_path in descriptors/briefs when provided."
     "Do not make trace required; preserve legacy behavior when absent."
     "Malformed trace path should fail before dispatch when caller explicitly requires trace, otherwise return warning."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(plan): propagate session trace through dispatch"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Forwarded arguments/fields."
     "Trace-required behavior."
     "Compatibility notes."
     "Acceptance command results."])
