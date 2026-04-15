;; MissionD — Pillar: engines
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

  (pillar engines
    (purpose "composite orchestration: autopilot + learning + slot lifecycle")

    (component autopilot-engine
      :target "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
      (tick-pipeline
        memory-scheduler -> extraction-check -> board-task-dispatch
        -> flow-progression -> supervision-check)
      (depends (db slot_manager event_bus llm_gateway context_pipeline)))

    (component flow-engine
      :target "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
      (depends (db slot_manager))
      (note "V1 flow engine — project-lifecycle phases tied to autopilot tick, NOT the new Flow Engine v2 below"))

    ;; ── Flow Engine v2 (commit 49bd316, 2026-04-14) ────────
    ;; General-purpose declarative node-based workflow orchestration,
    ;; independent of autopilot + project lifecycle. YAML-driven.
    (component flow-engine-v2
      :target "crates/missiond-daemon/src/engine/flow/mod.rs"
      :added "49bd316"
      :purpose "Declarative YAML→node-sequence executor reusing existing LLM/slot/MCP primitives"

      (module mod.rs
        :lines 171
        (type NodeType :tag "type" :variants
          (LlmCall            :fields (provider prompt max_tokens=65536))
          (SlotTask           :fields (model=opus prompt timeout_secs=3600))
          (McpTool            :fields (tool_name params))
          (DaemonAction       :fields (action params))
          (ParallelSlotTasks  :fields (parallelism=3 "tasks:Vec<ParallelTaskSpec>" gather=Aggregate timeout_secs=1800)
            :added "49bd316"
            :doc "fan-out N prompts to M running slots with bounded parallelism; POC fire-and-forget"))
        (type ParallelTaskSpec :fields (id prompt))
        (type GatherStrategy :variants (Aggregate AllSuccess AnySuccess) :default Aggregate
          :doc "Aggregate=collect all; AllSuccess=fail if any fails; AnySuccess=fail only if all fail")
        (type ErrorPolicy :variants (Stop Skip Retry(u32)) :default Stop)
        (type FlowNode    :fields (id node_type save_as depends_on on_error))
        (type FlowDefinition :fields (id name nodes))
        (type FlowContext :fields (vars:HashMap<String,String> current_node completed_nodes last_error)
          (method resolve_vars "template ${key} interpolation from ctx.vars")
          (method set/get)))

      (module runner.rs
        :lines 141
        :target "crates/missiond-daemon/src/engine/flow/runner.rs"
        (function run_flow
          :signature "fn run_flow<'a>(state, flow, ctx, task_id) -> Pin<Box<dyn Future>>"
          :why-boxed "Box::pin breaks async recursion cycle: run_flow → execute_node → dispatch_tool → flow_run::handle → run_flow"
          :loop "iterate flow.nodes skip(ctx.current_node), execute_with_retry, save_as→ctx.vars, completed_nodes.push, persist_context on each node"
          :on-error "ctx.last_error = Some(e.to_string()); persist; return Err")
        (function execute_with_retry
          :retries "ErrorPolicy::Retry(N) → 2^attempt secs exponential backoff"
          :skip "ErrorPolicy::Skip → warn + Ok(String::new())")
        (function persist_context
          :sink "store.update_board_task(task_id, UpdateBoardTaskInput { flow_context: Some(json), .. })"))

      (module handlers.rs
        :lines 317
        :target "crates/missiond-daemon/src/engine/flow/handlers.rs"
        (node-dispatch
          (LlmCall
            "gemini"  → "llm_gateway::call_gemini_for_flow(state, task_id, prompt)"
            "sonnet"  → "llm_gateway::call_sonnet_stateless(state, 'architecture reviewer', prompt, max_tokens, 'flow_node')"
            (fail-fast "unknown provider → anyhow err"))
          (SlotTask
            :selection "state.mission.list_slots() first non-excluded"
            (const EXCLUDED_ROLES #("memory" "supervisor" "strategy"))
            :precondition "slot must be running (SessionState != Exited) else fail-fast 'Start it first via mission_pty_spawn or slots.yaml auto_start'"
            :send "state.pty.send_fire_and_forget(slot_id, prompt)"
            :output "SlotTask dispatched to {slot_id} (timeout={n}s)")
          (ParallelSlotTasks
            :added "49bd316"
            :selection "list_slots() → filter non-excluded + Running (SessionState != Exited)"
            :parallelism "effective = min(parallelism, candidates.len(), tasks.len()).max(1) — prevents slot starvation deadlock"
            :dispatch "JoinSet + Arc<Semaphore>(effective) → round-robin slot assignment (idx % candidates.len())"
            :send "pty.send_fire_and_forget per task (fire-and-forget POC; Phase 2: tokio::time::timeout + SlotBecameIdle result reflow)"
            :gather "GatherStrategy: Aggregate→json array; AllSuccess→fail if any None; AnySuccess→fail only if all fail"
            :output "JSON array of per-task dispatch receipts, preserving task order")
          (McpTool
            :delegate "handlers::dispatch_tool(state, tool_name, params)"
            :output "concat text contents, propagate is_error")
          (DaemonAction
            "read_intent_lisp" → "dispatch_tool('mission_intent', {action:'read', project})"
            "close_flow"       → "update_board_task(task_id, status='done')"
            (fail-fast "unknown action → anyhow err"))))

      (module loader.rs
        :lines 77
        :target "crates/missiond-daemon/src/engine/flow/loader.rs"
        (function load_flow :path "$MISSIOND_HOME/flows/{flow_id}.yaml" :parser "serde_yaml::from_str::<FlowDefinition>")
        (function list_flows :scan "$MISSIOND_HOME/flows/*.{yaml,yml}"))

      (db-reuse
        :table board_tasks
        :columns (flow_template flow_phase flow_context)
        :note "v2 recycles existing flow_* columns; no migration needed")

      (handler
        :target "crates/missiond-daemon/src/handlers/compute/flow_run.rs"
        :lines 98
        (action list
          :call "loader::list_flows()"
          :output-json ({flows [...]} count))
        (action status
          :arg task_id
          :call "store.get_board_task → task.flow_context JSON parse → FlowContext"
          :output-json (task_id flow_phase status context))
        (action run
          :arg flow_id
          :arg params
          :pipeline (load_flow → create_board_task(category='flow',flow_template=id)
                     → ctx.set from params
                     → update_board_task(flow_phase='running',status='running',flow_context=json)
                     → run_flow(inline-await)
                     → on-ok update(flow_phase='completed',status='done')
                     → on-err update(flow_phase='failed',status='failed'))
          :known-limit "inline blocking — background spawn deferred pending Send bound resolution across dispatch_tool→run_flow→execute_node→dispatch_tool chain"))

      (mcp-tool
        :name mission_flow_run
        :target "crates/missiond-mcp/src/tools/compute/flow_run.rs"
        :description "Flow Engine v2: execute declarative node-based workflows. run=execute, list=available flows, status=check running flow"
        :required (flow_id)
        :properties (flow_id params action[run|list|status]=run task_id)
        :gen-gateway-route "dispatch_tool: 'mission_flow_run' → handler.handle_worker(name,args) (reuses worker pipe transport)")

      (depends (db slot_manager llm_gateway mcp_dispatch pty_manager))

      (fail-fast-invariants
        "Slot not running → immediate err (no auto-spawn)"
        "Unknown LLM provider → immediate err (no fallback)"
        "Unknown daemon action → immediate err (no noop)"
        "Flow YAML not found → immediate err"
        "ParallelSlotTasks empty tasks → immediate err"
        "ParallelSlotTasks no running non-excluded slots → immediate err"))

    (component memory-scheduler
      :target "crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs"
      (depends db))

    (component workflow-executor
      :target "crates/missiond-daemon/src/engine/intent_engine/workflow_executor.rs"
      (depends (db slot_manager)))

    (component learning-engine
      :target "crates/missiond-daemon/src/engine/learning_engine/mod.rs"
      (sub-engines
        (decision-engine
          :target "crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs"
          (decision-cascade kb-lookup -> gemini-consult -> decision-slot -> human-escalation))
        (extraction
          :target "crates/missiond-daemon/src/engine/learning_engine/extraction.rs")
        (decision-harvest
          :target "crates/missiond-daemon/src/engine/learning_engine/decision_harvest.rs")
        (intent-analyst
          :target "crates/missiond-daemon/src/engine/learning_engine/intent_analyst.rs")
        (timeline-analyst
          :target "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs")
        (idle-explorer
          :target "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs")
        (historical-scanner
          :target "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs")))

    (component slot-orchestrator
      :target "crates/missiond-daemon/src/slot_orchestrator/mod.rs"
      (engine-adapters
        (cc-controller    :target "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs")
        (gemini-controller :target "crates/missiond-daemon/src/slot_orchestrator/gemini_controller.rs"))
      (spawner :target "crates/missiond-daemon/src/slot_orchestrator/spawner.rs"

        (invariant sole-spawn-bottleneck
          :function "spawn_tracked_slot"
          :enforced "ALL 10 spawn paths go through this function; 0 direct pty.spawn() calls exist"
          :callers ("pty::mission_pty_spawn" "compute_slot::create_slot" "process::spawn+restart"
                    "task::auto_spawn_exited" "flow_engine::ensure_slot_for_task"
                    "memory_scheduler::ensure_memory_slot" "gemini_driver::ensure_spawned"
                    "main::handle_slots_reload" "cc_controller::spawn_and_register")
          :pipeline (perm-inject → tracking-env → pty-spawn → uuid-capture → initial-prompt))

        (slot-config-fields
          (initial_prompt :type "Option<String>" :doc "first message injected after slot reaches Idle")
          (dangerously_skip_permissions :type bool :serde-alias "dangerouslySkipPermissions")
          (mcp_config :type "Option<McpConfig>" :serde-alias "mcpConfig")
          (auto_start :type bool :serde-alias "autoStart")))

      (perm-injector :target "crates/missiond-daemon/src/slot_orchestrator/perm_injector.rs"
        :added "ec269d7"
        :updated "2026-04-12"
        :invoked-by "spawn_tracked_slot (inside the sole-spawn-bottleneck, covers all 10 paths)"
        :doc "before each slot spawn, reads global+role+project+slot union from learned_permissions.yaml (LearnedPermissions::get_for_spawn) and merges into <cwd>/.claude/settings.local.json (idempotent, preserves existing entries, dedups on (tool_pattern, param_pattern))")

      ;; commit 79a877f: added lisp_survey task (4th registered task in main.rs)
      (registered-tasks
        ;; main.rs: "SlotManager: 4 tasks registered (arch_maintenance, strategy_analyst, gemini_router, lisp_survey)"
        (task arch_maintenance  :slot-id "arch-surveyor"   :model sonnet  :timeout 900s)
        (task strategy_analyst  :slot-id "strategy"        :model gemini  :timeout 900s)
        (task gemini_router     :slot-id "gemini-router"   :model gemini  :timeout 900s)
        (task lisp_survey       :slot-id "lisp-surveyor"   :model sonnet  :timeout 900s :added "79a877f"))

      (depends (pty_manager slot_manager event_bus)))

    ;; commit ec269d7: NEW — LearnedPermissions authority
    ;; updated 2026-04-12 — Phase 1-5 architecture upgrade
    (component learned-permissions
      :target "crates/missiond-core/src/core/learned_permissions.rs"
      :updated "2026-04-12"
      (change REQUIRES_PARAM_PATTERN
        :semantics "bare Bash (no param_pattern) rejected as too dangerous; specific subcommand patterns (python3:*, npm test:*) persisted"
        :extract-fn "permission_extract::extract_confirm() — parses 'Yes, don't ask again for: python3:*' → ExtractedConfirm{pattern, project_path} incl. 'commands in <path>' suffix")

      ;; Multi-scope model: scope_type ∈ {global, role, project, slot}
      ;; Precedence at spawn time: slot > project > role > global (more specific wins on dedup)
      (scope-model
        (global  "scope_id=\"\" — applies to every spawn")
        (role    "scope_id=<role> — role-wide learned permissions")
        (project "scope_id=<project_id> — resolved via ProjectRegistry::resolve(cwd)")
        (slot    "scope_id=<slot_id> — per-slot overrides"))

      (method get_for_spawn
        :args "role: &str, project_id: Option<&str>, slot_id: Option<&str>"
        :returns "Vec<LearnedPermission>"
        :doc "union across all applicable scopes with later-wins dedup on (tool_pattern, param_pattern)")

      ;; Single write path (Phase 1): handle_confirm_required is the source of truth
      ;; for learning. mission_pty_confirm (manual MCP path) ALSO learns, for symmetry,
      ;; but the auto-approve branch in pty_event_worker is the 99% case.
      (flow permission-persistence
        :steps
        ("pty_event_worker::handle_confirm_required: auto-approve enabled"
         "→ opt2 text contains 'don't ask again'/'always'/'trust'/'不再' → use_allowlist=true"
         "→ permission_extract::extract_confirm(opt2) → ExtractedConfirm{pattern, project_path}"
         "→ LearnedPermissions::learn(role, role_id, tool, allow, pattern) [always]"
         "→ if project_path Some → ProjectRegistry::resolve → LearnedPermissions::learn(project, pid, tool, allow, pattern)"
         "→ ConfirmResponse::Option(2) sent as two sequential PTY writes (digit + Enter, 80ms apart)"
         "→ next spawn: perm_injector::sync_learned_to_local_settings(cwd, role, project_id, slot_id, learned)"
         "→ allowlist persists across slot spawns"))
      (tests "Phase 1-5 coverage: 10 permission_extract tests + 1 E2E pty confirm test + 1 get_for_spawn union/dedup test + 4 perm_injector tests (incl. multi_scope_union)")

      ;; Shared extraction module (Phase 1)
      (component permission-extract
        :target "crates/missiond-daemon/src/permission_extract.rs"
        :added "2026-04-12"
        :doc "single source of truth for parsing Claude Code confirm dialog option text; consumed by both mission_pty_confirm (MCP) and pty_event_worker::handle_confirm_required (auto-approve)"))

    ;; Phase 4: merged_for_slot MCP view
    (component permission-mcp-merged-view
      :target "crates/missiond-daemon/src/handlers/sysinfra/permission.rs"
      :added "2026-04-12"
      (tool mission_permission_query
        :action merged_for_slot
        :args (slot_id)
        :returns "{slotId, role, cwd, projectId, learned: [LearnedPermission], staticRoleRule, staticSlotRule}"
        :doc "shows the exact union of permission entries a given slot would see at spawn time — debugging/audit view")))

