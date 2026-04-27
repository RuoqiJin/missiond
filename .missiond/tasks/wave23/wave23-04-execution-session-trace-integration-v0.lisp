;; Wave 23 task contract.

(task wave23-04-execution-session-trace-integration-v0
  :schema "missiond.task-contract.v1"
  :title "Execution session-trace integration v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave23-01-session-trace-schema-v0"]
  :dispatch-strategy "agent-team"
  :goal "Add opt-in session-trace append support to mission_execution so dispatch/complete/preflight facts can be recorded by MissionD without relying on worker prose."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "scripts/**"
     ".missiond/v2/*.lisp"]

  :requirements
    ["Use agent-team if useful: 使用 agent-team提高效率."
     "Add optional session_trace_path argument to relevant mission_execution actions."
     "Append trace events for open/preflight_commit/complete when session_trace_path is supplied."
     "Trace append must be append-only, structured, and best-effort: report trace_warning on failure but do not hide core action result."
     "Do not spawn Node or shell scripts from daemon."
     "Preserve legacy behavior when session_trace_path is absent."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]

  :commit
    (:required true
     :message "feat(execution): append session trace events"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Actions that append trace."
     "Failure semantics."
     "Compatibility notes."
     "Acceptance command results."])
