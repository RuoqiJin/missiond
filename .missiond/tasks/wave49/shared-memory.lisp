(shared-memory wave49
  :schema "missiond.shared-memory.v1"
  :wave wave49
  :created-at "2026-04-29T06:30:00Z"
  :sequence 4

  (observation
    :id wave49-bootstrap-001
    :task wave49-bootstrap
    :agent codex-parent
    :seq 1
    :at "2026-04-29T06:30:00Z"
    :touched [".missiond/tasks/wave49/manifest.lisp"
              ".missiond/tasks/wave49/wave49-01-request-flow-restart-recovery-smoke-v0.lisp"]
    :summary "Wave49 prepared to implement the wave48 accepted recovery-smoke shard. Only scripts/check-v3-request-flow-smoke.mjs should be edited by the worker.")

  (observation
    :id wave49-bootstrap-002
    :task wave49-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-29T06:29:31Z"
    :touched [".missiond/claudecode/wave49-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded.")

  (claim
    :id wave49-01-claim-001
    :task wave49-01-request-flow-restart-recovery-smoke-v0
    :agent claudecode-wave49-01
    :seq 3
    :at "2026-04-29T07:10:00Z"
    :summary "Claiming wave49-01: implement the wave48-accepted recovery-smoke shard. Edits limited to scripts/check-v3-request-flow-smoke.mjs. Will add CLI flag --restart-during-dispatch (only valid with --live-ipc + --execute-real-dispatch), restart-recovery step planner, and dry-fixture coverage for default behavior, invalid flag combos, and the planned restart-recovery sequence. Live daemon restart NOT executed; parent must opt-in explicitly.")

  (completion
    :id wave49-01-completion-001
    :task wave49-01-request-flow-restart-recovery-smoke-v0
    :agent claudecode-wave49-01
    :seq 4
    :at "2026-04-29T07:35:00Z"
    :touched ["scripts/check-v3-request-flow-smoke.mjs"
              ".missiond/tasks/wave49/shared-memory.lisp"
              ".missiond/tasks/wave49/session-trace.lisp"
              ".missiond/tasks/wave49/reports/wave49-01-request-flow-restart-recovery-smoke-v0.report.lisp"]
    :summary "Done. Single-file change to scripts/check-v3-request-flow-smoke.mjs: added validateOpts (rejects --restart-during-dispatch unless --live-ipc + --execute-real-dispatch are also present), buildRestartRecoveryPlan (5-step structured plan with parent_run_command), runRestartRecoveryFixtures (6 validation cases + 1 plan-structural case, total fixtures grew 9 -> 16), and an opt-in restart_recovery_plan step inside runLiveIpcSmoke that emits the plan but never kills the daemon. All 5 acceptance commands pass: --dry-fixture, default static+fixture, aggregate v3 gate, check-task-report, git diff --check. Live restart execution intentionally deferred to the parent per the contract's 'Do not run a live daemon restart unless the parent explicitly asks after review.'"))
