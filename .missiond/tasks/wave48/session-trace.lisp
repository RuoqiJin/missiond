;; Wave 48 session trace.

(session-trace wave48
  :schema "missiond.session-trace.v1"
  :wave wave48
  :created-at "2026-04-29T06:01:00Z"
  :sequence 1

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
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
