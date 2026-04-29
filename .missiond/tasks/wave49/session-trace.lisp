(session-trace wave49
  :schema "missiond.session-trace.v1"
  :wave wave49
  :created-at "2026-04-29T06:30:00Z"
  :sequence 5

  (trace-event
    :id wave49-bootstrap-start-001
    :seq 1
    :at "2026-04-29T06:30:00Z"
    :task wave49-bootstrap
    :backend codex
    :kind dispatch
    :files [".missiond/tasks/wave49/manifest.lisp"
            ".missiond/tasks/wave49/wave49-01-request-flow-restart-recovery-smoke-v0.lisp"]
    :summary "Bootstrap wave49 restart-recovery smoke implementation wave.")

  (trace-event
    :id wave49-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T06:29:31Z"
    :task wave49-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave49-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T06:29:31Z"
    :task wave49-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave49-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.")

  (trace-event
    :id wave49-01-preamble-read-001
    :seq 4
    :at "2026-04-29T07:10:00Z"
    :task wave49-01-request-flow-restart-recovery-smoke-v0
    :backend claudecode
    :kind read
    :files [".missiond/claudecode/wave49-shared-preamble.md"
            ".missiond/tasks/wave49/wave49-01-request-flow-restart-recovery-smoke-v0.lisp"
            ".missiond/tasks/wave49/manifest.lisp"
            ".missiond/tasks/wave49/context-atlas.lisp"
            ".missiond/tasks/wave49/pattern-cards.lisp"
            ".missiond/tasks/wave48/context-pack.lisp"]
    :summary "Loaded shared preamble + task contract + manifest + context atlas + pattern cards + wave48 context-pack integration-plan before broad scans (audit pin per preamble protocol).")

  (trace-event
    :id wave49-01-completion-trace-002
    :seq 5
    :at "2026-04-29T07:35:00Z"
    :task wave49-01-request-flow-restart-recovery-smoke-v0
    :backend claudecode
    :kind observation
    :files ["scripts/check-v3-request-flow-smoke.mjs"
            ".missiond/tasks/wave49/shared-memory.lisp"
            ".missiond/tasks/wave49/reports/wave49-01-request-flow-restart-recovery-smoke-v0.report.lisp"]
    :summary "Wave49-01 done: single-file change to scripts/check-v3-request-flow-smoke.mjs adds validateOpts + buildRestartRecoveryPlan + runRestartRecoveryFixtures + opt-in restart_recovery_plan step inside runLiveIpcSmoke. All 5 acceptance commands pass; aggregate v3 gate (8 per-surface checkers) still ok; total fixtures 9 -> 16. Live daemon restart intentionally not executed."))
