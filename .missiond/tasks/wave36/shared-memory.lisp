;; Wave 36 shared-memory ledger.

(shared-memory wave36
  :schema "missiond.shared-memory.v1"
  :wave wave36
  :created-at "2026-04-28T22:56:00+08:00"
  :sequence 2

  (observation
    :id wave36-theme-001
    :task wave36-01-mission-request-review-response-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T22:56:00+08:00"
    :touched [".missiond/tasks/wave36/manifest.lisp"
              ".missiond/tasks/wave36/context-atlas.lisp"
              ".missiond/tasks/wave36/pattern-cards.lisp"
              ".missiond/tasks/wave36/wave36-01-mission-request-review-response-v0.lisp"]
    :summary "Wave36 theme: make mission_request continue the human review loop by accepting explicit review responses as the unified entry adapter, while preserving existing directive/plan approval gates.")

  (observation
    :id wave36-bootstrap-001
    :task wave36-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-28T14:55:47Z"
    :touched [".missiond/claudecode/wave36-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded."))
