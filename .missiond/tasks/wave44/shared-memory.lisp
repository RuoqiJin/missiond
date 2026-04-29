;; Wave 44 shared-memory ledger.

(shared-memory wave44
  :schema "missiond.shared-memory.v1"
  :wave wave44
  :created-at "2026-04-29T04:08:08Z"
  :sequence 1

  (observation
    :id wave44-bootstrap-001
    :task wave44-01-request-local-artifact-roots-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-29T04:08:08Z"
    :touched [".missiond/tasks/wave44/manifest.lisp"
              ".missiond/tasks/wave44/wave44-01-request-local-artifact-roots-v0.lisp"
              ".missiond/tasks/wave44/context-atlas.lisp"
              ".missiond/tasks/wave44/pattern-cards.lisp"]
    :summary "Wave44 prepared by Codex parent: make mission_request default live artifacts request-local only, with legacy compatibility writers explicit opt-in.")

  (observation
    :id wave44-bootstrap-002
    :task wave44-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-29T04:10:38Z"
    :touched [".missiond/claudecode/wave44-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded."))
