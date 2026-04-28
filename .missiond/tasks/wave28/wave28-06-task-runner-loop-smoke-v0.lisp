;; Wave 28 task contract.

(task wave28-06-task-runner-loop-smoke-v0
  :schema "missiond.task-contract.v1"
  :title "Task runner loop smoke v0"
  :kind smoke
  :status ready
  :owner "claudecode"
  :depends-on ["wave28-02-task-runner-plan-cli-v0"
               "wave28-03-wave-brief-batch-renderer-v0"
               "wave28-04-mission-plan-task-runner-dry-run-surface-v0"
               "wave28-05-task-runner-batch-verifier-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier full
  :dispatch-group "D"
  :estimated-minutes 45
  :heartbeat-minutes 10
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Add a cross-layer smoke suite for task-contract runner v0. The smoke should prove manifest checker, runner-plan CLI, wave brief renderer, daemon dry-run surface, and batch verifier agree on the same productive-only wave semantics."

  :write-scope
    ["scripts/check-task-runner-manifest.mjs"
     "scripts/plan-task-runner.mjs"
     "scripts/render-wave-briefs.mjs"
     "scripts/verify-task-runner-batch.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"
     ".missiond/v2/**"
     ".missiond/tasks/wave27/**"
     ".missiond/tasks/wave28/wave28-*.lisp"
     ".missiond/tasks/wave28/dispatch-plan.lisp"
     ".missiond/claudecode/**"
     "scripts/check-task-contract.mjs"
     "scripts/render-claudecode-task.mjs"
     "scripts/verify-task-run.mjs"]

  :requirements
    ["Add smoke fixtures or tests that pin the same manifest through checker -> plan CLI -> render-wave-briefs -> mission_plan dry-run -> batch verifier."
     "Pin productive-only behavior: archive/backfill/index are not worker nodes and are absent from thin brief generation and batch verification."
     "Pin verification-tier behavior: local tasks do not require full cargo; full tier appears only on final smoke/final nodes."
     "Pin heartbeat metadata propagation into thin brief or shared preamble guidance."
     "Pin no execution: task-runner dry-run does not spawn, does not call Node, does not mutate git, and returns applied=false."
     "Keep the smoke deterministic and no LLM/no network."]

  :acceptance
    ["node scripts/check-task-runner-manifest.mjs --dry-fixture"
     "node scripts/plan-task-runner.mjs --dry-fixture"
     "node scripts/render-wave-briefs.mjs --dry-fixture"
     "node scripts/verify-task-runner-batch.mjs --dry-fixture"
     "cargo test -p missiond-daemon task_runner --lib"
     "cargo test -p missiond-daemon --lib"
     "cargo build --workspace"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/check-task-runner-manifest.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs scripts/verify-task-runner-batch.mjs crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :commit
    (:required true
     :message "test(tasks): smoke task runner loop"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Smoke layers pinned."
     "Productive-only and no-execution proof."
     "Acceptance command results."])
