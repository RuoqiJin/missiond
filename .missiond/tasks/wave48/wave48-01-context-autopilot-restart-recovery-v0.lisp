;; Wave 48 task contract.

(task wave48-01-context-autopilot-restart-recovery-v0
  :schema "missiond.task-contract.v1"
  :title "context-pack investigation: autopilot dynamic slot restart recovery"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 35
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave48/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave48/pattern-cards.lisp"
  :goal "Read-only investigation for the next implementation shard: analyze why a delegated BoardTask assigned to a dynamic slot can fail after daemon restart, then append concrete observations and at least one shard-proposal to .missiond/tasks/wave48/context-pack.lisp using scripts/context-pack-append.mjs. Do not edit Rust/JS implementation files in this task."

  :write-scope
    [".missiond/tasks/wave48/context-pack.lisp"
     ".missiond/tasks/wave48/shared-memory.lisp"
     ".missiond/tasks/wave48/session-trace.lisp"
     ".missiond/tasks/wave48/reports/wave48-01-context-autopilot-restart-recovery-v0.report.lisp"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     "packages/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/v3/**"
     ".missiond/research/**"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/**"
     ".missiond/tasks/wave30/**"
     ".missiond/tasks/wave31/**"
     ".missiond/tasks/wave32/**"
     ".missiond/tasks/wave33/**"
     ".missiond/tasks/wave34/**"
     ".missiond/tasks/wave35/**"
     ".missiond/tasks/wave36/**"
     ".missiond/tasks/wave37/**"
     ".missiond/tasks/wave38/**"
     ".missiond/tasks/wave39/**"
     ".missiond/tasks/wave40/**"
     ".missiond/tasks/wave41/**"
     ".missiond/tasks/wave42/**"
     ".missiond/tasks/wave43/**"
     ".missiond/tasks/wave44/**"
     ".missiond/tasks/wave45/**"
     ".missiond/tasks/wave46/**"
     ".missiond/tasks/wave47/**"
     ".missiond/tasks/wave48/manifest.lisp"
     ".missiond/tasks/wave48/wave48-*.lisp"
     ".missiond/tasks/wave48/context-atlas.lisp"
     ".missiond/tasks/wave48/pattern-cards.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Read the shared preamble, this task contract, manifest, context atlas, and pattern cards before broad search."
     "Inspect only: crates/missiond-daemon/src/engine/intent_engine/autopilot.rs, crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs, crates/missiond-daemon/src/handlers/compute/task_delegate.rs, crates/missiond-daemon/src/handlers/compute/compute_slot.rs, and the V3 workstation-config/context-pack blueprint sections."
     "Use scripts/context-pack-append.mjs to append at least two observations to .missiond/tasks/wave48/context-pack.lisp. Each observation must cite concrete files and explain the restart failure path."
     "Use scripts/context-pack-append.mjs to append at least one shard-proposal. The proposal must identify a single code owner, a bounded write-scope, must-not-touch scope, and acceptance commands for implementing dynamic slot restart recovery."
     "Do not edit Rust, JS, package, or blueprint files in this task. This is an investigation/proposal shard only."
     "Write the task report and commit only the declared write scope."]

  :acceptance
    ["node scripts/check-context-pack.mjs .missiond/tasks/wave48/context-pack.lisp"
     "node scripts/check-task-report.mjs .missiond/tasks/wave48/reports/wave48-01-context-autopilot-restart-recovery-v0.report.lisp"
     "git diff --check -- .missiond/tasks/wave48/context-pack.lisp .missiond/tasks/wave48/reports/wave48-01-context-autopilot-restart-recovery-v0.report.lisp"]

  :commit
    (:required true
     :message "chore(tasks): record wave48 autopilot recovery context"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Which dynamic-slot restart failure path was identified."
     "Context-pack entries appended, including shard-proposal id."
     "Recommended implementation write-scope and acceptance commands."
     "Acceptance command results."])
