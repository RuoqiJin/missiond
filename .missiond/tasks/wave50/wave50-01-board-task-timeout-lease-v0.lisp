;; Wave 50 task contract.

(task wave50-01-board-task-timeout-lease-v0
  :schema "missiond.task-contract.v1"
  :title "derive BoardTask claim lease from timeout_secs"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 45
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave50/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave50/pattern-cards.lisp"
  :context-pack-path ".missiond/tasks/wave50/context-pack.lisp"
  :goal "Make Autopilot BoardTask claim leases derive from BoardTask.timeout_secs instead of the current fixed 20-minute lease. Keep pty.send budget, watchdog threshold, and claim lease aligned through V3 Lisp/code isomorphism."

  :write-scope
    ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
     ".missiond/v3/missiond-blueprint.lisp"
     "scripts/check-v3-workstation-config-isomorphism.mjs"
     ".missiond/tasks/wave50/shared-memory.lisp"
     ".missiond/tasks/wave50/session-trace.lisp"
     ".missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp"]

  :must-not-touch
    ["packages/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave48/**"
     ".missiond/tasks/wave49/**"
     ".missiond/tasks/wave50/manifest.lisp"
     ".missiond/tasks/wave50/wave50-*.lisp"
     ".missiond/tasks/wave50/context-atlas.lisp"
     ".missiond/tasks/wave50/pattern-cards.lisp"
     ".missiond/tasks/wave50/context-pack.lisp"
     ".missiond/claudecode/**"
     "scripts/check-context-pack.mjs"
     "scripts/context-pack-append.mjs"
     "scripts/context-pack-compile-shards.mjs"
     "scripts/check-v3-context-pack-isomorphism.mjs"]

  :requirements
    ["Read the shared preamble, this task contract, context atlas, pattern cards, and the wave50 context-pack integration-plan before broad scans."
     "Use scripts/context-pack-compile-shards.mjs .missiond/tasks/wave50/context-pack.lisp to confirm this is the accepted mapped shard."
     "Replace the fixed TimeDelta::minutes(20) BoardTask claim lease in dispatch_board_tasks with a timeout-derived helper."
     "Prefer deriving the lease from idle_watchdog_threshold_secs(timeout_secs), so explicit 3300s tasks receive a 3420s lease."
     "Add pure helper tests near existing pty_timeout / idle_watchdog tests."
     "Update .missiond/v3/missiond-blueprint.lisp and scripts/check-v3-workstation-config-isomorphism.mjs so the invariant is pinned."
     "Write the task report and commit only the declared write scope."]

  :acceptance
    ["node scripts/check-v3-workstation-config-isomorphism.mjs --dry-fixture"
     "node scripts/check-v3-workstation-config-isomorphism.mjs"
     "node scripts/check-v3-code-isomorphism-complete.mjs"
     "cargo check -p missiond-daemon"
     "cargo test -p missiond-daemon engine::intent_engine::autopilot::tests -- --nocapture"
     "node scripts/check-task-report.mjs .missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp"
     "git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp scripts/check-v3-workstation-config-isomorphism.mjs .missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp"]

  :commit
    (:required true
     :message "fix(autopilot): derive board task lease from timeout"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Helper name and exact timeout/lease semantics."
     "Which V3 blueprint/checker invariant was updated."
     "Acceptance command results."])
