;; Wave 43 session trace.

(session-trace wave43
  :schema "missiond.session-trace.v1"
  :wave wave43
  :created-at "2026-04-29T03:49:58Z"
  :sequence 5

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
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.")

  (trace-event
    :id wave43-01-claudecode-preamble-read-004
    :seq 4
    :at "2026-04-29T03:55:30Z"
    :task wave43-01-v3-request-live-ipc-smoke-v0
    :backend claudecode-worker
    :kind read
    :files [".missiond/claudecode/wave43-shared-preamble.md"
            ".missiond/claudecode/wave43-01-v3-request-live-ipc-smoke-v0.md"
            ".missiond/tasks/wave43/wave43-01-v3-request-live-ipc-smoke-v0.lisp"
            ".missiond/tasks/wave43/context-atlas.lisp"
            ".missiond/tasks/wave43/pattern-cards.lisp"
            "scripts/check-v3-request-flow-smoke.mjs"
            "scripts/task-runner-submit-dispatch.mjs"
            "crates/missiond-daemon/src/handlers/knowledge/request.rs"]
    :summary "Loaded wave43 brief + atlas + pattern-cards + wave42 smoke checker + IPC helper + live request handler before extending the smoke with --live-ipc mode.")

  (trace-event
    :id wave43-01-claudecode-completion-005
    :seq 5
    :at "2026-04-29T04:25:00Z"
    :task wave43-01-v3-request-live-ipc-smoke-v0
    :backend claudecode-worker
    :kind completion
    :files ["scripts/check-v3-request-flow-smoke.mjs"]
    :commit_hash "7e8516d33a46"
    :summary "Wave43-01 V3 request-flow live IPC smoke landed. --live-ipc drives start -> approve_intent -> approve_plan against the running daemon and stops at awaiting_execution; --cleanup removes only the request-local directory. Default + --dry-fixture unchanged. No drift; Rust unchanged."))
