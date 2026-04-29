;; Wave 38 session trace.

(session-trace wave38
  :schema "missiond.session-trace.v1"
  :wave wave38
  :created-at "2026-04-29T01:58:38Z"
  :sequence 1

  (trace-event
    :id wave38-bootstrap-start-001
    :seq 1
    :at "2026-04-29T01:58:38Z"
    :task wave38-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave38/manifest.lisp"
            ".missiond/tasks/wave38/wave38-01-workflow-methodology-artifact-v0.lisp"]
    :summary "Prepared wave38 for delegated ClaudeCode execution.")

  (trace-event
    :id wave38-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T02:00:45Z"
    :task wave38-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave38-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T02:00:45Z"
    :task wave38-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave38-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.")

  (trace-event
    :id wave38-01-workflow-methodology-artifact-v0-preamble-read-004
    :seq 4
    :at "2026-04-29T03:00:00+08:00"
    :task wave38-01-workflow-methodology-artifact-v0
    :backend claudecode
    :kind read
    :files [".missiond/claudecode/wave38-shared-preamble.md"
            ".missiond/claudecode/wave38-01-workflow-methodology-artifact-v0.md"
            ".missiond/tasks/wave38/wave38-01-workflow-methodology-artifact-v0.lisp"
            ".missiond/tasks/wave38/context-atlas.lisp"
            ".missiond/tasks/wave38/pattern-cards.lisp"]
    :summary "Worker loaded shared preamble + thin brief + atlas/pattern cards before broad scans, satisfying the wave38 audit expectation."))
