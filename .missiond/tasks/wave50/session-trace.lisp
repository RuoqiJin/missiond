(session-trace wave50
  :schema "missiond.session-trace.v1"
  :wave wave50
  :created-at "2026-04-29T08:05:00Z"
  :sequence 5

  (trace-event
    :id wave50-bootstrap-start-001
    :seq 1
    :at "2026-04-29T08:05:00Z"
    :task wave50-bootstrap
    :backend codex
    :kind dispatch
    :files [".missiond/tasks/wave50/manifest.lisp"
            ".missiond/tasks/wave50/context-pack.lisp"
            ".missiond/tasks/wave50/wave50-01-board-task-timeout-lease-v0.lisp"]
    :summary "Bootstrap wave50 timeout-derived BoardTask lease implementation wave.")

  (trace-event
    :id wave50-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T07:00:28Z"
    :task wave50-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave50-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T07:00:28Z"
    :task wave50-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave50-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.")

  (trace-event
    :id wave50-01-preamble-read-001
    :seq 4
    :at "2026-04-29T08:30:00Z"
    :task wave50-01-board-task-timeout-lease-v0
    :backend claudecode
    :kind read
    :files [".missiond/claudecode/wave50-shared-preamble.md"
            ".missiond/tasks/wave50/wave50-01-board-task-timeout-lease-v0.lisp"
            ".missiond/tasks/wave50/manifest.lisp"
            ".missiond/tasks/wave50/context-atlas.lisp"
            ".missiond/tasks/wave50/pattern-cards.lisp"
            ".missiond/tasks/wave50/context-pack.lisp"]
    :summary "Loaded shared preamble + task contract + manifest + context atlas + pattern cards + context-pack integration-plan, plus context-pack-compile-shards verifier (1 shard, 1 group, mapped) before broad scans (audit pin per preamble protocol).")

  (trace-event
    :id wave50-01-completion-trace-002
    :seq 5
    :at "2026-04-29T08:55:00Z"
    :task wave50-01-board-task-timeout-lease-v0
    :backend claudecode
    :kind observation
    :files ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
            ".missiond/v3/missiond-blueprint.lisp"
            "scripts/check-v3-workstation-config-isomorphism.mjs"
            ".missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp"]
    :summary "Wave50-01 done: derive_board_task_lease_secs added, fixed 20-minute lease replaced, 7 new helper tests pass, V3 invariant pinned in blueprint + workstation-config checker. All 7 acceptance commands green; aggregate V3 gate (7 surfaces, 8 checkers) still ok; cargo test reports 31 autopilot tests passing."))
