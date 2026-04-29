;; Wave 48 dispatch-time context atlas.

(context-atlas wave48-context-pack-parallel-investigation-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave48
  :goal "Use context-pack as a parallel investigation surface before code-shard implementation."
  :read-order [".missiond/claudecode/wave48-shared-preamble.md"
               ".missiond/tasks/wave48/context-atlas.lisp"
               ".missiond/tasks/wave48/pattern-cards.lisp"
               ".missiond/tasks/wave48/context-pack.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               "scripts/check-context-pack.mjs"
               "scripts/context-pack-append.mjs"
               "scripts/check-v3-context-pack-isomorphism.mjs"
               "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
               "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
               "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
               "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "Architecture authority. Read context-pack and workstation-config sections before proposals."
      :grep ["multi-agent-context-pack"
             "context-pack"
             "workstation-config"
             "delegated-boardtask"])
    (file "scripts/context-pack-append.mjs"
      :purpose "Required mutation helper for appending observations/proposals without hand-allocating seq."
      :grep ["appendContextPackEntry"
             "withLock"
             "nextSequence"
             "validateContextPackSource"])
    (file "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
      :purpose "Primary dynamic slot recovery investigation target."
      :grep ["dispatch_board_tasks"
             "Dynamic slot assignment"
             "task.assignee"
             "ensure_autopilot_pty"
             "unclaim_board_task"])
    (file "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
      :purpose "Existing missing-slot behavior that currently notes and retries/fails."
      :grep ["ensure_autopilot_pty"
             "slot not found"
             "Slot `{}` 不存在"
             "increment_board_task_retry"])))
