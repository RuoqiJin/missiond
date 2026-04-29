;; Wave 44 session trace.

(session-trace wave44
  :schema "missiond.session-trace.v1"
  :wave wave44
  :created-at "2026-04-29T04:08:08Z"
  :sequence 1

  (trace-event
    :id wave44-bootstrap-start-001
    :seq 1
    :at "2026-04-29T04:08:08Z"
    :task wave44-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave44/manifest.lisp"
            ".missiond/tasks/wave44/wave44-01-request-local-artifact-roots-v0.lisp"
            ".missiond/tasks/wave44/context-atlas.lisp"
            ".missiond/tasks/wave44/pattern-cards.lisp"]
    :summary "Prepared wave44 for delegated ClaudeCode execution.")

  (trace-event
    :id wave44-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T04:10:38Z"
    :task wave44-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave44-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T04:10:38Z"
    :task wave44-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave44-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
