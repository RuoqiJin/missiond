;; ════════════════════════════════════════════════════════════════
;; MissionD RPC Gateway Declaration
;; Pattern: rpc-gateway (generalized interface layer)
;; Status: Phase 2 — MCP protocol layer
;; ════════════════════════════════════════════════════════════════

(rpc-gateway missiond-mcp
  :target "missiond-mcp/src/gen_server.rs"

  ;; Supported transports: MCP uses stdio, daemon uses IPC
  (transports stdio ipc)

  ;; JSON-RPC method routes
  (methods
    (method "initialize"
      :handler handle_initialize
      :output Value
      :description "MCP protocol initialization handshake")

    (method "notifications/initialized"
      :handler handle_notifications_initialized
      :output Value
      :description "Client confirms initialization complete")

    (method "tools/list"
      :handler handle_tools_list
      :output Value
      :description "List all available tool definitions with JSON Schema")

    (method "tools/call"
      :handler handle_tools_call
      :input ((name String :required) (arguments Value))
      :output Value
      :description "Execute a tool by name with arguments")

    (method "ping"
      :handler handle_ping
      :output Value
      :description "Health check ping/pong"))

  ;; Tool dispatch routing — tool_name → handler module
  (dispatch
    ;; Compute domain
    (group task
      (tools mission_task_submit mission_task_query mission_task_cancel
             mission_task_delegate))
    (group pty
      (tools mission_pty_spawn mission_pty_send mission_pty_read
             mission_pty_status mission_pty_screenshot mission_pty_confirm
             mission_pty_signal))
    (group slot
      (tools mission_slots mission_slot_history mission_compute_slot
             mission_agents))
    (group cc_tasks
      (tools mission_cc_query mission_cc_swarm))
    (group worker
      (tools mission_worker))
    (group job
      (tools mission_job_poll))

    ;; Knowledge domain
    (group kb :prefix "mission_kb_")
    (group board :prefix "mission_board_"
      (tools mission_board_create mission_board_query mission_board_update
             mission_board_delete mission_board_claim mission_board_decompose
             mission_board_note_add mission_board_retry))
    (group skill
      (tools mission_skill_query mission_skill_exec mission_skill_mutate
             mission_skill_context))
    (group memory
      (tools mission_memory))
    (group insight
      (tools mission_insight))
    (group embedding
      (tools mission_embedding_ops))

    ;; Communication domain
    (group conversation :prefix "mission_conversation_")
    (group question
      (tools mission_question))
    (group router_chat
      (tools mission_router_chat mission_router_chat_manage))
    (group timeline
      (tools mission_timeline))
    (group audit
      (tools mission_audit))
    (group retrospective
      (tools mission_retrospective_manage))
    (group decision
      (tools mission_decision_stats))

    ;; System/Infra domain
    (group infra
      (tools mission_infra_query mission_infra_ops))
    (group permission
      (tools mission_permission_query mission_permission_mutate))
    (group system
      (tools mission_sys_config mission_sys_logs mission_power_control
             mission_control mission_pause))

    ;; External MCP proxy
    (group external :prefix "xjp_"))

  ;; Error code constants
  (error-codes
    UNKNOWN_TOOL UNKNOWN_ACTION MISSING_PARAM INVALID_PARAM
    NOT_FOUND PERMISSION_DENIED IPC_TIMEOUT SPAWN_FAILED DB_ERROR))
