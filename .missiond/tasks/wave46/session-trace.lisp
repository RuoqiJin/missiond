;; Wave 46 session trace.

(session-trace wave46
  :schema "missiond.session-trace.v1"
  :wave wave46
  :created-at "2026-04-29T04:47:14Z"
  :sequence 1

  (trace-event
    :id wave46-bootstrap-start-001
    :seq 1
    :at "2026-04-29T04:47:14Z"
    :task wave46-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave46/manifest.lisp"
            ".missiond/tasks/wave46/wave46-01-request-internal-execute-dry-run-v0.lisp"
            ".missiond/tasks/wave46/context-atlas.lisp"
            ".missiond/tasks/wave46/pattern-cards.lisp"]
    :summary "Prepared wave46 for delegated ClaudeCode execution. Scope: make --execute-dry-run exercise execute_mode=internal and prove workstation_dispatch_status=dry_run_no_dispatch.")

  (trace-event
    :id wave46-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T04:50:24Z"
    :task wave46-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave46-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T04:50:24Z"
    :task wave46-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave46-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
