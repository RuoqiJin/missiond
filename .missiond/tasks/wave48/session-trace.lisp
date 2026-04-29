;; Wave 48 session trace.

(session-trace wave48
  :schema "missiond.session-trace.v1"
  :wave wave48
  :created-at "2026-04-29T06:01:00Z"
  :sequence 8

  (trace-event
    :id wave48-bootstrap-start-001
    :seq 1
    :at "2026-04-29T06:01:00Z"
    :task wave48-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave48/manifest.lisp"
            ".missiond/tasks/wave48/context-pack.lisp"
            ".missiond/tasks/wave48/context-atlas.lisp"
            ".missiond/tasks/wave48/pattern-cards.lisp"]
    :summary "Prepared wave48 for parallel context-pack investigation before code-shard implementation.")

  (trace-event
    :id wave48-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T06:03:41Z"
    :task wave48-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave48-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T06:03:41Z"
    :task wave48-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave48-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.")

  (trace-event
    :id wave48-01-preamble-read-001
    :seq 4
    :at "2026-04-29T06:30:00Z"
    :task wave48-01-context-autopilot-restart-recovery-v0
    :backend claudecode
    :kind read
    :files [".missiond/claudecode/wave48-shared-preamble.md"
            ".missiond/tasks/wave48/wave48-01-context-autopilot-restart-recovery-v0.lisp"
            ".missiond/tasks/wave48/manifest.lisp"
            ".missiond/tasks/wave48/context-atlas.lisp"
            ".missiond/tasks/wave48/pattern-cards.lisp"]
    :summary "Loaded shared preamble + task contract + manifest + context atlas + pattern cards before broad scans (audit pin per preamble protocol).")

  (trace-event
    :id wave48-01-investigation-read-002
    :seq 5
    :at "2026-04-29T06:38:00Z"
    :task wave48-01-context-autopilot-restart-recovery-v0
    :backend claudecode
    :kind read
    :files ["crates/missiond-daemon/src/main.rs"
            "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
            "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
            "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
            "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
            "crates/missiond-core/src/db/pg/board.rs"
            ".missiond/v3/missiond-blueprint.lisp"]
    :summary "Read-only inspection of dispatch + slot-bootstrap + dynamic-slot lifecycle + recover_stale_running_tasks SQL to confirm the failure path.")

  (trace-event
    :id wave48-02-preamble-read-001
    :seq 6
    :at "2026-04-29T06:55:00Z"
    :task wave48-02-context-dispatch-shard-plan-v0
    :backend claudecode
    :kind read
    :files [".missiond/claudecode/wave48-shared-preamble.md"
            ".missiond/tasks/wave48/wave48-02-context-dispatch-shard-plan-v0.lisp"
            ".missiond/tasks/wave48/manifest.lisp"
            ".missiond/tasks/wave48/context-atlas.lisp"
            ".missiond/tasks/wave48/pattern-cards.lisp"
            ".missiond/tasks/wave48/context-pack.lisp"
            ".missiond/tasks/wave48/shared-memory.lisp"]
    :summary "Loaded shared preamble + task contract + manifest + context atlas + pattern cards + existing context-pack + shared-memory before broad scans (audit pin per preamble protocol).")

  (trace-event
    :id wave48-01-completion-trace-003
    :seq 7
    :at "2026-04-29T06:58:00Z"
    :task wave48-01-context-autopilot-restart-recovery-v0
    :backend claudecode
    :kind observation
    :files [".missiond/tasks/wave48/context-pack.lisp"
            ".missiond/tasks/wave48/shared-memory.lisp"
            ".missiond/tasks/wave48/reports/wave48-01-context-autopilot-restart-recovery-v0.report.lisp"]
    :summary "Wave48-01 done: context-pack appended (2 observations + 1 shard-proposal), report status=done, all 5 acceptance commands passed.")

  (trace-event
    :id wave48-02-completion-trace-002
    :seq 8
    :at "2026-04-29T07:00:00Z"
    :task wave48-02-context-dispatch-shard-plan-v0
    :backend claudecode
    :kind observation
    :files [".missiond/tasks/wave48/context-pack.lisp"
            ".missiond/tasks/wave48/shared-memory.lisp"
            ".missiond/tasks/wave48/reports/wave48-02-context-dispatch-shard-plan-v0.report.lisp"]
    :summary "Wave48-02 done: context-pack appended (3 observations + 2 shard-proposals + 1 conflict, seqs 5-10), report status=done, all 3 acceptance commands passed. Shard split recommendation captured in report :notes."))
