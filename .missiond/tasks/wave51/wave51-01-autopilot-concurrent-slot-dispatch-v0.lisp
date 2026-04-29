;; Wave 51 task contract.

(task wave51-01-autopilot-concurrent-slot-dispatch-v0
  :schema "missiond.task-contract.v1"
  :title "start Autopilot pty sends concurrently across slots"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 55
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave51/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave51/pattern-cards.lisp"
  :context-pack-path ".missiond/tasks/wave51/context-pack.lisp"
  :goal "Make Autopilot BoardTask dispatch start pty.send work concurrently across different slots in the same dispatch tick. Preserve same-slot exclusion by holding the per-slot dispatch guard across each send."

  :write-scope
    ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
     ".missiond/v3/missiond-blueprint.lisp"
     "scripts/check-v3-workstation-config-isomorphism.mjs"
     ".missiond/tasks/wave51/shared-memory.lisp"
     ".missiond/tasks/wave51/session-trace.lisp"
     ".missiond/tasks/wave51/reports/wave51-01-autopilot-concurrent-slot-dispatch-v0.report.lisp"]

  :must-not-touch
    ["packages/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave48/**"
     ".missiond/tasks/wave49/**"
     ".missiond/tasks/wave50/**"
     ".missiond/tasks/wave51/manifest.lisp"
     ".missiond/tasks/wave51/wave51-*.lisp"
     ".missiond/tasks/wave51/context-atlas.lisp"
     ".missiond/tasks/wave51/pattern-cards.lisp"
     ".missiond/tasks/wave51/context-pack.lisp"
     ".missiond/claudecode/**"
     "scripts/check-context-pack.mjs"
     "scripts/context-pack-append.mjs"
     "scripts/context-pack-compile-shards.mjs"
     "scripts/check-v3-context-pack-isomorphism.mjs"]

  :requirements
    ["Read the shared preamble, this task contract, context atlas, pattern cards, and the wave51 context-pack integration-plan before broad scans."
     "Use scripts/context-pack-compile-shards.mjs .missiond/tasks/wave51/context-pack.lisp to confirm this is the accepted mapped shard."
     "Fix dispatch_board_tasks so it does not await one slot's state.pty.send before starting sends for other ready tasks assigned to other slots in the same tick."
     "Preserve the per-slot dispatch guard across each individual state.pty.send call; same-slot work must remain exclusive."
     "Keep the existing close-owner behavior, auth/quota/failure paths, KB confidence feedback, deploy post-mortem trigger, prompt snapshot, and dispatch event semantics unless there is a compile-driven reason to factor them."
     "Add a focused regression guard near autopilot tests, preferably source/pure-level if a full AppState integration test would be too heavy."
     "Update .missiond/v3/missiond-blueprint.lisp and scripts/check-v3-workstation-config-isomorphism.mjs so the invariant is pinned."
     "Write the task report and commit only the declared write scope."]

  :acceptance
    ["node scripts/check-v3-workstation-config-isomorphism.mjs --dry-fixture"
     "node scripts/check-v3-workstation-config-isomorphism.mjs"
     "node scripts/check-v3-code-isomorphism-complete.mjs"
     "cargo check -p missiond-daemon"
     "cargo test -p missiond-daemon engine::intent_engine::autopilot::tests -- --nocapture"
     "node scripts/check-task-report.mjs .missiond/tasks/wave51/reports/wave51-01-autopilot-concurrent-slot-dispatch-v0.report.lisp"
     "git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp scripts/check-v3-workstation-config-isomorphism.mjs .missiond/tasks/wave51/reports/wave51-01-autopilot-concurrent-slot-dispatch-v0.report.lisp"]

  :commit
    (:required true
     :message "fix(autopilot): dispatch board tasks concurrently across slots"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Exact concurrency structure used and how the per-slot guard is held."
     "Which V3 blueprint/checker invariant was updated."
     "Acceptance command results."])
