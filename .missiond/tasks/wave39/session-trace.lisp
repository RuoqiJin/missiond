;; Wave 39 session trace.

(session-trace wave39
  :schema "missiond.session-trace.v1"
  :wave wave39
  :created-at "2026-04-29T02:19:53Z"
  :sequence 1

  (trace-event
    :id wave39-bootstrap-start-001
    :seq 1
    :at "2026-04-29T02:19:53Z"
    :task wave39-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave39/manifest.lisp"
            ".missiond/tasks/wave39/wave39-01-task-scoped-lifecycle-event-files-v0.lisp"
            ".missiond/tasks/wave39/context-atlas.lisp"
            ".missiond/tasks/wave39/pattern-cards.lisp"]
    :summary "Prepared wave39 for delegated ClaudeCode execution.")

  (trace-event
    :id wave39-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T02:20:13Z"
    :task wave39-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave39-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T02:20:13Z"
    :task wave39-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave39-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
