;; Wave 44 session trace.

(session-trace wave44
  :schema "missiond.session-trace.v1"
  :wave wave44
  :created-at "2026-04-29T04:08:08Z"
  :sequence 5

  (trace-event
    :id wave44-bootstrap-start-001
    :seq 1
    :at "2026-04-29T04:08:08Z"
    :task wave44-bootstrap
    :backend codex-parent
    :kind start
    :files [".missiond/tasks/wave44/manifest.lisp"
            ".missiond/tasks/wave44/wave44-01-request-local-artifact-roots-v0.lisp"
            ".missiond/tasks/wave44/context-atlas.lisp"
            ".missiond/tasks/wave44/pattern-cards.lisp"]
    :summary "Prepared wave44 for delegated ClaudeCode execution.")

  (trace-event
    :id wave44-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T04:10:38Z"
    :task wave44-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave44-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T04:10:38Z"
    :task wave44-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave44-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.")

  (trace-event
    :id wave44-01-claudecode-preamble-read-004
    :seq 4
    :at "2026-04-29T04:30:00Z"
    :task wave44-01-request-local-artifact-roots-v0
    :backend claudecode-worker
    :kind read
    :files [".missiond/claudecode/wave44-shared-preamble.md"
            ".missiond/claudecode/wave44-01-request-local-artifact-roots-v0.md"
            ".missiond/tasks/wave44/wave44-01-request-local-artifact-roots-v0.lisp"
            ".missiond/tasks/wave44/context-atlas.lisp"
            ".missiond/tasks/wave44/pattern-cards.lisp"
            ".missiond/v3/missiond-blueprint.lisp"
            "crates/missiond-daemon/src/handlers/knowledge/request.rs"
            "crates/missiond-mcp/src/tools/knowledge/request.rs"
            "scripts/check-v3-request-flow-smoke.mjs"]
    :summary "Loaded wave44 brief + atlas + pattern-cards + V3 blueprint + request.rs + MCP + smoke checker before changing default artifact roots.")

  (trace-event
    :id wave44-01-claudecode-completion-005
    :seq 5
    :at "2026-04-29T05:00:00Z"
    :task wave44-01-request-local-artifact-roots-v0
    :backend claudecode-worker
    :kind completion
    :files [".missiond/v3/missiond-blueprint.lisp"
            "crates/missiond-daemon/src/handlers/knowledge/request.rs"
            "crates/missiond-mcp/src/tools/knowledge/request.rs"
            "scripts/check-v3-request-flow-smoke.mjs"]
    :commit_hash "c9dfe3b57a5e"
    :summary "Wave44-01 landed. compat_write_file added at mission_request adapter; default flow strips write_file from forwarded args. Smoke compat_write_audit step asserts request-local-only on default --live-ipc; both acceptance runs report new_alignment_subdirs=[] and new_plan_subdirs=[]. Daemon tests 79->86; MCP test pass; aggregate v3 gate still daemon-free."))
