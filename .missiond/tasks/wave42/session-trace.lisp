;; Wave 42 session trace.

(session-trace wave42
  :schema "missiond.session-trace.v1"
  :wave wave42
  :created-at "2026-04-29T03:30:31Z"
  :sequence 5

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
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.")

  (trace-event
    :id wave42-01-claudecode-preamble-read-004
    :seq 4
    :at "2026-04-29T03:35:30Z"
    :task wave42-01-v3-request-flow-smoke-v0
    :backend claudecode-worker
    :kind read
    :files [".missiond/claudecode/wave42-shared-preamble.md"
            ".missiond/claudecode/wave42-01-v3-request-flow-smoke-v0.md"
            ".missiond/tasks/wave42/wave42-01-v3-request-flow-smoke-v0.lisp"
            ".missiond/tasks/wave42/context-atlas.lisp"
            ".missiond/tasks/wave42/pattern-cards.lisp"
            ".missiond/v3/missiond-blueprint.lisp"]
    :summary "Loaded wave42 shared preamble + brief + atlas/pattern-cards before broad scans.")

  (trace-event
    :id wave42-01-claudecode-completion-005
    :seq 5
    :at "2026-04-29T03:55:00Z"
    :task wave42-01-v3-request-flow-smoke-v0
    :backend claudecode-worker
    :kind completion
    :files [".missiond/v3/missiond-blueprint.lisp"
            "scripts/check-v3-request-flow-smoke.mjs"
            "scripts/check-v3-code-isomorphism-complete.mjs"]
    :commit_hash "67ec5d8b6c7f"
    :summary "Wave42-01 V3 request-flow smoke gate landed. Aggregate per-surface checker count grew from 6 to 7. All acceptance commands exit 0."))
