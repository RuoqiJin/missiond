;; Wave 27 task contract.

(task wave27-06-router-dispatch-descriptor-smoke-v0
  :schema "missiond.task-contract.v1"
  :title "Router dispatch descriptor smoke v0"
  :kind smoke
  :status ready
  :owner "claudecode"
  :depends-on ["wave27-02-router-dispatch-descriptor-cli-v0"
               "wave27-03-plan-router-dispatch-descriptor-surface-v0"
               "wave27-04-report-router-dispatch-descriptor-fields-v0"
               "wave27-05-renderer-router-dispatch-descriptor-context-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Add cross-layer smoke coverage proving router dispatch descriptors are machine-checkable handoffs and still cannot execute or switch backend."

  :write-scope
    ["scripts/check-router-dispatch-descriptor.mjs"
     "scripts/build-router-dispatch-descriptor.mjs"
     "scripts/check-task-report.mjs"
     "scripts/render-claudecode-task.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :must-not-touch
    [".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/wave27/wave27-*.lisp"
     ".missiond/claudecode/**"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]

  :requirements
    ["Layer A: check-router-dispatch-descriptor dry-fixtures pin valid blocked seed descriptor, valid synthetic runtime-ready eligible descriptor, no_execution false rejection, runtime_replacement true rejection, and current-default eligible rejection."
     "Layer B: build-router-dispatch-descriptor dry-fixtures prove seed registry yields router_apply_eligible=false and synthetic runtime-ready yields true while no_execution remains true in both."
     "Layer C: mission_plan tests prove Rust descriptor surface agrees with CLI on seed current-default registry and off/default remains no-I/O."
     "Layer D: report checker fixture validates all descriptor report fields and rejects string booleans."
     "Layer E: renderer fixture asserts advisory / dry-run only / no execution / MUST NOT switch backend / build-router-dispatch-descriptor literals."
     "Static audit must prove no new shell/child_process/git/network/LLM calls in descriptor advisory path; assemble forbidden strings from parts to avoid self-match."
     "Do not change MCP schema in this smoke task unless required by a compile fix; if required, document why."]

  :acceptance
    ["node scripts/check-router-dispatch-descriptor.mjs --dry-fixture"
     "node scripts/build-router-dispatch-descriptor.mjs --dry-fixture"
     "node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/render-claudecode-task.mjs --dry-fixture"
     "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/check-router-dispatch-descriptor.mjs scripts/build-router-dispatch-descriptor.mjs scripts/check-task-report.mjs scripts/render-claudecode-task.mjs crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :commit
    (:required true
     :message "test(router): smoke dispatch descriptor chain"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Layer-by-layer invariant list."
     "Test deltas."
     "Acceptance command results."])
