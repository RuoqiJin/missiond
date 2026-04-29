;; Wave 43 shared-memory ledger.

(shared-memory wave43
  :schema "missiond.shared-memory.v1"
  :wave wave43
  :created-at "2026-04-29T03:49:58Z"
  :sequence 1

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
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded."))
