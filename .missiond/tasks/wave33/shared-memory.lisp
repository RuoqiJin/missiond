;; Wave 33 shared-memory ledger.

(shared-memory wave33
  :schema "missiond.shared-memory.v1"
  :wave wave33
  :created-at "2026-04-28T20:52:00+08:00"
  :sequence 4

  (observation
    :id wave33-bootstrap-001
    :task wave33-01-autopilot-prompt-contract-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T20:52:00+08:00"
    :touched [".missiond/tasks/wave33/manifest.lisp"
              ".missiond/tasks/wave33/context-atlas.lisp"
              ".missiond/tasks/wave33/pattern-cards.lisp"
              ".missiond/tasks/wave33/wave33-01-autopilot-prompt-contract-v0.lisp"]
    :summary "Wave33 theme: continue MissionD code-to-Lisp isomorphism cleanup by moving Autopilot prompt/tool behavior into the V3 Lisp workstation-config contract.")

  (observation
    :id wave33-bootstrap-002
    :task wave33-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-28T12:53:40Z"
    :touched [".missiond/claudecode/wave33-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded.")

  (claim
    :id wave33-01-claim-003
    :task wave33-01-autopilot-prompt-contract-v0
    :agent claudecode-worker
    :seq 3
    :at "2026-04-28T13:05:00Z"
    :touched [".missiond/claudecode/wave33-shared-preamble.md"
              ".missiond/tasks/wave33/context-atlas.lisp"
              ".missiond/tasks/wave33/pattern-cards.lisp"
              ".missiond/tasks/wave33/wave33-01-autopilot-prompt-contract-v0.lisp"]
    :summary "Claiming wave33-01: extract pure prompt-base helper, suppress duplicated objective text, project conditional board self-close into V3 workstation-config and autopilot.rs.")

  (completion
    :id wave33-01-completion-004
    :task wave33-01-autopilot-prompt-contract-v0
    :agent claudecode-worker
    :seq 4
    :at "2026-04-28T13:25:00Z"
    :touched ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
              ".missiond/v3/missiond-blueprint.lisp"]
    :summary "wave33-01 done. V3 workstation-config now declares prompt-tool-contract autopilot-claudecode-prompt (always-shown Board Task ID, objective-dedupe rule, conditional board-self-close). Autopilot.rs projects it via build_base_prompt + append_board_task_id_suffix; old unconditional `你必须调用` wording replaced with conditional clause + tools-absent fallback to a final summary. 18 autopilot tests pass (8 new), cargo check + lisp blueprint + architecture-lisp + NUL + diff-check all green."))
