;; Wave 34 shared-memory ledger.

(shared-memory wave34
  :schema "missiond.shared-memory.v1"
  :wave wave34
  :created-at "2026-04-28T21:15:00+08:00"
  :sequence 1

  (observation
    :id wave34-bootstrap-001
    :task wave34-01-autopilot-complete-close-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T21:15:00+08:00"
    :touched [".missiond/tasks/wave34/manifest.lisp"
              ".missiond/tasks/wave34/context-atlas.lisp"
              ".missiond/tasks/wave34/pattern-cards.lisp"
              ".missiond/tasks/wave34/wave34-01-autopilot-complete-close-v0.lisp"]
    :summary "Wave34 theme: project delegated BoardTask prompt ownership and close ownership into V3 Lisp and the task_delegate/compute_slot/Autopilot runtime path.")

  (observation
    :id wave34-bootstrap-002
    :task wave34-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-28T13:18:58Z"
    :touched [".missiond/claudecode/wave34-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded.")

  (claim
    :id wave34-01-claim-003
    :task wave34-01-autopilot-complete-close-v0
    :agent claudecode
    :seq 3
    :at "2026-04-28T13:25:00Z"
    :touched [".missiond/claudecode/wave34-01-autopilot-complete-close-v0.md"
              ".missiond/claudecode/wave34-shared-preamble.md"
              ".missiond/tasks/wave34/wave34-01-autopilot-complete-close-v0.lisp"
              ".missiond/tasks/wave34/context-atlas.lisp"
              ".missiond/tasks/wave34/pattern-cards.lisp"]
    :summary "Claudecode worker claims wave34-01: project execution-ownership rule into V3 blueprint, suppress compute_slot initial_prompt for task_delegate auto-provision, hold autopilot dispatch guard across pty.send.")

  (completion
    :id wave34-01-completion-004
    :task wave34-01-autopilot-complete-close-v0
    :agent claudecode
    :seq 4
    :at "2026-04-28T13:55:00Z"
    :touched ["crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
              "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
              "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
              ".missiond/v3/missiond-blueprint.lisp"
              ".missiond/tasks/wave34/reports/wave34-01-autopilot-complete-close-v0.report.lisp"]
    :summary "wave34-01 complete: V3 execution-ownership :: delegated-boardtask added; compute_slot::effective_initial_prompt + suppress_initial_prompt arg suppress the spawner fire-and-forget for task_delegate auto-provision; task_delegate::build_compute_slot_create_args hardcodes suppress=true; autopilot dispatch_board_tasks now holds slot_dispatch.try_acquire_guard across state.pty.send and uses decide_close_action to preserve Done self-close and Blocked question states. Acceptance: autopilot tests 22/22 (4 new), compute_slot tests 9/9 (3 new), cargo check clean, blueprint-compression + architecture-lisp + NUL + git diff --check all OK."))
