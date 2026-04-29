;; Wave 43 session trace.

(session-trace wave43
  :schema "missiond.session-trace.v1"
  :wave wave43
  :created-at "2026-04-29T03:49:58Z"
  :sequence 1

  (trace-event
    :id wave43-bootstrap-start-001
    :seq 1
    :at "2026-04-29T03:49:58Z"
    :task wave43-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave43/manifest.lisp"
            ".missiond/tasks/wave43/wave43-01-v3-request-live-ipc-smoke-v0.lisp"
            ".missiond/tasks/wave43/context-atlas.lisp"
            ".missiond/tasks/wave43/pattern-cards.lisp"]
    :summary "Prepared wave43 for delegated ClaudeCode execution.")

  (trace-event
    :id wave43-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T03:52:06Z"
    :task wave43-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave43-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T03:52:06Z"
    :task wave43-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave43-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
