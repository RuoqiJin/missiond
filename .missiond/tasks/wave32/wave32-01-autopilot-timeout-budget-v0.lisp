;; Wave 32 task contract.

(task wave32-01-autopilot-timeout-budget-v0
  :schema "missiond.task-contract.v1"
  :title "Autopilot PTY timeout budget alignment v0"
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
  :context-atlas-path ".missiond/tasks/wave32/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave32/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Fix the wave31 stability issue where Autopilot sent a ClaudeCode task with a fixed 10 minute pty.send timeout even though mission_task_delegate had already stored a longer task timeout_secs on the board task."

  :write-scope
    ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
     ".missiond/v3/missiond-blueprint.lisp"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
     "crates/missiond-core/src/types/board.rs"
     "crates/missiond-core/src/db/pg/board.rs"
     "crates/missiond-pty/**"
     "crates/missiond-mcp/**"
     "scripts/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/tasks/schema/**"
     ".missiond/tasks/wave31/**"
     ".missiond/tasks/wave32/manifest.lisp"
     ".missiond/tasks/wave32/dispatch-plan.lisp"
     ".missiond/tasks/wave32/context-atlas.lisp"
     ".missiond/tasks/wave32/pattern-cards.lisp"
     ".missiond/tasks/wave32/wave32-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Replace Autopilot's fixed `let timeout_ms = 600_000` PTY send budget with a helper derived from BoardTask.timeout_secs. If timeout_secs is absent or invalid, use the existing task_delegate default of 1800 seconds. Clamp to a sane 60..7200 second range before converting to milliseconds."
     "Update the smart watchdog that currently unclaims idle running tasks after claimed_age > 120s. It must wait until the task timeout plus a small grace window has elapsed before treating an idle slot as orphaned. This prevents long-running Opus coding tasks from being re-dispatched while their original pty.send is still within the declared task timeout."
     "Keep the no-PTY-session branch recoverable without waiting for the full timeout, because a missing session is different from an idle session that may still be returning a result."
     "Improve watchdog note/log wording so it says the task exceeded its configured timeout/grace, not only that daemon restart may have lost send()."
     "Add focused pure unit tests in autopilot.rs for timeout derivation and watchdog threshold behavior. Do not construct AppState in tests."
     "Update .missiond/v3/missiond-blueprint.lisp workstation-config invariants or implementation-map note to record that Autopilot wait budget and watchdog recovery are Lisp/task-timeout projected policy, not hardcoded runtime constants."]

  :acceptance
    ["cargo test -p missiond-daemon engine::intent_engine::autopilot::tests"
     "cargo check -p missiond-daemon"
     "node scripts/check-lisp-blueprint-compression.mjs"
     "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
     "perl -ne 'exit 1 if /\\x00/' crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"
     "git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"]

  :commit
    (:required true
     :message "fix(autopilot): honor task timeout budget"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Timeout derivation policy."
     "Watchdog recovery threshold policy."
     "Whether blueprint invariant/note changed."
     "Acceptance command results."])
