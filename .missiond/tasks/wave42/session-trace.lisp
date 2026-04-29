;; Wave 42 session trace.

(session-trace wave42
  :schema "missiond.session-trace.v1"
  :wave wave42
  :created-at "2026-04-29T03:30:31Z"
  :sequence 1

  (trace-event
    :id wave42-bootstrap-start-001
    :seq 1
    :at "2026-04-29T03:30:31Z"
    :task wave42-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave42/manifest.lisp"
            ".missiond/tasks/wave42/wave42-01-v3-request-flow-smoke-v0.lisp"
            ".missiond/tasks/wave42/context-atlas.lisp"
            ".missiond/tasks/wave42/pattern-cards.lisp"]
    :summary "Prepared wave42 for delegated ClaudeCode execution.")

  (trace-event
    :id wave42-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T03:33:37Z"
    :task wave42-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave42-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T03:33:37Z"
    :task wave42-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave42-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
