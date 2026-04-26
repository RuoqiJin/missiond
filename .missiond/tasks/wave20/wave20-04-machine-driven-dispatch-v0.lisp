;; Wave 20 task contract.

(task wave20-04-machine-driven-dispatch-v0
  :schema "missiond.task-contract.v1"
  :title "Machine-driven task contract dispatch v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave20-02-renderer-scoped-commit-guard-v2"
               "wave20-03-execution-preflight-contract-scope-v1"]
  :dispatch-strategy "agent-team"
  :goal "Add an internal dispatch mode where MissionD hands task.lisp directly to workstation_dispatch and treats Markdown as optional compatibility output, not the load-bearing contract."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-core/src/event/events/execution.rs"
     "scripts/**"
     ".missiond/v2/*.lisp"]

  :requirements
    ["Use agent-team if useful: 使用 agent-team提高效率."
     "Add an opt-in mode such as dispatch_contract_mode=\"machine\" or render_markdown=false; default remains current rendered brief behavior."
     "When machine mode is enabled and task_contract_path exists, workstation_dispatch must consume task.lisp directly and include task_contract_path in the returned descriptor."
     "Do not require Markdown generation for machine mode; if a rendered path exists, surface it as compatibility metadata only."
     "Malformed contract must return SafeDescriptor-style failure and must not fall back to claude -p or unscoped prompt mode."
     "Update unified_entry forwarding so the mode can be passed through the unified entry pipeline."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests"
     "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests"
     "cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-task-contract.mjs --all"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(plan): dispatch directly from Lisp task contracts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New mode/argument names."
     "Machine-mode response fields."
     "Fallback behavior."
     "Acceptance command results."])
