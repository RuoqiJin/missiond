;; Wave 45 session trace.

(session-trace wave45
  :schema "missiond.session-trace.v1"
  :wave wave45
  :created-at "2026-04-29T04:30:35Z"
  :sequence 1

  (trace-event
    :id wave45-bootstrap-start-001
    :seq 1
    :at "2026-04-29T04:30:35Z"
    :task wave45-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave45/manifest.lisp"
            ".missiond/tasks/wave45/wave45-01-request-execute-dry-run-smoke-v0.lisp"
            ".missiond/tasks/wave45/context-atlas.lisp"
            ".missiond/tasks/wave45/pattern-cards.lisp"]
    :summary "Prepared wave45 for delegated ClaudeCode execution. Scope: explicit --execute-dry-run live request smoke; default live smoke remains non-executing.")

  (trace-event
    :id wave45-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T04:32:38Z"
    :task wave45-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave45-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T04:32:38Z"
    :task wave45-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave45-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.")

  (trace-event
    :id wave45-trace-bootstrap-start-003
    :seq 4
    :at "2026-04-29T04:33:20Z"
    :task wave45-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave45-trace-bootstrap-read-003
    :seq 5
    :at "2026-04-29T04:33:20Z"
    :task wave45-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave45-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
