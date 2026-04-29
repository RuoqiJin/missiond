;; Wave 47 shared-memory ledger.

(shared-memory wave47
  :schema "missiond.shared-memory.v1"
  :wave wave47
  :created-at "2026-04-29T05:11:25Z"
  :sequence 1

  (observation
    :id wave47-bootstrap-001
    :task wave47-01-request-real-dispatch-smoke-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-29T05:11:25Z"
    :touched [".missiond/tasks/wave47/manifest.lisp"
              ".missiond/tasks/wave47/wave47-01-request-real-dispatch-smoke-v0.lisp"
              ".missiond/tasks/wave47/context-atlas.lisp"
              ".missiond/tasks/wave47/pattern-cards.lisp"]
    :summary "Wave47 prepared by Codex parent: after wave46 internal dry-run proof, add an explicit opt-in real-dispatch smoke that proves mission_request execute_plan can create a delegated BoardTask through workstation_dispatch without adding real dispatch to default or aggregate gates.")

  (observation
    :id wave47-bootstrap-002
    :task wave47-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-29T05:13:44Z"
    :touched [".missiond/claudecode/wave47-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded."))
