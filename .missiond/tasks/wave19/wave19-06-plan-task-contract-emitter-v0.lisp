;; Wave 19 task contract.

(task wave19-06-plan-task-contract-emitter-v0
  :schema "missiond.task-contract.v1"
  :title "Plan runner task-contract emitter v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave19-02-task-contract-verifier-v1" "wave19-05-renderer-dispatch-brief-v1"]
  :dispatch-strategy "agent-team"
  :goal "Teach mission_plan execute paths to emit task.lisp contracts for workstation-dispatchable nodes before rendering Markdown, making Lisp the dispatch SSOT."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Use agent-team if useful: 使用 agent-team提高效率."
     "Add an opt-in execute argument such as emit_task_contract or task_contract_mode; default must preserve current behavior byte-compatible where possible."
     "For eligible mission_task_delegate/workstation nodes, write .missiond/tasks/generated/<plan_id>/<node_id>.lisp using task-contract v1 fields derived from PLAN node hints: goal/objective, owned files, forbidden files, acceptance, commit policy, dispatch strategy."
     "Return task_contract_path and rendered_task_brief_path or render_command in the response; do not require Node from Rust unless an existing project pattern already does so safely."
     "Never parse arbitrary Lisp beyond existing PLAN hint parser boundaries."
     "On task-contract write failure after inner dispatch is not allowed; emit contract before dispatch or dry-run only."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-task-contract.mjs --all"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(plan): emit Lisp task contracts for dispatch"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New execute arguments and response fields."
     "Generated task path convention."
     "Compatibility boundaries."
     "Acceptance command results."])
