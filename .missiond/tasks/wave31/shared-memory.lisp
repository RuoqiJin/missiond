;; Wave 31 shared-memory ledger.

(shared-memory wave31
  :schema "missiond.shared-memory.v1"
  :wave wave31
  :created-at "2026-04-28T19:22:31+08:00"
  :sequence 1

  (observation
    :id wave31-bootstrap-001
    :task wave31-01-mission-request-local-projections-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T19:22:31+08:00"
    :touched [".missiond/tasks/wave31/manifest.lisp"
              ".missiond/tasks/wave31/context-atlas.lisp"
              ".missiond/tasks/wave31/pattern-cards.lisp"
              ".missiond/tasks/wave31/wave31-01-mission-request-local-projections-v0.lisp"]
    :summary "Wave31 theme: measure ClaudeCode efficiency after V3 request entry and wave30 lifecycle upgrades by implementing request-local Lisp projections.")

  (observation
    :id wave31-bootstrap-002
    :task wave31-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-28T11:22:52Z"
    :touched [".missiond/claudecode/wave31-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded."))
