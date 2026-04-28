;; Wave 34 task contract.

(task wave34-01-autopilot-complete-close-v0
  :schema "missiond.task-contract.v1"
  :title "Autopilot delegated-task completion ownership v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 40
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave34/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave34/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Fix the delegated BoardTask execution ownership gap observed in wave33: dynamic slot provisioning must not send a fire-and-forget task objective before Autopilot sends the real BoardTask prompt, and Autopilot must remain the close owner after pty.send returns Complete unless the worker already self-closed or blocked the task."

  :observed-runtime-evidence
    ["wave33 board task 9b6d9c61-f251-4c52-83d8-378e0e313f65 used an Opus 4.7 dynamic slot and worker commit 9245d42 succeeded."
     "Daemon logs showed slot_orchestrator::spawner sent an initial prompt after idle using Message sent (fire-and-forget), then Autopilot later claimed/executed the same BoardTask with state.pty.send."
     "missiond-pty emitted TextOutputEvent::Complete for the slot, but Autopilot did not transition the BoardTask to done; the orchestrator had to close it by SQL and add a summary note."
     "This is a Lisp-isomorphism gap: V3 described prompt content, but did not yet define prompt ownership and close ownership for the delegated execution path."]

  :write-scope
    ["crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
     "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
     ".missiond/v3/missiond-blueprint.lisp"]

  :must-not-touch
    ["crates/missiond-daemon/src/slot_orchestrator/spawner.rs"
     "crates/missiond-daemon/src/slot_dispatch.rs"
     "crates/missiond-daemon/src/handlers/compute/pty.rs"
     "crates/missiond-core/**"
     "crates/missiond-mcp/**"
     "crates/missiond-pty/**"
     "scripts/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/tasks/schema/**"
     ".missiond/tasks/wave31/**"
     ".missiond/tasks/wave32/**"
     ".missiond/tasks/wave33/**"
     ".missiond/tasks/wave34/manifest.lisp"
     ".missiond/tasks/wave34/dispatch-plan.lisp"
     ".missiond/tasks/wave34/context-atlas.lisp"
     ".missiond/tasks/wave34/pattern-cards.lisp"
     ".missiond/tasks/wave34/wave34-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Add a compact execution-ownership rule under .missiond/v3/missiond-blueprint.lisp workstation-config. It must state: for delegated BoardTask execution, Autopilot is the task prompt owner; compute_slot/spawner may provision and warm a slot but must not send the task objective as a fire-and-forget execution prompt; Autopilot is the close owner unless board MCP tools are attached and the worker self-closes or the task becomes blocked by a question."
     "Update the workstation-config implementation-map note so it names the Rust projection points in task_delegate, compute_slot, and autopilot."
     "In compute_slot.rs, add an explicit create-time option or small helper for initial prompt ownership. Direct mission_compute_slot create should remain compatible by default, but task_delegate auto-provisioning must be able to suppress PTYSpawnOptions.initial_prompt."
     "In task_delegate.rs, pass the new option when auto-provisioning a slot for a BoardTask so the dynamic slot starts idle and the queued BoardTask is the only task prompt Autopilot sends."
     "In autopilot.rs, close the release-before-send race around the per-slot dispatch guard. The guard is per-slot, so it may be held until state.pty.send returns; keep existing behavior that preserves Done self-close and Blocked question states."
     "Keep wave32 timeout-budget projection and wave33 prompt/tool wording intact."
     "Add focused tests for the new compute_slot/task_delegate initial-prompt option or helper, and for any Autopilot guard helper if extracted. Avoid constructing AppState in tests."]

  :acceptance
    ["cargo test -p missiond-daemon engine::intent_engine::autopilot::tests"
     "cargo test -p missiond-daemon handlers::compute::compute_slot::tests"
     "cargo check -p missiond-daemon"
     "node scripts/check-lisp-blueprint-compression.mjs"
     "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
     "perl -ne 'exit 1 if /\\x00/' crates/missiond-daemon/src/handlers/compute/task_delegate.rs crates/missiond-daemon/src/handlers/compute/compute_slot.rs crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"
     "git diff --check -- crates/missiond-daemon/src/handlers/compute/task_delegate.rs crates/missiond-daemon/src/handlers/compute/compute_slot.rs crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"]

  :commit
    (:required true
     :message "fix(autopilot): close delegated task completion loop"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "V3 execution-ownership rule added."
     "How task_delegate suppresses compute_slot initial_prompt for delegated BoardTasks."
     "How Autopilot keeps prompt/close ownership and preserves self-close or blocked states."
     "Acceptance command results."])
