;; Wave 43 shared-memory ledger.

(shared-memory wave43
  :schema "missiond.shared-memory.v1"
  :wave wave43
  :created-at "2026-04-29T03:49:58Z"
  :sequence 4

  (observation
    :id wave43-bootstrap-001
    :task wave43-01-v3-request-live-ipc-smoke-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-29T03:49:58Z"
    :touched [".missiond/tasks/wave43/manifest.lisp"
              ".missiond/tasks/wave43/wave43-01-v3-request-live-ipc-smoke-v0.lisp"
              ".missiond/tasks/wave43/context-atlas.lisp"
              ".missiond/tasks/wave43/pattern-cards.lisp"]
    :summary "Wave43 prepared by Codex parent: upgrade V3 request-flow smoke to an opt-in live IPC path that stops at awaiting_execution.")

  (observation
    :id wave43-bootstrap-002
    :task wave43-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-29T03:52:06Z"
    :touched [".missiond/claudecode/wave43-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded.")

  (claim
    :id wave43-01-claim-001
    :task wave43-01-v3-request-live-ipc-smoke-v0
    :agent claudecode-worker
    :seq 3
    :at "2026-04-29T03:55:30Z"
    :summary "Claim wave43-01-v3-request-live-ipc-smoke-v0: extend check-v3-request-flow-smoke.mjs with opt-in --live-ipc mode that drives mission_request start -> approve_intent -> approve_plan and stops at awaiting_execution.")

  (completion
    :id wave43-01-completion-001
    :task wave43-01-v3-request-live-ipc-smoke-v0
    :agent claudecode-worker
    :seq 4
    :at "2026-04-29T04:25:00Z"
    :commit_hash "7e8516d33a46"
    :touched ["scripts/check-v3-request-flow-smoke.mjs"]
    :summary "Added opt-in --live-ipc mode to check-v3-request-flow-smoke.mjs: drives mission_request start -> approve_intent -> approve_plan over the daemon tools/call IPC, stops at awaiting_execution, and (with --cleanup) removes only the request-local directory. Supports --endpoint / --session-id / --request-id / --cleanup / --json. Reuses callToolViaIpc from scripts/task-runner-submit-dispatch.mjs. No Lisp/code drift exposed; .missiond/v3/missiond-blueprint.lisp + request.rs + MCP schema unchanged. Default + --dry-fixture remain daemon-free (per requirement #1) so the aggregate v3 gate stays deterministic. 79 daemon request handler tests + MCP surfaces test pass."))
