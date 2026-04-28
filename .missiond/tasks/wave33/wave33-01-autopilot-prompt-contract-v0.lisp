;; Wave 33 task contract.

(task wave33-01-autopilot-prompt-contract-v0
  :schema "missiond.task-contract.v1"
  :title "Autopilot prompt/tool contract Lisp projection v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 35
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave33/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave33/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Align Autopilot's ClaudeCode prompt construction with the V3 Lisp workstation-config contract so delegated coding tasks no longer show duplicated objective text or unconditional instructions to call board MCP tools that may not be attached to the slot."

  :write-scope
    ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
     ".missiond/v3/missiond-blueprint.lisp"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
     "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
     "crates/missiond-daemon/src/handlers/compute/pty.rs"
     "crates/missiond-core/**"
     "crates/missiond-mcp/**"
     "scripts/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/tasks/schema/**"
     ".missiond/tasks/wave31/**"
     ".missiond/tasks/wave32/**"
     ".missiond/tasks/wave33/manifest.lisp"
     ".missiond/tasks/wave33/dispatch-plan.lisp"
     ".missiond/tasks/wave33/context-atlas.lisp"
     ".missiond/tasks/wave33/pattern-cards.lisp"
     ".missiond/tasks/wave33/wave33-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Add a compact prompt/tool contract under .missiond/v3/missiond-blueprint.lisp workstation-config. It should say: Board Task ID is always surfaced; worker self-close via board MCP tools is conditional on tool availability; if tools are unavailable, the worker should return a concise completion summary and Autopilot/orchestrator remains responsible for closing the task; prompt assembly must avoid duplicating title/objective text."
     "Update the workstation-config implementation-map note to state that Autopilot prompt assembly projects this V3 prompt/tool contract."
     "In autopilot.rs, extract the title/description/template prompt assembly into a pure helper. The helper must suppress duplicated objective text when description is exactly the title or starts with title followed by blank lines. Distinct title + description should still render both."
     "Replace the unconditional self-close wording that says the worker must call mission_board_update / mission_board_note_add. New wording must be conditional and explicit that returning a final summary is acceptable when board tools are absent."
     "Keep Decision Engine escalation guidance and ops-task guidance behaviorally intact, except for any helper extraction needed to make the code testable."
     "Add focused pure unit tests in autopilot.rs for duplicate objective suppression and conditional board-tool completion instructions. Do not construct AppState in tests."]

  :acceptance
    ["cargo test -p missiond-daemon engine::intent_engine::autopilot::tests"
     "cargo check -p missiond-daemon"
     "node scripts/check-lisp-blueprint-compression.mjs"
     "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
     "perl -ne 'exit 1 if /\\x00/' crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"
     "git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"]

  :commit
    (:required true
     :message "fix(autopilot): project prompt tool contract"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Prompt/tool contract added to V3 blueprint."
     "Prompt helper behavior and test coverage."
     "Exact replacement wording for board-tool self-close instructions."
     "Acceptance command results."])
