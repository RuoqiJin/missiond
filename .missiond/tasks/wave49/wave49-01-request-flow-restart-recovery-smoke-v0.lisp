;; Wave 49 task contract.

(task wave49-01-request-flow-restart-recovery-smoke-v0
  :schema "missiond.task-contract.v1"
  :title "implement request-flow restart recovery smoke"
  :kind test-fix
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 45
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave49/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave49/pattern-cards.lisp"
  :goal "Implement the accepted wave48 recovery-smoke shard in scripts/check-v3-request-flow-smoke.mjs. Add an opt-in --restart-during-dispatch mode that is valid only with --live-ipc --execute-real-dispatch, plus dry-fixture coverage that proves the parser/planner refuses unsafe combinations and preserves existing default behavior. Do not run a live daemon restart unless the parent explicitly asks after review."

  :write-scope
    ["scripts/check-v3-request-flow-smoke.mjs"
     ".missiond/tasks/wave49/shared-memory.lisp"
     ".missiond/tasks/wave49/session-trace.lisp"
     ".missiond/tasks/wave49/reports/wave49-01-request-flow-restart-recovery-smoke-v0.report.lisp"]

  :must-not-touch
    ["crates/**"
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
     ".missiond/tasks/wave48/**"
     ".missiond/tasks/wave49/manifest.lisp"
     ".missiond/tasks/wave49/wave49-*.lisp"
     ".missiond/tasks/wave49/context-atlas.lisp"
     ".missiond/tasks/wave49/pattern-cards.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Read the shared preamble, this task contract, context atlas, pattern cards, and wave48 context-pack integration-plan first."
     "Preserve default behavior: without --live-ipc the smoke stays read-only; with --live-ipc but without --execute-real-dispatch it still stops before dispatch."
     "Add a CLI flag --restart-during-dispatch that errors unless both --live-ipc and --execute-real-dispatch are present."
     "Implement the restart-recovery smoke as an explicit opt-in path. It should be structured so parent/Codex can review the steps before running it against a live daemon."
     "Dry-fixture coverage must include safe default behavior, invalid flag combinations, and a planned restart-recovery step sequence."
     "Keep the implementation localized to scripts/check-v3-request-flow-smoke.mjs; do not edit Rust, V3 blueprint/checkers, package files, or wave48 artifacts."
     "Write the task report and commit only the declared write scope."]

  :acceptance
    ["node scripts/check-v3-request-flow-smoke.mjs --dry-fixture"
     "node scripts/check-v3-request-flow-smoke.mjs"
     "node scripts/check-v3-code-isomorphism-complete.mjs"
     "node scripts/check-task-report.mjs .missiond/tasks/wave49/reports/wave49-01-request-flow-restart-recovery-smoke-v0.report.lisp"
     "git diff --check -- scripts/check-v3-request-flow-smoke.mjs .missiond/tasks/wave49/reports/wave49-01-request-flow-restart-recovery-smoke-v0.report.lisp"]

  :commit
    (:required true
     :message "test(v3): add restart recovery dispatch smoke"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Exactly what --restart-during-dispatch does and what remains parent-run/live-only."
     "Dry-fixture cases added."
     "Backward compatibility evidence for default and --live-ipc non-dispatch modes."
     "Acceptance command results."])
