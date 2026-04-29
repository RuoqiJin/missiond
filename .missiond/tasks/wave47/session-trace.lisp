;; Wave 47 session trace.

(session-trace wave47
  :schema "missiond.session-trace.v1"
  :wave wave47
  :created-at "2026-04-29T05:11:25Z"
  :sequence 1

  (trace-event
    :id wave47-bootstrap-start-001
    :seq 1
    :at "2026-04-29T05:11:25Z"
    :task wave47-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave47/manifest.lisp"
            ".missiond/tasks/wave47/wave47-01-request-real-dispatch-smoke-v0.lisp"
            ".missiond/tasks/wave47/context-atlas.lisp"
            ".missiond/tasks/wave47/pattern-cards.lisp"]
    :summary "Prepared wave47 for delegated ClaudeCode execution. Scope: real mission_request execute_plan dispatch smoke, opt-in only.")

  (trace-event
    :id wave47-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T05:13:44Z"
    :task wave47-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave47-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T05:13:44Z"
    :task wave47-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave47-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
