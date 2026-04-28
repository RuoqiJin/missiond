;; Worker report for wave34-01-autopilot-complete-close-v0.
;; Schema: missiond.report-contract.v1

(report wave34-01-autopilot-complete-close-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave34-01-autopilot-complete-close-v0"
  :status done
  :commit_hash "1cb92dd87639"
  :files_changed
    ["crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
     "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
     ".missiond/v3/missiond-blueprint.lisp"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon engine::intent_engine::autopilot::tests"
      :exit_code 0 :ok true
      :detail "22 tests passed (18 pre-existing + 4 new decide_close_action tests covering Done self-close preserve, Blocked preserve, Running/Open owner-closes, None owner-closes); 1711 filtered.")
     (:command "cargo test -p missiond-daemon handlers::compute::compute_slot::tests"
      :exit_code 0 :ok true
      :detail "9 tests passed (6 pre-existing model-projection + 3 new effective_initial_prompt tests covering default-objective passthrough, suppress=true → None, objective-absent both flags); 1724 filtered.")
     (:command "cargo check -p missiond-daemon"
      :exit_code 0 :ok true
      :detail "compiles clean; no new warnings introduced by the four touched files (pre-existing crate dead-code/unused-import warnings unchanged).")
     (:command "node scripts/check-lisp-blueprint-compression.mjs"
      :exit_code 0 :ok true
      :detail "v1 manifest + v3 blueprint compression check OK after workstation-config execution-ownership addition + implementation-map note update.")
     (:command "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "architecture-lisp shape OK (1 file scanned).")
     (:command "perl -ne 'exit 1 if /\\x00/' crates/missiond-daemon/src/handlers/compute/task_delegate.rs crates/missiond-daemon/src/handlers/compute/compute_slot.rs crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "no NUL bytes in any of the four write-scope files.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/compute/task_delegate.rs crates/missiond-daemon/src/handlers/compute/compute_slot.rs crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "no trailing-whitespace / mixed-indent diff complaints in the staged write scope.")]

  :blueprint_change
    (:file ".missiond/v3/missiond-blueprint.lisp"
     :workstation_config_execution_ownership_added true
     :execution_ownership_form
       (execution-ownership delegated-boardtask
         :applies-to [coder researcher ops]
         :prompt-owner
           "For delegated BoardTask execution, Autopilot is the sole task-prompt owner. mission_task_delegate auto-provision (compute_slot/spawner) MAY warm a dynamic slot but MUST NOT send the task objective as a fire-and-forget initial-prompt; the slot starts idle and Autopilot sends the BoardTask prompt via state.pty.send."
         :close-owner
           (:default "Autopilot is the close owner — when state.pty.send returns Complete, Autopilot transitions the BoardTask running→done and writes the summary note."
            :worker-self-close "If the slot has board MCP tools attached and the worker already drove the BoardTask to Done via mission_board_update before pty.send returns, Autopilot preserves the worker's Done state and only logs that the task self-closed."
            :blocked "If the task transitioned to Blocked (e.g. mission_question_create) during execution, Autopilot preserves the Blocked state on pty.send return and never overwrites it with done.")
         :dispatch-guard
           "The per-slot dispatch guard MUST be held across the entire state.pty.send call; the legacy release-before-send pattern allowed a second caller to dispatch to the same slot mid-flight. The guard is per-slot, so holding it does not starve callers targeting other slots."
         :rationale
           "Wave33 evidence: a delegated BoardTask was sent twice — once via spawner.initial_prompt fire-and-forget, then again via Autopilot pty.send — and the slot's TextOutputEvent::Complete arrived without Autopilot transitioning the BoardTask to done. Single ownership of prompt+close eliminates the orphaned-task class entirely.")
     :implementation_map_note_changed true
     :implementation_map_note_summary
       "workstation-config surface :note now records that the V3 execution-ownership rule for delegated BoardTasks projects to: compute_slot::effective_initial_prompt + suppress_initial_prompt arg (delegated path starts the slot idle), task_delegate::auto_provision_slot create_args carrying suppress_initial_prompt=true, and autopilot dispatch_board_tasks holding slot_dispatch.try_acquire_guard across state.pty.send with decide_close_action preserving Done self-close and Blocked question states."
     :status_kept "code-aligned-partial"
     :status_rationale "wave34-01 closes the delegated execution prompt+close ownership gap that wave33 surfaced; wave32 timeout-budget projection and wave33 prompt-tool-contract wording remain intact and are not modified.")

  :compute_slot_initial_prompt_suppression
    (:helper "effective_initial_prompt(objective: Option<String>, suppress: bool) -> Option<String>"
     :rules
       ["suppress=true → return None unconditionally; the spawner does not warm the slot with a fire-and-forget prompt."
        "suppress=false → return objective as-is; direct mission_compute_slot create keeps legacy warm-up behaviour."]
     :arg_field "suppress_initial_prompt"
     :arg_default false
     :arg_consumer "create_slot in compute_slot.rs reads args[\"suppress_initial_prompt\"].as_bool().unwrap_or(false), then computes initial_prompt_for_spawn = effective_initial_prompt(objective_owned.clone(), suppress_initial_prompt) and feeds it into PTYSpawnOptions.initial_prompt."
     :test_coverage
       [(:name "effective_initial_prompt_returns_objective_by_default"
         :proves "Direct compute_slot create path: suppress=false → caller-supplied objective survives unchanged.")
        (:name "effective_initial_prompt_suppresses_when_flag_set"
         :proves "task_delegate auto-provision path: suppress=true → None, so PTYSpawnOptions.initial_prompt is None and the slot starts idle.")
        (:name "effective_initial_prompt_returns_none_when_objective_absent"
         :proves "Both flag values yield None when no objective is supplied — Direct create with an objectiveless slot still works.")])

  :task_delegate_create_args_helper
    (:helper "build_compute_slot_create_args(template, objective, ttl, cwd, model, model_profile) -> Value"
     :always_sets "suppress_initial_prompt: true (hardcoded in the helper so the delegated path can never accidentally warm the slot with a fire-and-forget prompt)."
     :consumer "auto_provision_slot calls build_compute_slot_create_args(...) and passes the result to compute_slot::handle(state, \"mission_compute_slot\", create_args)."
     :test_coverage
       [(:name "create_args_always_suppresses_initial_prompt"
         :proves "Every delegated-path provisioning carries suppress_initial_prompt=true regardless of optional inputs.")
        (:name "create_args_carry_template_objective_and_ttl"
         :proves "Required fields (action, template, objective, max_ttl) are populated from the helper inputs.")
        (:name "create_args_pass_through_optional_fields"
         :proves "Optional fields cwd / model / model_profile pass through when supplied, alongside the suppress flag.")
        (:name "create_args_omit_optional_fields_when_absent"
         :proves "Optional fields are absent when not supplied — no spurious null fields leak into the JSON payload.")])

  :autopilot_close_ownership
    (:dispatch_guard_change
       (:from "try_acquire then release immediately before pty.send (release-before-send window let a second caller dispatch to the same slot)"
        :to "try_acquire_guard RAII, held across state.pty.send and dropped on continue/break/end-of-iteration; per-slot scope means other slots remain dispatchable concurrently"
        :rationale "Wave33 race surface: spawner.initial_prompt fire-and-forget already occupied the slot, then Autopilot released the guard a tick before pty.send returned, leaving room for a duplicate dispatch and an orphaned BoardTask.")
     :close_helper "decide_close_action(current_status: Option<BoardTaskStatus>) -> DispatchCloseAction"
     :close_action_rules
       ["Some(Done) → AlreadySelfClosed (worker self-closed via attached board MCP tools — preserve, only log)."
        "Some(Blocked) → PreserveBlocked (mission_question_create marked the task blocked — add a note, do NOT overwrite)."
        "Some(Open|Running|Verifying|Failed|Skipped) | None → OwnerClosesAsDone (Autopilot is the close owner; transition running→done, publish StatusChanged, record success outcome)."]
     :preserved_behavior
       ["wave32 timeout-budget projection (derive_pty_timeout_secs / derive_pty_timeout_ms / idle_watchdog_threshold_secs) — unchanged."
        "wave33 prompt/tool wording (build_base_prompt objective dedupe, append_board_task_id_suffix conditional self-close) — unchanged."
        "wave33 close-owner branches for Done self-close and Blocked questions — preserved bit-for-bit through decide_close_action."
        "Auth-error and quota-exhaustion branches inside the Ok(res) arm — unchanged."
        "Retry/failure tracking and KB attribution paths inside the Err(e) arm — unchanged."]
     :test_coverage
       [(:name "decide_close_action_preserves_self_close_done"
         :proves "Done status maps to AlreadySelfClosed — worker's self-close is preserved on pty.send return.")
        (:name "decide_close_action_preserves_blocked_question_state"
         :proves "Blocked status maps to PreserveBlocked — mission_question_create state is never overwritten with done.")
        (:name "decide_close_action_owner_closes_running_or_open"
         :proves "Running and Open both map to OwnerClosesAsDone — Autopilot is the close owner on the normal path.")
        (:name "decide_close_action_owner_closes_when_status_unknown"
         :proves "None status (DB lookup miss) still maps to OwnerClosesAsDone — matches the legacy `_ =>` arm and avoids a new orphan path.")])

  :preserved_behavior
    ["wave32-01 timeout-budget projection (PTY_TIMEOUT_DEFAULT_SECS / MIN / MAX, derive_pty_timeout_*, idle_watchdog_threshold_secs, WATCHDOG_MISSING_SESSION_PROBE_SECS) — unchanged."
     "wave33-01 prompt-tool-contract wording (objective dedupe, conditional board self-close, Decision Engine escalation suffix, ops focus suffix) — unchanged in autopilot.rs and re-stated in V3 blueprint."
     "Direct `mission_compute_slot create` callers omit suppress_initial_prompt and keep the legacy warm-up behaviour (objective_owned still becomes PTYSpawnOptions.initial_prompt). Only the delegated auto-provision path opts in to suppression."
     "task_delegate's existing flow (idle-slot reuse via find_and_reserve_slot, dynamic-slot quota, intent whitelist, context-hint injection, BoardTask creation, board_dispatch_notify) — unchanged; only the create_args construction was refactored into a pure helper."]

  :notes
    "Three pure helpers were extracted so the V3 execution-ownership rule has a unit-testable Rust projection without constructing AppState: compute_slot::effective_initial_prompt (initial-prompt suppression), task_delegate::build_compute_slot_create_args (create-args shape), engine::intent_engine::autopilot::decide_close_action (close-action policy). The autopilot dispatch guard now uses the existing slot_dispatch::try_acquire_guard RAII path so the guard auto-releases on every continue/break/end-of-iteration without requiring a fourth helper. SlotAcquireGuard contains only `&SlotDispatchGuard + String` (Send + Sync), so holding it across the async pty.send call is safe and does not introduce any new Send/Sync requirements."

  :trace_references
    [".missiond/tasks/wave34/shared-memory.lisp :: wave34-01-claim-003"
     ".missiond/tasks/wave34/session-trace.lisp :: wave34-01-trace-preamble-read-004"])
