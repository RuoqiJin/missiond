;; Wave 41 session trace.

(session-trace wave41
  :schema "missiond.session-trace.v1"
  :wave wave41
  :created-at "2026-04-29T03:14:37Z"
  :sequence 1

  (trace-event
    :id wave41-bootstrap-start-001
    :seq 1
    :at "2026-04-29T03:14:37Z"
    :task wave41-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave41/manifest.lisp"
            ".missiond/tasks/wave41/wave41-01-v3-complete-isomorphism-gate-v0.lisp"
            ".missiond/tasks/wave41/context-atlas.lisp"
            ".missiond/tasks/wave41/pattern-cards.lisp"]
    :summary "Prepared wave41 for delegated ClaudeCode execution.")

  (trace-event
    :id wave41-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T03:16:30Z"
    :task wave41-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave41-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T03:16:30Z"
    :task wave41-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave41-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
