;; Wave 40 session trace.

(session-trace wave40
  :schema "missiond.session-trace.v1"
  :wave wave40
  :created-at "2026-04-29T02:55:06Z"
  :sequence 1

  (trace-event
    :id wave40-bootstrap-start-001
    :seq 1
    :at "2026-04-29T02:55:06Z"
    :task wave40-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave40/manifest.lisp"
            ".missiond/tasks/wave40/wave40-01-parent-hotfix-report-preservation-v0.lisp"
            ".missiond/tasks/wave40/context-atlas.lisp"
            ".missiond/tasks/wave40/pattern-cards.lisp"]
    :summary "Prepared wave40 for delegated ClaudeCode execution.")

  (trace-event
    :id wave40-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T02:57:18Z"
    :task wave40-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave40-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T02:57:18Z"
    :task wave40-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave40-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
