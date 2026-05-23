(behavior-universe missiond
  :schema "missiond.behavior-universe.v1"
  :project missiond
  :status code-aligned
  :owner typed-lisp-compiler
  :rule "MissionD program-level SSOT closure: every observed active behavior class discovered from code must be claimed here or tombstoned; generated observed JSON is diagnostic evidence, not editable SSOT."

  (behavior
    :id missiond-background-workers
    :kind worker
    :owner runtime-workers
    :observed ["worker:*"]
    :code ["crates/missiond-daemon/src/workers/**"
           "crates/missiond-daemon/src/workers/mod.rs"]
    :effects [])

  (behavior
    :id missiond-background-tasks
    :kind scheduler
    :owner runtime-orchestrator
    :observed ["background-task:*" "scheduler:*"]
    :code ["crates/**/src/**/*.rs"
           "scripts/**/*.mjs"
           "packages/**/*.ts"
           "packages/**/*.tsx"]
    :effects [])

  (behavior
    :id missiond-public-tools
    :kind mcp-tool
    :owner mcp-gateway
    :observed ["mcp-tool:*"]
    :code ["crates/missiond-mcp/src/tools/**"
           "crates/missiond-mcp/src/gen_gateway.rs"]
    :effects [mission-global-instruction-write])

  (behavior
    :id missiond-routes-and-cli
    :kind route
    :owner runtime-api
    :observed ["route:*" "cli:*"]
    :code ["crates/**/src/**/*.rs"
           "packages/**/src/**/*.ts"
           "packages/**/src/**/*.tsx"
           "scripts/**/*.mjs"]
    :effects [])

  (behavior
    :id missiond-database-mutations
    :kind db-write
    :owner persistence
    :observed ["db-write:*"]
    :code ["crates/missiond-core/src/db/**"
           "crates/missiond-daemon/src/handlers/**"
           "crates/missiond-daemon/src/engine/**"]
    :effects [])

  (behavior
    :id missiond-process-and-network-io
    :kind subprocess
    :owner runtime-io
    :observed ["subprocess:*" "network:*" "model-call:*"]
    :code ["crates/**/src/**/*.rs"
           "scripts/**/*.mjs"
           "packages/**/*.ts"
           "packages/**/*.tsx"]
    :effects [])

  (behavior
    :id global-claude-md-sync
    :kind effect
    :owner context-runtime
    :observed ["effect:fs-write:crates/missiond-daemon/src/context/claude_md_sync.rs:*"]
    :code ["crates/missiond-daemon/src/context/claude_md_sync.rs"]
    :effects [global-claude-md-managed-section]
    (anchor
      :role entry
      :observed "effect:fs-write:crates/missiond-daemon/src/context/claude_md_sync.rs:169"
      :file "crates/missiond-daemon/src/context/claude_md_sync.rs"
      :symbol "sync_claude_md"
      :effect global-claude-md-managed-section)
    (anchor
      :role effect-site
      :observed "effect:fs-write:crates/missiond-daemon/src/context/claude_md_sync.rs:169"
      :file "crates/missiond-daemon/src/context/claude_md_sync.rs"
      :symbol "sync_claude_md"
      :effect global-claude-md-managed-section)
    (trigger
      :from-file "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
      :from-symbol "autopilot_tick"
      :calls "sync_claude_md"))

  (behavior
    :id mission-global-instruction-manager
    :kind effect
    :owner sysinfra-control
    :observed ["effect:fs-write:crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs:*"]
    :code ["crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs"
           "crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs"]
    :effects [mission-global-instruction-write]
    (anchor
      :role tool
      :observed "mcp-tool:mission_global_instruction"
      :file "crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs"
      :symbol "mission_global_instruction")
    (anchor
      :role entry
      :observed "effect:fs-write:crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs:121"
      :file "crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs"
      :symbol "production_atomic_write"
      :effect mission-global-instruction-write)
    (anchor
      :role effect-site
      :observed "effect:fs-write:crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs:121"
      :file "crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs"
      :symbol "production_atomic_write"
      :effect mission-global-instruction-write))

  (behavior
    :id missiond-filesystem-effects
    :kind effect
    :owner runtime-io
    :observed ["effect:fs-write:*" "effect:fs-append:*" "effect:fs-rename:*" "effect:fs-delete:*"]
    :code ["crates/**/src/**/*.rs"
           "scripts/**/*.mjs"
           "packages/**/*.ts"
           "packages/**/*.tsx"]
    :effects [global-claude-md-managed-section
              mission-global-instruction-write
              xjpcode-briefing-write
              project-vault-sync-write
              gemini-shadow-settings-write
              missiond-repo-file-write
              missiond-repo-file-append
              missiond-repo-file-rename
              missiond-repo-file-delete])

  ;; BEGIN GENERATED NAVIGATION ANCHORS
  (behavior
    :id missiond-navigation-effect
    :kind effect
    :owner navigation-gate
    :observed ["effect:fs-write:crates/missiond-daemon/src/context/claude_md_sync.rs:169"
              "effect:fs-write:crates/missiond-daemon/src/handlers/knowledge/project/vault.rs:68"
              "effect:fs-write:crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs:121"
              "effect:fs-write:crates/missiond-daemon/src/llm/gemini_cli.rs:999"
              "effect:fs-write:crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs:63"]
    :code ["crates/missiond-daemon/src/context/claude_md_sync.rs"
          "crates/missiond-daemon/src/handlers/knowledge/project/vault.rs"
          "crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs"
          "crates/missiond-daemon/src/llm/gemini_cli.rs"
          "crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs"]
    :effects [gemini-shadow-settings-write
             global-claude-md-managed-section
             mission-global-instruction-write
             project-vault-sync-write
             xjpcode-briefing-write]
    (anchor
      :role effect-site
      :observed "effect:fs-write:crates/missiond-daemon/src/context/claude_md_sync.rs:169"
      :file "crates/missiond-daemon/src/context/claude_md_sync.rs"
      :symbol "sync_claude_md"
      :effect global-claude-md-managed-section)
    (anchor
      :role effect-site
      :observed "effect:fs-write:crates/missiond-daemon/src/handlers/knowledge/project/vault.rs:68"
      :file "crates/missiond-daemon/src/handlers/knowledge/project/vault.rs"
      :symbol "handle_vault_sync"
      :effect project-vault-sync-write)
    (anchor
      :role effect-site
      :observed "effect:fs-write:crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs:121"
      :file "crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs"
      :symbol "production_atomic_write"
      :effect mission-global-instruction-write)
    (anchor
      :role effect-site
      :observed "effect:fs-write:crates/missiond-daemon/src/llm/gemini_cli.rs:999"
      :file "crates/missiond-daemon/src/llm/gemini_cli.rs"
      :symbol "ensure_auth_home"
      :effect gemini-shadow-settings-write)
    (anchor
      :role effect-site
      :observed "effect:fs-write:crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs:63"
      :file "crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs"
      :symbol "generate_and_write"
      :effect xjpcode-briefing-write))

  (behavior
    :id missiond-navigation-mcp-tool
    :kind mcp-tool
    :owner navigation-gate
    :observed ["mcp-tool:mission_agent"
              "mcp-tool:mission_audit"
              "mcp-tool:mission_beacon"
              "mcp-tool:mission_board_claim"
              "mcp-tool:mission_board_create"
              "mcp-tool:mission_board_decompose"
              "mcp-tool:mission_board_delete"
              "mcp-tool:mission_board_note_add"
              "mcp-tool:mission_board_query"
              "mcp-tool:mission_board_retry"
              "mcp-tool:mission_board_update"
              "mcp-tool:mission_capability_usage"
              "mcp-tool:mission_cascade_lint"
              "mcp-tool:mission_cascade_plan"
              "mcp-tool:mission_cascade_trigger"
              "mcp-tool:mission_cc_query"
              "mcp-tool:mission_cc_swarm"
              "mcp-tool:mission_claim_status"
              "mcp-tool:mission_code_search"
              "mcp-tool:mission_codex_ops"
              "mcp-tool:mission_codex_replay"
              "mcp-tool:mission_compute_slot"
              "mcp-tool:mission_context_boot"
              "mcp-tool:mission_context_gather"
              "mcp-tool:mission_context_slice"
              "mcp-tool:mission_control"
              "mcp-tool:mission_convergence_status"
              "mcp-tool:mission_conversation_analyze"
              "mcp-tool:mission_conversation_query"
              "mcp-tool:mission_conversation_reconcile"
              "mcp-tool:mission_daemon_update"
              "mcp-tool:mission_decision_stats"
              "mcp-tool:mission_directive"
              "mcp-tool:mission_embedding_ops"
              "mcp-tool:mission_execution"
              "mcp-tool:mission_flow_run"
              "mcp-tool:mission_forge_build"
              "mcp-tool:mission_forge_lint"
              "mcp-tool:mission_gemini_auth"
              "mcp-tool:mission_global_instruction"
              "mcp-tool:mission_inbox"
              "mcp-tool:mission_incident"
              "mcp-tool:mission_infra_ops"
              "mcp-tool:mission_infra_query"
              "mcp-tool:mission_insight"
              "mcp-tool:mission_intent"
              "mcp-tool:mission_job_poll"
              "mcp-tool:mission_kb_mutate"
              "mcp-tool:mission_kb_ops"
              "mcp-tool:mission_kb_query"
              "mcp-tool:mission_kb_remember"
              "mcp-tool:mission_kb_review"
              "mcp-tool:mission_llm_trace"
              "mcp-tool:mission_master_status"
              "mcp-tool:mission_memory"
              "mcp-tool:mission_minimax_process"
              "mcp-tool:mission_nightly_evolution"
              "mcp-tool:mission_pause"
              "mcp-tool:mission_permission_mutate"
              "mcp-tool:mission_permission_query"
              "mcp-tool:mission_plan"
              "mcp-tool:mission_power_control"
              "mcp-tool:mission_project"
              "mcp-tool:mission_pty_confirm"
              "mcp-tool:mission_pty_read"
              "mcp-tool:mission_pty_screenshot"
              "mcp-tool:mission_pty_send"
              "mcp-tool:mission_pty_signal"
              "mcp-tool:mission_pty_spawn"
              "mcp-tool:mission_pty_status"
              "mcp-tool:mission_question"
              "mcp-tool:mission_request"
              "mcp-tool:mission_retrospective_manage"
              "mcp-tool:mission_router_chat"
              "mcp-tool:mission_router_chat_manage"
              "mcp-tool:mission_shared_memory"
              "mcp-tool:mission_skill_context"
              "mcp-tool:mission_skill_exec"
              "mcp-tool:mission_skill_mutate"
              "mcp-tool:mission_skill_query"
              "mcp-tool:mission_slot_history"
              "mcp-tool:mission_slots"
              "mcp-tool:mission_sonnet_process"
              "mcp-tool:mission_submit_phase_result"
              "mcp-tool:mission_swarm_run"
              "mcp-tool:mission_sys_config"
              "mcp-tool:mission_sys_logs"
              "mcp-tool:mission_task_cancel"
              "mcp-tool:mission_task_delegate"
              "mcp-tool:mission_task_query"
              "mcp-tool:mission_task_submit"
              "mcp-tool:mission_timeline"
              "mcp-tool:mission_tool_directory"
              "mcp-tool:mission_universe_graph"
              "mcp-tool:mission_worker"
              "mcp-tool:mission_workflow"]
    :code ["crates/missiond-mcp/src/tools/comm/audit.rs"
          "crates/missiond-mcp/src/tools/comm/capability_usage.rs"
          "crates/missiond-mcp/src/tools/comm/codex_ops.rs"
          "crates/missiond-mcp/src/tools/comm/codex_replay.rs"
          "crates/missiond-mcp/src/tools/comm/conversation.rs"
          "crates/missiond-mcp/src/tools/comm/question.rs"
          "crates/missiond-mcp/src/tools/comm/router_chat.rs"
          "crates/missiond-mcp/src/tools/comm/timeline.rs"
          "crates/missiond-mcp/src/tools/comm/tool_directory.rs"
          "crates/missiond-mcp/src/tools/compute/cc_tasks.rs"
          "crates/missiond-mcp/src/tools/compute/compute_slot.rs"
          "crates/missiond-mcp/src/tools/compute/flow_run.rs"
          "crates/missiond-mcp/src/tools/compute/forge.rs"
          "crates/missiond-mcp/src/tools/compute/job.rs"
          "crates/missiond-mcp/src/tools/compute/minimax.rs"
          "crates/missiond-mcp/src/tools/compute/process.rs"
          "crates/missiond-mcp/src/tools/compute/pty.rs"
          "crates/missiond-mcp/src/tools/compute/slot.rs"
          "crates/missiond-mcp/src/tools/compute/task.rs"
          "crates/missiond-mcp/src/tools/compute/task_delegate.rs"
          "crates/missiond-mcp/src/tools/compute/worker.rs"
          "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
          "crates/missiond-mcp/src/tools/knowledge/board.rs"
          "crates/missiond-mcp/src/tools/knowledge/cascade.rs"
          "crates/missiond-mcp/src/tools/knowledge/context_gather.rs"
          "crates/missiond-mcp/src/tools/knowledge/directive.rs"
          "crates/missiond-mcp/src/tools/knowledge/insight.rs"
          "crates/missiond-mcp/src/tools/knowledge/intent.rs"
          "crates/missiond-mcp/src/tools/knowledge/kb.rs"
          "crates/missiond-mcp/src/tools/knowledge/memory.rs"
          "crates/missiond-mcp/src/tools/knowledge/plan.rs"
          "crates/missiond-mcp/src/tools/knowledge/project.rs"
          "crates/missiond-mcp/src/tools/knowledge/request.rs"
          "crates/missiond-mcp/src/tools/knowledge/shared_memory.rs"
          "crates/missiond-mcp/src/tools/knowledge/skill.rs"
          "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
          "crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs"
          "crates/missiond-mcp/src/tools/sysinfra/infra.rs"
          "crates/missiond-mcp/src/tools/sysinfra/permission.rs"
          "crates/missiond-mcp/src/tools/sysinfra/power.rs"
          "crates/missiond-mcp/src/tools/sysinfra/system.rs"]
    :effects []
    (anchor
      :role tool
      :observed "mcp-tool:mission_agent"
      :file "crates/missiond-mcp/src/tools/compute/process.rs"
      :symbol "mission_agent")
    (anchor
      :role tool
      :observed "mcp-tool:mission_audit"
      :file "crates/missiond-mcp/src/tools/comm/audit.rs"
      :symbol "mission_audit")
    (anchor
      :role tool
      :observed "mcp-tool:mission_beacon"
      :file "crates/missiond-mcp/src/tools/knowledge/kb.rs"
      :symbol "mission_beacon")
    (anchor
      :role tool
      :observed "mcp-tool:mission_board_claim"
      :file "crates/missiond-mcp/src/tools/knowledge/board.rs"
      :symbol "mission_board_claim")
    (anchor
      :role tool
      :observed "mcp-tool:mission_board_create"
      :file "crates/missiond-mcp/src/tools/knowledge/board.rs"
      :symbol "mission_board_create")
    (anchor
      :role tool
      :observed "mcp-tool:mission_board_decompose"
      :file "crates/missiond-mcp/src/tools/knowledge/board.rs"
      :symbol "mission_board_decompose")
    (anchor
      :role tool
      :observed "mcp-tool:mission_board_delete"
      :file "crates/missiond-mcp/src/tools/knowledge/board.rs"
      :symbol "mission_board_delete")
    (anchor
      :role tool
      :observed "mcp-tool:mission_board_note_add"
      :file "crates/missiond-mcp/src/tools/knowledge/board.rs"
      :symbol "mission_board_note_add")
    (anchor
      :role tool
      :observed "mcp-tool:mission_board_query"
      :file "crates/missiond-mcp/src/tools/knowledge/board.rs"
      :symbol "mission_board_query")
    (anchor
      :role tool
      :observed "mcp-tool:mission_board_retry"
      :file "crates/missiond-mcp/src/tools/knowledge/board.rs"
      :symbol "mission_board_retry")
    (anchor
      :role tool
      :observed "mcp-tool:mission_board_update"
      :file "crates/missiond-mcp/src/tools/knowledge/board.rs"
      :symbol "mission_board_update")
    (anchor
      :role tool
      :observed "mcp-tool:mission_capability_usage"
      :file "crates/missiond-mcp/src/tools/comm/capability_usage.rs"
      :symbol "mission_capability_usage")
    (anchor
      :role tool
      :observed "mcp-tool:mission_cascade_lint"
      :file "crates/missiond-mcp/src/tools/knowledge/cascade.rs"
      :symbol "mission_cascade_lint")
    (anchor
      :role tool
      :observed "mcp-tool:mission_cascade_plan"
      :file "crates/missiond-mcp/src/tools/knowledge/cascade.rs"
      :symbol "mission_cascade_plan")
    (anchor
      :role tool
      :observed "mcp-tool:mission_cascade_trigger"
      :file "crates/missiond-mcp/src/tools/knowledge/cascade.rs"
      :symbol "mission_cascade_trigger")
    (anchor
      :role tool
      :observed "mcp-tool:mission_cc_query"
      :file "crates/missiond-mcp/src/tools/compute/cc_tasks.rs"
      :symbol "mission_cc_query")
    (anchor
      :role tool
      :observed "mcp-tool:mission_cc_swarm"
      :file "crates/missiond-mcp/src/tools/compute/cc_tasks.rs"
      :symbol "mission_cc_swarm")
    (anchor
      :role tool
      :observed "mcp-tool:mission_claim_status"
      :file "crates/missiond-mcp/src/tools/knowledge/shared_memory.rs"
      :symbol "mission_claim_status")
    (anchor
      :role tool
      :observed "mcp-tool:mission_code_search"
      :file "crates/missiond-mcp/src/tools/knowledge/kb.rs"
      :symbol "mission_code_search")
    (anchor
      :role tool
      :observed "mcp-tool:mission_codex_ops"
      :file "crates/missiond-mcp/src/tools/comm/codex_ops.rs"
      :symbol "mission_codex_ops")
    (anchor
      :role tool
      :observed "mcp-tool:mission_codex_replay"
      :file "crates/missiond-mcp/src/tools/comm/codex_replay.rs"
      :symbol "mission_codex_replay")
    (anchor
      :role tool
      :observed "mcp-tool:mission_compute_slot"
      :file "crates/missiond-mcp/src/tools/compute/compute_slot.rs"
      :symbol "mission_compute_slot")
    (anchor
      :role tool
      :observed "mcp-tool:mission_context_boot"
      :file "crates/missiond-mcp/src/tools/knowledge/context_gather.rs"
      :symbol "mission_context_boot")
    (anchor
      :role tool
      :observed "mcp-tool:mission_context_gather"
      :file "crates/missiond-mcp/src/tools/knowledge/context_gather.rs"
      :symbol "mission_context_gather")
    (anchor
      :role tool
      :observed "mcp-tool:mission_context_slice"
      :file "crates/missiond-mcp/src/tools/knowledge/shared_memory.rs"
      :symbol "mission_context_slice")
    (anchor
      :role tool
      :observed "mcp-tool:mission_control"
      :file "crates/missiond-mcp/src/tools/compute/worker.rs"
      :symbol "mission_control")
    (anchor
      :role tool
      :observed "mcp-tool:mission_convergence_status"
      :file "crates/missiond-mcp/src/tools/compute/process.rs"
      :symbol "mission_convergence_status")
    (anchor
      :role tool
      :observed "mcp-tool:mission_conversation_analyze"
      :file "crates/missiond-mcp/src/tools/comm/conversation.rs"
      :symbol "mission_conversation_analyze")
    (anchor
      :role tool
      :observed "mcp-tool:mission_conversation_query"
      :file "crates/missiond-mcp/src/tools/comm/conversation.rs"
      :symbol "mission_conversation_query")
    (anchor
      :role tool
      :observed "mcp-tool:mission_conversation_reconcile"
      :file "crates/missiond-mcp/src/tools/comm/conversation.rs"
      :symbol "mission_conversation_reconcile")
    (anchor
      :role tool
      :observed "mcp-tool:mission_daemon_update"
      :file "crates/missiond-mcp/src/tools/sysinfra/system.rs"
      :symbol "mission_daemon_update")
    (anchor
      :role tool
      :observed "mcp-tool:mission_decision_stats"
      :file "crates/missiond-mcp/src/tools/comm/question.rs"
      :symbol "mission_decision_stats")
    (anchor
      :role tool
      :observed "mcp-tool:mission_directive"
      :file "crates/missiond-mcp/src/tools/knowledge/directive.rs"
      :symbol "mission_directive")
    (anchor
      :role tool
      :observed "mcp-tool:mission_embedding_ops"
      :file "crates/missiond-mcp/src/tools/comm/conversation.rs"
      :symbol "mission_embedding_ops")
    (anchor
      :role tool
      :observed "mcp-tool:mission_execution"
      :file "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
      :symbol "mission_execution")
    (anchor
      :role tool
      :observed "mcp-tool:mission_flow_run"
      :file "crates/missiond-mcp/src/tools/compute/flow_run.rs"
      :symbol "mission_flow_run")
    (anchor
      :role tool
      :observed "mcp-tool:mission_forge_build"
      :file "crates/missiond-mcp/src/tools/compute/forge.rs"
      :symbol "mission_forge_build")
    (anchor
      :role tool
      :observed "mcp-tool:mission_forge_lint"
      :file "crates/missiond-mcp/src/tools/compute/forge.rs"
      :symbol "mission_forge_lint")
    (anchor
      :role tool
      :observed "mcp-tool:mission_gemini_auth"
      :file "crates/missiond-mcp/src/tools/comm/question.rs"
      :symbol "mission_gemini_auth")
    (anchor
      :role tool
      :observed "mcp-tool:mission_global_instruction"
      :file "crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs"
      :symbol "mission_global_instruction")
    (anchor
      :role tool
      :observed "mcp-tool:mission_inbox"
      :file "crates/missiond-mcp/src/tools/compute/process.rs"
      :symbol "mission_inbox")
    (anchor
      :role tool
      :observed "mcp-tool:mission_incident"
      :file "crates/missiond-mcp/src/tools/comm/question.rs"
      :symbol "mission_incident")
    (anchor
      :role tool
      :observed "mcp-tool:mission_infra_ops"
      :file "crates/missiond-mcp/src/tools/sysinfra/infra.rs"
      :symbol "mission_infra_ops")
    (anchor
      :role tool
      :observed "mcp-tool:mission_infra_query"
      :file "crates/missiond-mcp/src/tools/sysinfra/infra.rs"
      :symbol "mission_infra_query")
    (anchor
      :role tool
      :observed "mcp-tool:mission_insight"
      :file "crates/missiond-mcp/src/tools/knowledge/insight.rs"
      :symbol "mission_insight")
    (anchor
      :role tool
      :observed "mcp-tool:mission_intent"
      :file "crates/missiond-mcp/src/tools/knowledge/intent.rs"
      :symbol "mission_intent")
    (anchor
      :role tool
      :observed "mcp-tool:mission_job_poll"
      :file "crates/missiond-mcp/src/tools/compute/job.rs"
      :symbol "mission_job_poll")
    (anchor
      :role tool
      :observed "mcp-tool:mission_kb_mutate"
      :file "crates/missiond-mcp/src/tools/knowledge/kb.rs"
      :symbol "mission_kb_mutate")
    (anchor
      :role tool
      :observed "mcp-tool:mission_kb_ops"
      :file "crates/missiond-mcp/src/tools/knowledge/kb.rs"
      :symbol "mission_kb_ops")
    (anchor
      :role tool
      :observed "mcp-tool:mission_kb_query"
      :file "crates/missiond-mcp/src/tools/knowledge/kb.rs"
      :symbol "mission_kb_query")
    (anchor
      :role tool
      :observed "mcp-tool:mission_kb_remember"
      :file "crates/missiond-mcp/src/tools/knowledge/kb.rs"
      :symbol "mission_kb_remember")
    (anchor
      :role tool
      :observed "mcp-tool:mission_kb_review"
      :file "crates/missiond-mcp/src/tools/knowledge/kb.rs"
      :symbol "mission_kb_review")
    (anchor
      :role tool
      :observed "mcp-tool:mission_llm_trace"
      :file "crates/missiond-mcp/src/tools/comm/question.rs"
      :symbol "mission_llm_trace")
    (anchor
      :role tool
      :observed "mcp-tool:mission_master_status"
      :file "crates/missiond-mcp/src/tools/compute/process.rs"
      :symbol "mission_master_status")
    (anchor
      :role tool
      :observed "mcp-tool:mission_memory"
      :file "crates/missiond-mcp/src/tools/knowledge/memory.rs"
      :symbol "mission_memory")
    (anchor
      :role tool
      :observed "mcp-tool:mission_minimax_process"
      :file "crates/missiond-mcp/src/tools/compute/minimax.rs"
      :symbol "mission_minimax_process")
    (anchor
      :role tool
      :observed "mcp-tool:mission_nightly_evolution"
      :file "crates/missiond-mcp/src/tools/compute/process.rs"
      :symbol "mission_nightly_evolution")
    (anchor
      :role tool
      :observed "mcp-tool:mission_pause"
      :file "crates/missiond-mcp/src/tools/compute/slot.rs"
      :symbol "mission_pause")
    (anchor
      :role tool
      :observed "mcp-tool:mission_permission_mutate"
      :file "crates/missiond-mcp/src/tools/sysinfra/permission.rs"
      :symbol "mission_permission_mutate")
    (anchor
      :role tool
      :observed "mcp-tool:mission_permission_query"
      :file "crates/missiond-mcp/src/tools/sysinfra/permission.rs"
      :symbol "mission_permission_query")
    (anchor
      :role tool
      :observed "mcp-tool:mission_plan"
      :file "crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :symbol "mission_plan")
    (anchor
      :role tool
      :observed "mcp-tool:mission_power_control"
      :file "crates/missiond-mcp/src/tools/sysinfra/power.rs"
      :symbol "mission_power_control")
    (anchor
      :role tool
      :observed "mcp-tool:mission_project"
      :file "crates/missiond-mcp/src/tools/knowledge/project.rs"
      :symbol "mission_project")
    (anchor
      :role tool
      :observed "mcp-tool:mission_pty_confirm"
      :file "crates/missiond-mcp/src/tools/compute/pty.rs"
      :symbol "mission_pty_confirm")
    (anchor
      :role tool
      :observed "mcp-tool:mission_pty_read"
      :file "crates/missiond-mcp/src/tools/compute/pty.rs"
      :symbol "mission_pty_read")
    (anchor
      :role tool
      :observed "mcp-tool:mission_pty_screenshot"
      :file "crates/missiond-mcp/src/tools/compute/pty.rs"
      :symbol "mission_pty_screenshot")
    (anchor
      :role tool
      :observed "mcp-tool:mission_pty_send"
      :file "crates/missiond-mcp/src/tools/compute/pty.rs"
      :symbol "mission_pty_send")
    (anchor
      :role tool
      :observed "mcp-tool:mission_pty_signal"
      :file "crates/missiond-mcp/src/tools/compute/pty.rs"
      :symbol "mission_pty_signal")
    (anchor
      :role tool
      :observed "mcp-tool:mission_pty_spawn"
      :file "crates/missiond-mcp/src/tools/compute/pty.rs"
      :symbol "mission_pty_spawn")
    (anchor
      :role tool
      :observed "mcp-tool:mission_pty_status"
      :file "crates/missiond-mcp/src/tools/compute/pty.rs"
      :symbol "mission_pty_status")
    (anchor
      :role tool
      :observed "mcp-tool:mission_question"
      :file "crates/missiond-mcp/src/tools/comm/question.rs"
      :symbol "mission_question")
    (anchor
      :role tool
      :observed "mcp-tool:mission_request"
      :file "crates/missiond-mcp/src/tools/knowledge/request.rs"
      :symbol "mission_request")
    (anchor
      :role tool
      :observed "mcp-tool:mission_retrospective_manage"
      :file "crates/missiond-mcp/src/tools/comm/conversation.rs"
      :symbol "mission_retrospective_manage")
    (anchor
      :role tool
      :observed "mcp-tool:mission_router_chat"
      :file "crates/missiond-mcp/src/tools/comm/router_chat.rs"
      :symbol "mission_router_chat")
    (anchor
      :role tool
      :observed "mcp-tool:mission_router_chat_manage"
      :file "crates/missiond-mcp/src/tools/comm/router_chat.rs"
      :symbol "mission_router_chat_manage")
    (anchor
      :role tool
      :observed "mcp-tool:mission_shared_memory"
      :file "crates/missiond-mcp/src/tools/knowledge/shared_memory.rs"
      :symbol "mission_shared_memory")
    (anchor
      :role tool
      :observed "mcp-tool:mission_skill_context"
      :file "crates/missiond-mcp/src/tools/knowledge/skill.rs"
      :symbol "mission_skill_context")
    (anchor
      :role tool
      :observed "mcp-tool:mission_skill_exec"
      :file "crates/missiond-mcp/src/tools/knowledge/skill.rs"
      :symbol "mission_skill_exec")
    (anchor
      :role tool
      :observed "mcp-tool:mission_skill_mutate"
      :file "crates/missiond-mcp/src/tools/knowledge/skill.rs"
      :symbol "mission_skill_mutate")
    (anchor
      :role tool
      :observed "mcp-tool:mission_skill_query"
      :file "crates/missiond-mcp/src/tools/knowledge/skill.rs"
      :symbol "mission_skill_query")
    (anchor
      :role tool
      :observed "mcp-tool:mission_slot_history"
      :file "crates/missiond-mcp/src/tools/compute/slot.rs"
      :symbol "mission_slot_history")
    (anchor
      :role tool
      :observed "mcp-tool:mission_slots"
      :file "crates/missiond-mcp/src/tools/compute/process.rs"
      :symbol "mission_slots")
    (anchor
      :role tool
      :observed "mcp-tool:mission_sonnet_process"
      :file "crates/missiond-mcp/src/tools/compute/minimax.rs"
      :symbol "mission_sonnet_process")
    (anchor
      :role tool
      :observed "mcp-tool:mission_submit_phase_result"
      :file "crates/missiond-mcp/src/tools/knowledge/board.rs"
      :symbol "mission_submit_phase_result")
    (anchor
      :role tool
      :observed "mcp-tool:mission_swarm_run"
      :file "crates/missiond-mcp/src/tools/compute/task_delegate.rs"
      :symbol "mission_swarm_run")
    (anchor
      :role tool
      :observed "mcp-tool:mission_sys_config"
      :file "crates/missiond-mcp/src/tools/sysinfra/system.rs"
      :symbol "mission_sys_config")
    (anchor
      :role tool
      :observed "mcp-tool:mission_sys_logs"
      :file "crates/missiond-mcp/src/tools/sysinfra/system.rs"
      :symbol "mission_sys_logs")
    (anchor
      :role tool
      :observed "mcp-tool:mission_task_cancel"
      :file "crates/missiond-mcp/src/tools/compute/task.rs"
      :symbol "mission_task_cancel")
    (anchor
      :role tool
      :observed "mcp-tool:mission_task_delegate"
      :file "crates/missiond-mcp/src/tools/compute/task_delegate.rs"
      :symbol "mission_task_delegate")
    (anchor
      :role tool
      :observed "mcp-tool:mission_task_query"
      :file "crates/missiond-mcp/src/tools/compute/task.rs"
      :symbol "mission_task_query")
    (anchor
      :role tool
      :observed "mcp-tool:mission_task_submit"
      :file "crates/missiond-mcp/src/tools/compute/task.rs"
      :symbol "mission_task_submit")
    (anchor
      :role tool
      :observed "mcp-tool:mission_timeline"
      :file "crates/missiond-mcp/src/tools/comm/timeline.rs"
      :symbol "mission_timeline")
    (anchor
      :role tool
      :observed "mcp-tool:mission_tool_directory"
      :file "crates/missiond-mcp/src/tools/comm/tool_directory.rs"
      :symbol "mission_tool_directory")
    (anchor
      :role tool
      :observed "mcp-tool:mission_universe_graph"
      :file "crates/missiond-mcp/src/tools/knowledge/cascade.rs"
      :symbol "mission_universe_graph")
    (anchor
      :role tool
      :observed "mcp-tool:mission_worker"
      :file "crates/missiond-mcp/src/tools/compute/worker.rs"
      :symbol "mission_worker")
    (anchor
      :role tool
      :observed "mcp-tool:mission_workflow"
      :file "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
      :symbol "mission_workflow"))

  (behavior
    :id missiond-navigation-route
    :kind route
    :owner navigation-gate
    :observed ["route:crates/missiond-core/src/event/subscription/api.rs:294"
              "route:crates/missiond-core/src/event/subscription/failure.rs:165"
              "route:crates/skill-store/src/routes/auth.rs:12"
              "route:crates/skill-store/src/routes/auth.rs:13"
              "route:crates/skill-store/src/routes/auth.rs:14"
              "route:crates/skill-store/src/routes/auth.rs:15"
              "route:crates/skill-store/src/routes/creator.rs:11"
              "route:crates/skill-store/src/routes/invoke.rs:11"
              "route:crates/skill-store/src/routes/mod.rs:10"
              "route:crates/skill-store/src/routes/skills.rs:13"
              "route:crates/skill-store/src/routes/skills.rs:14"
              "route:crates/skill-store/src/routes/skills.rs:15"
              "route:crates/skill-store/src/routes/skills.rs:16"
              "route:crates/skill-store/src/routes/skills.rs:20"
              "route:crates/skill-store/src/routes/subscriptions.rs:12"
              "route:crates/skill-store/src/routes/subscriptions.rs:13"
              "route:crates/skill-store/src/routes/subscriptions.rs:14"
              "route:crates/skill-store/src/routes/subscriptions.rs:15"
              "route:crates/skill-store/src/routes/subscriptions.rs:16"
              "route:packages/board/src/app/api/architecture/beacons/route.ts:7"
              "route:packages/board/src/app/api/architecture/route.ts:33"
              "route:packages/board/src/app/api/codex-replay/control/route.ts:8"
              "route:packages/board/src/app/api/codex-replay/run/route.ts:8"
              "route:packages/board/src/app/api/codex-replay/runs/route.ts:8"
              "route:packages/board/src/app/api/codex-replay/status/route.ts:8"
              "route:packages/board/src/app/api/conversation-image/route.ts:9"
              "route:packages/board/src/app/api/conversations/route.ts:4"
              "route:packages/board/src/app/api/conversations/route.ts:62"
              "route:packages/board/src/app/api/decisions/stats/route.ts:4"
              "route:packages/board/src/app/api/deploy/status/route.ts:28"
              "route:packages/board/src/app/api/images/route.ts:26"
              "route:packages/board/src/app/api/infra/route.ts:4"
              "route:packages/board/src/app/api/jarvis/conversations/route.ts:26"
              "route:packages/board/src/app/api/kb/route.ts:31"
              "route:packages/board/src/app/api/kb/route.ts:4"
              "route:packages/board/src/app/api/kb/route.ts:47"
              "route:packages/board/src/app/api/master/chat/route.ts:7"
              "route:packages/board/src/app/api/master/status/route.ts:8"
              "route:packages/board/src/app/api/memory/pause/route.ts:4"
              "route:packages/board/src/app/api/memory/status/route.ts:4"
              "route:packages/board/src/app/api/memory/task-stats/route.ts:4"
              "route:packages/board/src/app/api/memory/token-stats/route.ts:4"
              "route:packages/board/src/app/api/projects/route.ts:7"
              "route:packages/board/src/app/api/pty/agents/route.ts:4"
              "route:packages/board/src/app/api/pty/confirm/route.ts:4"
              "route:packages/board/src/app/api/pty/kill/route.ts:4"
              "route:packages/board/src/app/api/pty/screen/route.ts:4"
              "route:packages/board/src/app/api/pty/spawn/route.ts:4"
              "route:packages/board/src/app/api/pty/status/route.ts:20"
              "route:packages/board/src/app/api/questions/route.ts:20"
              "route:packages/board/src/app/api/questions/route.ts:4"
              "route:packages/board/src/app/api/slots/route.ts:143"
              "route:packages/board/src/app/api/system/conversation-message/route.ts:4"
              "route:packages/board/src/app/api/system/gemini-content/route.ts:4"
              "route:packages/board/src/app/api/system/health/route.ts:4"
              "route:packages/board/src/app/api/system/llm-traces/route.ts:4"
              "route:packages/board/src/app/api/system/message-image/route.ts:8"
              "route:packages/board/src/app/api/system/narrations/route.ts:4"
              "route:packages/board/src/app/api/system/tool-call/route.ts:4"
              "route:packages/board/src/app/api/tasks/route.ts:105"
              "route:packages/board/src/app/api/tasks/route.ts:25"
              "route:packages/board/src/app/api/tasks/route.ts:46"
              "route:packages/board/src/app/api/tasks/route.ts:92"
              "route:packages/board/src/app/api/timeline/events/route.ts:28"
              "route:packages/board/src/app/api/timeline/stats/route.ts:16"
              "route:packages/board/src/app/api/timeline/traces/route.ts:4"
              "route:packages/board/src/app/api/transcripts/route.ts:48"]
    :code ["crates/missiond-core/src/event/subscription/api.rs"
          "crates/missiond-core/src/event/subscription/failure.rs"
          "crates/skill-store/src/routes/auth.rs"
          "crates/skill-store/src/routes/creator.rs"
          "crates/skill-store/src/routes/invoke.rs"
          "crates/skill-store/src/routes/mod.rs"
          "crates/skill-store/src/routes/skills.rs"
          "crates/skill-store/src/routes/subscriptions.rs"
          "packages/board/src/app/api/architecture/beacons/route.ts"
          "packages/board/src/app/api/architecture/route.ts"
          "packages/board/src/app/api/codex-replay/control/route.ts"
          "packages/board/src/app/api/codex-replay/run/route.ts"
          "packages/board/src/app/api/codex-replay/runs/route.ts"
          "packages/board/src/app/api/codex-replay/status/route.ts"
          "packages/board/src/app/api/conversation-image/route.ts"
          "packages/board/src/app/api/conversations/route.ts"
          "packages/board/src/app/api/decisions/stats/route.ts"
          "packages/board/src/app/api/deploy/status/route.ts"
          "packages/board/src/app/api/images/route.ts"
          "packages/board/src/app/api/infra/route.ts"
          "packages/board/src/app/api/jarvis/conversations/route.ts"
          "packages/board/src/app/api/kb/route.ts"
          "packages/board/src/app/api/master/chat/route.ts"
          "packages/board/src/app/api/master/status/route.ts"
          "packages/board/src/app/api/memory/pause/route.ts"
          "packages/board/src/app/api/memory/status/route.ts"
          "packages/board/src/app/api/memory/task-stats/route.ts"
          "packages/board/src/app/api/memory/token-stats/route.ts"
          "packages/board/src/app/api/projects/route.ts"
          "packages/board/src/app/api/pty/agents/route.ts"
          "packages/board/src/app/api/pty/confirm/route.ts"
          "packages/board/src/app/api/pty/kill/route.ts"
          "packages/board/src/app/api/pty/screen/route.ts"
          "packages/board/src/app/api/pty/spawn/route.ts"
          "packages/board/src/app/api/pty/status/route.ts"
          "packages/board/src/app/api/questions/route.ts"
          "packages/board/src/app/api/slots/route.ts"
          "packages/board/src/app/api/system/conversation-message/route.ts"
          "packages/board/src/app/api/system/gemini-content/route.ts"
          "packages/board/src/app/api/system/health/route.ts"
          "packages/board/src/app/api/system/llm-traces/route.ts"
          "packages/board/src/app/api/system/message-image/route.ts"
          "packages/board/src/app/api/system/narrations/route.ts"
          "packages/board/src/app/api/system/tool-call/route.ts"
          "packages/board/src/app/api/tasks/route.ts"
          "packages/board/src/app/api/timeline/events/route.ts"
          "packages/board/src/app/api/timeline/stats/route.ts"
          "packages/board/src/app/api/timeline/traces/route.ts"
          "packages/board/src/app/api/transcripts/route.ts"]
    :effects []
    (anchor
      :role route
      :observed "route:crates/missiond-core/src/event/subscription/api.rs:294"
      :file "crates/missiond-core/src/event/subscription/api.rs"
      :symbol "handle_nack")
    (anchor
      :role route
      :observed "route:crates/missiond-core/src/event/subscription/failure.rs:165"
      :file "crates/missiond-core/src/event/subscription/failure.rs"
      :symbol "route")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/auth.rs:12"
      :file "crates/skill-store/src/routes/auth.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/auth.rs:13"
      :file "crates/skill-store/src/routes/auth.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/auth.rs:14"
      :file "crates/skill-store/src/routes/auth.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/auth.rs:15"
      :file "crates/skill-store/src/routes/auth.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/creator.rs:11"
      :file "crates/skill-store/src/routes/creator.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/invoke.rs:11"
      :file "crates/skill-store/src/routes/invoke.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/mod.rs:10"
      :file "crates/skill-store/src/routes/mod.rs"
      :symbol "build_router")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/skills.rs:13"
      :file "crates/skill-store/src/routes/skills.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/skills.rs:14"
      :file "crates/skill-store/src/routes/skills.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/skills.rs:15"
      :file "crates/skill-store/src/routes/skills.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/skills.rs:16"
      :file "crates/skill-store/src/routes/skills.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/skills.rs:20"
      :file "crates/skill-store/src/routes/skills.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/subscriptions.rs:12"
      :file "crates/skill-store/src/routes/subscriptions.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/subscriptions.rs:13"
      :file "crates/skill-store/src/routes/subscriptions.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/subscriptions.rs:14"
      :file "crates/skill-store/src/routes/subscriptions.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/subscriptions.rs:15"
      :file "crates/skill-store/src/routes/subscriptions.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:crates/skill-store/src/routes/subscriptions.rs:16"
      :file "crates/skill-store/src/routes/subscriptions.rs"
      :symbol "routes")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/architecture/beacons/route.ts:7"
      :file "packages/board/src/app/api/architecture/beacons/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/architecture/route.ts:33"
      :file "packages/board/src/app/api/architecture/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/codex-replay/control/route.ts:8"
      :file "packages/board/src/app/api/codex-replay/control/route.ts"
      :symbol "POST")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/codex-replay/run/route.ts:8"
      :file "packages/board/src/app/api/codex-replay/run/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/codex-replay/runs/route.ts:8"
      :file "packages/board/src/app/api/codex-replay/runs/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/codex-replay/status/route.ts:8"
      :file "packages/board/src/app/api/codex-replay/status/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/conversation-image/route.ts:9"
      :file "packages/board/src/app/api/conversation-image/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/conversations/route.ts:4"
      :file "packages/board/src/app/api/conversations/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/conversations/route.ts:62"
      :file "packages/board/src/app/api/conversations/route.ts"
      :symbol "POST")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/decisions/stats/route.ts:4"
      :file "packages/board/src/app/api/decisions/stats/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/deploy/status/route.ts:28"
      :file "packages/board/src/app/api/deploy/status/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/images/route.ts:26"
      :file "packages/board/src/app/api/images/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/infra/route.ts:4"
      :file "packages/board/src/app/api/infra/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/jarvis/conversations/route.ts:26"
      :file "packages/board/src/app/api/jarvis/conversations/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/kb/route.ts:31"
      :file "packages/board/src/app/api/kb/route.ts"
      :symbol "PATCH")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/kb/route.ts:4"
      :file "packages/board/src/app/api/kb/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/kb/route.ts:47"
      :file "packages/board/src/app/api/kb/route.ts"
      :symbol "DELETE")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/master/chat/route.ts:7"
      :file "packages/board/src/app/api/master/chat/route.ts"
      :symbol "POST")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/master/status/route.ts:8"
      :file "packages/board/src/app/api/master/status/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/memory/pause/route.ts:4"
      :file "packages/board/src/app/api/memory/pause/route.ts"
      :symbol "POST")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/memory/status/route.ts:4"
      :file "packages/board/src/app/api/memory/status/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/memory/task-stats/route.ts:4"
      :file "packages/board/src/app/api/memory/task-stats/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/memory/token-stats/route.ts:4"
      :file "packages/board/src/app/api/memory/token-stats/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/projects/route.ts:7"
      :file "packages/board/src/app/api/projects/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/pty/agents/route.ts:4"
      :file "packages/board/src/app/api/pty/agents/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/pty/confirm/route.ts:4"
      :file "packages/board/src/app/api/pty/confirm/route.ts"
      :symbol "POST")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/pty/kill/route.ts:4"
      :file "packages/board/src/app/api/pty/kill/route.ts"
      :symbol "POST")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/pty/screen/route.ts:4"
      :file "packages/board/src/app/api/pty/screen/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/pty/spawn/route.ts:4"
      :file "packages/board/src/app/api/pty/spawn/route.ts"
      :symbol "POST")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/pty/status/route.ts:20"
      :file "packages/board/src/app/api/pty/status/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/questions/route.ts:20"
      :file "packages/board/src/app/api/questions/route.ts"
      :symbol "POST")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/questions/route.ts:4"
      :file "packages/board/src/app/api/questions/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/slots/route.ts:143"
      :file "packages/board/src/app/api/slots/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/system/conversation-message/route.ts:4"
      :file "packages/board/src/app/api/system/conversation-message/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/system/gemini-content/route.ts:4"
      :file "packages/board/src/app/api/system/gemini-content/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/system/health/route.ts:4"
      :file "packages/board/src/app/api/system/health/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/system/llm-traces/route.ts:4"
      :file "packages/board/src/app/api/system/llm-traces/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/system/message-image/route.ts:8"
      :file "packages/board/src/app/api/system/message-image/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/system/narrations/route.ts:4"
      :file "packages/board/src/app/api/system/narrations/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/system/tool-call/route.ts:4"
      :file "packages/board/src/app/api/system/tool-call/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/tasks/route.ts:105"
      :file "packages/board/src/app/api/tasks/route.ts"
      :symbol "DELETE")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/tasks/route.ts:25"
      :file "packages/board/src/app/api/tasks/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/tasks/route.ts:46"
      :file "packages/board/src/app/api/tasks/route.ts"
      :symbol "POST")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/tasks/route.ts:92"
      :file "packages/board/src/app/api/tasks/route.ts"
      :symbol "PATCH")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/timeline/events/route.ts:28"
      :file "packages/board/src/app/api/timeline/events/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/timeline/stats/route.ts:16"
      :file "packages/board/src/app/api/timeline/stats/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/timeline/traces/route.ts:4"
      :file "packages/board/src/app/api/timeline/traces/route.ts"
      :symbol "GET")
    (anchor
      :role route
      :observed "route:packages/board/src/app/api/transcripts/route.ts:48"
      :file "packages/board/src/app/api/transcripts/route.ts"
      :symbol "GET"))

  (behavior
    :id missiond-navigation-scheduler
    :kind scheduler
    :owner navigation-gate
    :observed ["background-task:crates/missiond-core/src/cc_tasks/watcher.rs:207"
              "background-task:crates/missiond-core/src/cc_tasks/watcher.rs:259"
              "background-task:crates/missiond-core/src/cc_tasks/watcher.rs:288"
              "background-task:crates/missiond-core/src/event/in_memory/log.rs:116"
              "background-task:crates/missiond-core/src/event/in_memory/mod.rs:140"
              "background-task:crates/missiond-core/src/event/metrics/emitter.rs:88"
              "background-task:crates/missiond-core/src/event/pipeline/step3_commit/log_writer.rs:98"
              "background-task:crates/missiond-core/src/event/subscription/api.rs:162"
              "background-task:crates/missiond-core/src/gemini_cli/watcher.rs:145"
              "background-task:crates/missiond-core/src/gemini_cli/watcher.rs:66"
              "background-task:crates/missiond-core/src/sync/client.rs:107"
              "background-task:crates/missiond-core/src/sync/relay.rs:102"
              "background-task:crates/missiond-core/src/sync/relay.rs:111"
              "background-task:crates/missiond-core/src/ws/server.rs:2880"
              "background-task:crates/missiond-core/src/ws/server.rs:614"
              "background-task:crates/missiond-core/src/ws/server.rs:634"
              "background-task:crates/missiond-daemon/src/bus/bootstrap.rs:208"
              "background-task:crates/missiond-daemon/src/bus/bootstrap.rs:433"
              "background-task:crates/missiond-daemon/src/bus/retention_cron.rs:42"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:1154"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:136"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:188"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:219"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:256"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:301"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:356"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:558"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:585"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:617"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:640"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:660"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:688"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:706"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:725"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:735"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:756"
              "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:863"
              "background-task:crates/missiond-daemon/src/bus/ws_bridge.rs:63"
              "background-task:crates/missiond-daemon/src/engine/codex_replay.rs:1068"
              "background-task:crates/missiond-daemon/src/engine/codex_replay.rs:302"
              "background-task:crates/missiond-daemon/src/engine/codex_replay.rs:576"
              "background-task:crates/missiond-daemon/src/engine/codex_replay.rs:616"
              "background-task:crates/missiond-daemon/src/engine/commit_convergence.rs:133"
              "background-task:crates/missiond-daemon/src/engine/intent_engine/autopilot.rs:3102"
              "background-task:crates/missiond-daemon/src/engine/intent_engine/autopilot.rs:3697"
              "background-task:crates/missiond-daemon/src/engine/intent_engine/autopilot.rs:3818"
              "background-task:crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs:236"
              "background-task:crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs:326"
              "background-task:crates/missiond-daemon/src/engine/learning_engine/extraction.rs:42"
              "background-task:crates/missiond-daemon/src/engine/learning_engine/extraction.rs:67"
              "background-task:crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs:149"
              "background-task:crates/missiond-daemon/src/engine/learning_engine/mod.rs:63"
              "background-task:crates/missiond-daemon/src/engine/lisp_code_sync.rs:215"
              "background-task:crates/missiond-daemon/src/engine/lisp_code_sync.rs:216"
              "background-task:crates/missiond-daemon/src/engine/lisp_code_sync.rs:218"
              "background-task:crates/missiond-daemon/src/engine/master_control.rs:1057"
              "background-task:crates/missiond-daemon/src/engine/master_control.rs:1097"
              "background-task:crates/missiond-daemon/src/engine/master_control.rs:850"
              "background-task:crates/missiond-daemon/src/engine/master_control.rs:887"
              "background-task:crates/missiond-daemon/src/engine/master_control.rs:921"
              "background-task:crates/missiond-daemon/src/engine/master_control.rs:965"
              "background-task:crates/missiond-daemon/src/engine/nightly_evolution.rs:117"
              "background-task:crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs:370"
              "background-task:crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs:153"
              "background-task:crates/missiond-daemon/src/handlers/comm/retrospective.rs:1019"
              "background-task:crates/missiond-daemon/src/handlers/compute/compute_slot.rs:430"
              "background-task:crates/missiond-daemon/src/handlers/compute/task_delegate.rs:2588"
              "background-task:crates/missiond-daemon/src/handlers/knowledge/board/events.rs:12"
              "background-task:crates/missiond-daemon/src/handlers/knowledge/board/events.rs:26"
              "background-task:crates/missiond-daemon/src/handlers/knowledge/board/events.rs:44"
              "background-task:crates/missiond-daemon/src/handlers/knowledge/board/update.rs:134"
              "background-task:crates/missiond-daemon/src/handlers/knowledge/board/update.rs:207"
              "background-task:crates/missiond-daemon/src/handlers/knowledge/board/update.rs:81"
              "background-task:crates/missiond-daemon/src/handlers/sysinfra/system.rs:500"
              "background-task:crates/missiond-daemon/src/infra/mcp_client.rs:162"
              "background-task:crates/missiond-daemon/src/infra/mcp_client.rs:203"
              "background-task:crates/missiond-daemon/src/llm/codex_cli.rs:153"
              "background-task:crates/missiond-daemon/src/llm/gemini_cli.rs:551"
              "background-task:crates/missiond-daemon/src/llm/minimax_gateway.rs:369"
              "background-task:crates/missiond-daemon/src/llm/sonnet_gateway.rs:383"
              "background-task:crates/missiond-daemon/src/slot_orchestrator/spawner.rs:190"
              "background-task:crates/missiond-daemon/src/supervisor.rs:20"
              "background-task:crates/missiond-daemon/src/supervisor.rs:359"
              "background-task:crates/missiond-daemon/src/workers/local/pty_event_worker.rs:700"
              "background-task:crates/missiond-daemon/src/workers/local/pty_event_worker.rs:774"
              "background-task:crates/missiond-daemon/src/workers/mod.rs:87"
              "background-task:crates/missiond-pty/src/manager.rs:385"
              "background-task:crates/missiond-pty/src/manager.rs:567"
              "background-task:crates/missiond-pty/src/manager.rs:594"
              "background-task:crates/missiond-pty/src/session.rs:878"
              "background-task:crates/missiond-pty/src/session.rs:887"
              "background-task:crates/missiond-pty/src/session.rs:920"
              "background-task:crates/missiond-pty/src/session.rs:950"
              "background-task:crates/missiond-runner/src/runner.rs:114"
              "background-task:crates/skill-store/src/main.rs:76"
              "background-task:crates/skill-store/src/services/executor.rs:71"
              "scheduler:crates/missiond-core/src/cc_tasks/watcher.rs:1076"
              "scheduler:crates/missiond-core/src/cc_tasks/watcher.rs:261"
              "scheduler:crates/missiond-core/src/event/metrics/emitter.rs:89"
              "scheduler:crates/missiond-core/src/gemini_cli/watcher.rs:351"
              "scheduler:crates/missiond-core/src/sync/client.rs:9"
              "scheduler:crates/missiond-core/src/ws/server.rs:3717"
              "scheduler:crates/missiond-core/src/ws/server.rs:3906"
              "scheduler:crates/missiond-daemon/src/engine/lisp_code_sync.rs:291"
              "scheduler:crates/missiond-daemon/src/engine/master_control.rs:1104"
              "scheduler:crates/missiond-daemon/src/engine/nightly_evolution.rs:143"
              "scheduler:crates/missiond-daemon/src/workers/local/conversation_organizer.rs:58"
              "scheduler:crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs:56"
              "scheduler:crates/missiond-daemon/src/workers/local/reconcile_worker.rs:51"
              "scheduler:crates/missiond-daemon/src/workers/local/tagger_chunker.rs:109"
              "scheduler:crates/missiond-daemon/src/workers/local/tagger_chunker.rs:111"
              "scheduler:crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs:27"
              "scheduler:crates/missiond-pty/src/manager.rs:596"
              "scheduler:crates/missiond-pty/src/session.rs:952"
              "scheduler:crates/skill-store/src/main.rs:77"
              "scheduler:packages/board/src/App.tsx:186"
              "scheduler:packages/board/src/components/AutopilotMonitor.tsx:30"
              "scheduler:packages/board/src/components/CodexReplayDashboard.tsx:164"
              "scheduler:packages/board/src/components/DecisionDashboard.tsx:514"
              "scheduler:packages/board/src/components/DecisionDashboard.tsx:520"
              "scheduler:packages/board/src/components/DeployDashboard.tsx:241"
              "scheduler:packages/board/src/components/EngineDashboard.tsx:332"
              "scheduler:packages/board/src/components/JarvisChat.tsx:371"
              "scheduler:packages/board/src/components/JarvisChat.tsx:635"
              "scheduler:packages/board/src/components/MemoryDashboard.tsx:403"
              "scheduler:packages/board/src/components/PendingQuestions.tsx:24"
              "scheduler:packages/board/src/components/Terminal.tsx:245"
              "scheduler:packages/board/src/components/Terminal.tsx:385"
              "scheduler:packages/board/src/components/Terminal.tsx:396"
              "scheduler:packages/board/src/components/Terminal.tsx:75"
              "scheduler:packages/board/src/components/architecture/DiffPanel.tsx:20"
              "scheduler:packages/board/src/eventStream.ts:181"
              "scheduler:packages/board/src/eventStream.ts:52"
              "scheduler:packages/board/src/eventStream.ts:60"
              "scheduler:packages/board/src/hooks/useTimelineGestures.ts:198"
              "scheduler:packages/board/src/hooks/useTimelineGestures.ts:292"
              "scheduler:packages/board/src/hooks/useTimelineGestures.ts:58"
              "scheduler:packages/board/src/hooks/useTimelineGestures.ts:65"
              "scheduler:packages/board/src/lib/missiond.ts:42"
              "scheduler:packages/node-client/src/client.ts:565"
              "scheduler:packages/node-client/src/daemon.ts:124"
              "scheduler:packages/node-client/src/daemon.ts:260"
              "scheduler:packages/node-client/src/daemon.ts:300"
              "scheduler:packages/node-client/src/pty.ts:242"
              "scheduler:packages/node-client/src/pty.ts:489"
              "scheduler:scripts/context-pack-append.mjs:238"
              "scheduler:scripts/dispatch-memory-review-direct-wave.mjs:122"
              "scheduler:scripts/dispatch-memory-review-direct-wave.mjs:61"
              "scheduler:scripts/dispatch-memory-review-wave.mjs:125"
              "scheduler:scripts/dispatch-memory-review-wave.mjs:55"
              "scheduler:scripts/mission-mcp-call.mjs:57"
              "scheduler:scripts/report-claude-role-attribution.mjs:170"
              "scheduler:scripts/report-codex-conversation-duplicates.mjs:144"
              "scheduler:scripts/task-runner-submit-dispatch.mjs:256"]
    :code ["crates/missiond-core/src/cc_tasks/watcher.rs"
          "crates/missiond-core/src/event/in_memory/log.rs"
          "crates/missiond-core/src/event/in_memory/mod.rs"
          "crates/missiond-core/src/event/metrics/emitter.rs"
          "crates/missiond-core/src/event/pipeline/step3_commit/log_writer.rs"
          "crates/missiond-core/src/event/subscription/api.rs"
          "crates/missiond-core/src/gemini_cli/watcher.rs"
          "crates/missiond-core/src/sync/client.rs"
          "crates/missiond-core/src/sync/relay.rs"
          "crates/missiond-core/src/ws/server.rs"
          "crates/missiond-daemon/src/bus/bootstrap.rs"
          "crates/missiond-daemon/src/bus/retention_cron.rs"
          "crates/missiond-daemon/src/bus/v2_subscribers.rs"
          "crates/missiond-daemon/src/bus/ws_bridge.rs"
          "crates/missiond-daemon/src/engine/codex_replay.rs"
          "crates/missiond-daemon/src/engine/commit_convergence.rs"
          "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
          "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
          "crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs"
          "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
          "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"
          "crates/missiond-daemon/src/engine/learning_engine/mod.rs"
          "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
          "crates/missiond-daemon/src/engine/master_control.rs"
          "crates/missiond-daemon/src/engine/nightly_evolution.rs"
          "crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs"
          "crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs"
          "crates/missiond-daemon/src/handlers/comm/retrospective.rs"
          "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
          "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
          "crates/missiond-daemon/src/handlers/knowledge/board/events.rs"
          "crates/missiond-daemon/src/handlers/knowledge/board/update.rs"
          "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
          "crates/missiond-daemon/src/infra/mcp_client.rs"
          "crates/missiond-daemon/src/llm/codex_cli.rs"
          "crates/missiond-daemon/src/llm/gemini_cli.rs"
          "crates/missiond-daemon/src/llm/minimax_gateway.rs"
          "crates/missiond-daemon/src/llm/sonnet_gateway.rs"
          "crates/missiond-daemon/src/slot_orchestrator/spawner.rs"
          "crates/missiond-daemon/src/supervisor.rs"
          "crates/missiond-daemon/src/workers/local/conversation_organizer.rs"
          "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
          "crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
          "crates/missiond-daemon/src/workers/local/reconcile_worker.rs"
          "crates/missiond-daemon/src/workers/local/tagger_chunker.rs"
          "crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs"
          "crates/missiond-daemon/src/workers/mod.rs"
          "crates/missiond-pty/src/manager.rs"
          "crates/missiond-pty/src/session.rs"
          "crates/missiond-runner/src/runner.rs"
          "crates/skill-store/src/main.rs"
          "crates/skill-store/src/services/executor.rs"
          "packages/board/src/App.tsx"
          "packages/board/src/components/AutopilotMonitor.tsx"
          "packages/board/src/components/CodexReplayDashboard.tsx"
          "packages/board/src/components/DecisionDashboard.tsx"
          "packages/board/src/components/DeployDashboard.tsx"
          "packages/board/src/components/EngineDashboard.tsx"
          "packages/board/src/components/JarvisChat.tsx"
          "packages/board/src/components/MemoryDashboard.tsx"
          "packages/board/src/components/PendingQuestions.tsx"
          "packages/board/src/components/Terminal.tsx"
          "packages/board/src/components/architecture/DiffPanel.tsx"
          "packages/board/src/eventStream.ts"
          "packages/board/src/hooks/useTimelineGestures.ts"
          "packages/board/src/lib/missiond.ts"
          "packages/node-client/src/client.ts"
          "packages/node-client/src/daemon.ts"
          "packages/node-client/src/pty.ts"
          "scripts/context-pack-append.mjs"
          "scripts/dispatch-memory-review-direct-wave.mjs"
          "scripts/dispatch-memory-review-wave.mjs"
          "scripts/mission-mcp-call.mjs"
          "scripts/report-claude-role-attribution.mjs"
          "scripts/report-codex-conversation-duplicates.mjs"
          "scripts/task-runner-submit-dispatch.mjs"]
    :effects []
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/cc_tasks/watcher.rs:207"
      :file "crates/missiond-core/src/cc_tasks/watcher.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/cc_tasks/watcher.rs:259"
      :file "crates/missiond-core/src/cc_tasks/watcher.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/cc_tasks/watcher.rs:288"
      :file "crates/missiond-core/src/cc_tasks/watcher.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/event/in_memory/log.rs:116"
      :file "crates/missiond-core/src/event/in_memory/log.rs"
      :symbol "spawn")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/event/in_memory/mod.rs:140"
      :file "crates/missiond-core/src/event/in_memory/mod.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/event/metrics/emitter.rs:88"
      :file "crates/missiond-core/src/event/metrics/emitter.rs"
      :symbol "spawn_bus_metrics_emitter")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/event/pipeline/step3_commit/log_writer.rs:98"
      :file "crates/missiond-core/src/event/pipeline/step3_commit/log_writer.rs"
      :symbol "spawn_log_writer")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/event/subscription/api.rs:162"
      :file "crates/missiond-core/src/event/subscription/api.rs"
      :symbol "spawn_flusher")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/gemini_cli/watcher.rs:145"
      :file "crates/missiond-core/src/gemini_cli/watcher.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/gemini_cli/watcher.rs:66"
      :file "crates/missiond-core/src/gemini_cli/watcher.rs"
      :symbol "new")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/sync/client.rs:107"
      :file "crates/missiond-core/src/sync/client.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/sync/relay.rs:102"
      :file "crates/missiond-core/src/sync/relay.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/sync/relay.rs:111"
      :file "crates/missiond-core/src/sync/relay.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/ws/server.rs:2880"
      :file "crates/missiond-core/src/ws/server.rs"
      :symbol "handle_chat_completions")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/ws/server.rs:614"
      :file "crates/missiond-core/src/ws/server.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/ws/server.rs:634"
      :file "crates/missiond-core/src/ws/server.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/bootstrap.rs:208"
      :file "crates/missiond-daemon/src/bus/bootstrap.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/bootstrap.rs:433"
      :file "crates/missiond-daemon/src/bus/bootstrap.rs"
      :symbol "spawn_shutdown_fuse")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/retention_cron.rs:42"
      :file "crates/missiond-daemon/src/bus/retention_cron.rs"
      :symbol "spawn_retention_cron")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:1154"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_event_ref_cache_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:136"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_incident_reactor")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:188"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_extraction_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:219"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_submit_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:256"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_autopilot_board_event_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:301"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_autopilot_slot_event_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:356"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_deployment_event_response_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:558"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_decision_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:585"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_harvest_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:617"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_realtime_extraction_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:640"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_realtime_extraction_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:660"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_session_reflection_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:688"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_session_reflection_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:706"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_kb_consolidation_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:725"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_kb_consolidation_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:735"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_kb_consolidation_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:756"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_intent_analyst_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/v2_subscribers.rs:863"
      :file "crates/missiond-daemon/src/bus/v2_subscribers.rs"
      :symbol "spawn_review_resolution_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/bus/ws_bridge.rs:63"
      :file "crates/missiond-daemon/src/bus/ws_bridge.rs"
      :symbol "spawn_ws_bridge")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/codex_replay.rs:1068"
      :file "crates/missiond-daemon/src/engine/codex_replay.rs"
      :symbol "connect")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/codex_replay.rs:302"
      :file "crates/missiond-daemon/src/engine/codex_replay.rs"
      :symbol "spawn_campaign")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/codex_replay.rs:576"
      :file "crates/missiond-daemon/src/engine/codex_replay.rs"
      :symbol "wait_turn_with_timeout")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/codex_replay.rs:616"
      :file "crates/missiond-daemon/src/engine/codex_replay.rs"
      :symbol "codex_io_sink")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/commit_convergence.rs:133"
      :file "crates/missiond-daemon/src/engine/commit_convergence.rs"
      :symbol "start_commit_convergence_service")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/intent_engine/autopilot.rs:3102"
      :file "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
      :symbol "dispatch_board_tasks_with_config")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/intent_engine/autopilot.rs:3697"
      :file "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
      :symbol "dispatch_board_tasks_with_config")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/intent_engine/autopilot.rs:3818"
      :file "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
      :symbol "dispatch_board_tasks_with_config")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs:236"
      :file "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
      :symbol "drop")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs:326"
      :file "crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs"
      :symbol "dispatch_queued_submit_tasks")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/learning_engine/extraction.rs:42"
      :file "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
      :symbol "set_extraction_phase")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/learning_engine/extraction.rs:67"
      :file "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
      :symbol "emit_dispatch_event")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs:149"
      :file "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"
      :symbol "check_historical_scan")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/learning_engine/mod.rs:63"
      :file "crates/missiond-daemon/src/engine/learning_engine/mod.rs"
      :symbol "learning_tick")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/lisp_code_sync.rs:215"
      :file "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
      :symbol "start_lisp_code_sync_service")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/lisp_code_sync.rs:216"
      :file "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
      :symbol "start_lisp_code_sync_service")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/lisp_code_sync.rs:218"
      :file "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
      :symbol "start_lisp_code_sync_service")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/master_control.rs:1057"
      :file "crates/missiond-daemon/src/engine/master_control.rs"
      :symbol "spawn_question_event_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/master_control.rs:1097"
      :file "crates/missiond-daemon/src/engine/master_control.rs"
      :symbol "spawn_master_decision_loop")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/master_control.rs:850"
      :file "crates/missiond-daemon/src/engine/master_control.rs"
      :symbol "notify_board_event_direct")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/master_control.rs:887"
      :file "crates/missiond-daemon/src/engine/master_control.rs"
      :symbol "spawn_board_event_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/master_control.rs:921"
      :file "crates/missiond-daemon/src/engine/master_control.rs"
      :symbol "spawn_slot_event_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/master_control.rs:965"
      :file "crates/missiond-daemon/src/engine/master_control.rs"
      :symbol "spawn_incident_event_sub")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/engine/nightly_evolution.rs:117"
      :file "crates/missiond-daemon/src/engine/nightly_evolution.rs"
      :symbol "start_nightly_evolution_service")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs:370"
      :file "crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs"
      :symbol "handle_maintenance")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs:153"
      :file "crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs"
      :symbol "gemini_watch")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/comm/retrospective.rs:1019"
      :file "crates/missiond-daemon/src/handlers/comm/retrospective.rs"
      :symbol "handle_backfill")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/compute/compute_slot.rs:430"
      :file "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
      :symbol "create_slot")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/compute/task_delegate.rs:2588"
      :file "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
      :symbol "spawn_mechanic_repair")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/knowledge/board/events.rs:12"
      :file "crates/missiond-daemon/src/handlers/knowledge/board/events.rs"
      :symbol "publish_board_created")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/knowledge/board/events.rs:26"
      :file "crates/missiond-daemon/src/handlers/knowledge/board/events.rs"
      :symbol "publish_board_update")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/knowledge/board/events.rs:44"
      :file "crates/missiond-daemon/src/handlers/knowledge/board/events.rs"
      :symbol "publish_board_status_changed")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/knowledge/board/update.rs:134"
      :file "crates/missiond-daemon/src/handlers/knowledge/board/update.rs"
      :symbol "handle_toggle")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/knowledge/board/update.rs:207"
      :file "crates/missiond-daemon/src/handlers/knowledge/board/update.rs"
      :symbol "handle_single_update")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/knowledge/board/update.rs:81"
      :file "crates/missiond-daemon/src/handlers/knowledge/board/update.rs"
      :symbol "handle_batch_update")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/sysinfra/system.rs:500"
      :file "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
      :symbol "daemon_update")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/infra/mcp_client.rs:162"
      :file "crates/missiond-daemon/src/infra/mcp_client.rs"
      :symbol "spawn")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/infra/mcp_client.rs:203"
      :file "crates/missiond-daemon/src/infra/mcp_client.rs"
      :symbol "spawn")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/llm/codex_cli.rs:153"
      :file "crates/missiond-daemon/src/llm/codex_cli.rs"
      :symbol "publish")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/llm/gemini_cli.rs:551"
      :file "crates/missiond-daemon/src/llm/gemini_cli.rs"
      :symbol "stream_events")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/llm/minimax_gateway.rs:369"
      :file "crates/missiond-daemon/src/llm/minimax_gateway.rs"
      :symbol "run")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/llm/sonnet_gateway.rs:383"
      :file "crates/missiond-daemon/src/llm/sonnet_gateway.rs"
      :symbol "run")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/slot_orchestrator/spawner.rs:190"
      :file "crates/missiond-daemon/src/slot_orchestrator/spawner.rs"
      :symbol "spawn_tracked_slot")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/supervisor.rs:20"
      :file "crates/missiond-daemon/src/supervisor.rs"
      :symbol "check_slot_context_levels")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/supervisor.rs:359"
      :file "crates/missiond-daemon/src/supervisor.rs"
      :symbol "schedule_supervisor_patrol")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/workers/local/pty_event_worker.rs:700"
      :file "crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
      :symbol "handle_confirm_required")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/workers/local/pty_event_worker.rs:774"
      :file "crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
      :symbol "handle_mcp_tool_error")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/workers/mod.rs:87"
      :file "crates/missiond-daemon/src/workers/mod.rs"
      :symbol "spawn_worker")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-pty/src/manager.rs:385"
      :file "crates/missiond-pty/src/manager.rs"
      :symbol "spawn")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-pty/src/manager.rs:567"
      :file "crates/missiond-pty/src/manager.rs"
      :symbol "spawn")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-pty/src/manager.rs:594"
      :file "crates/missiond-pty/src/manager.rs"
      :symbol "spawn")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-pty/src/session.rs:878"
      :file "crates/missiond-pty/src/session.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-pty/src/session.rs:887"
      :file "crates/missiond-pty/src/session.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-pty/src/session.rs:920"
      :file "crates/missiond-pty/src/session.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-pty/src/session.rs:950"
      :file "crates/missiond-pty/src/session.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-runner/src/runner.rs:114"
      :file "crates/missiond-runner/src/runner.rs"
      :symbol "run")
    (anchor
      :role scheduler
      :observed "background-task:crates/skill-store/src/main.rs:76"
      :file "crates/skill-store/src/main.rs"
      :symbol "main")
    (anchor
      :role scheduler
      :observed "background-task:crates/skill-store/src/services/executor.rs:71"
      :file "crates/skill-store/src/services/executor.rs"
      :symbol "invoke")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-core/src/cc_tasks/watcher.rs:1076"
      :file "crates/missiond-core/src/cc_tasks/watcher.rs"
      :symbol "cursor_persist_loop")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-core/src/cc_tasks/watcher.rs:261"
      :file "crates/missiond-core/src/cc_tasks/watcher.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-core/src/event/metrics/emitter.rs:89"
      :file "crates/missiond-core/src/event/metrics/emitter.rs"
      :symbol "spawn_bus_metrics_emitter")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-core/src/gemini_cli/watcher.rs:351"
      :file "crates/missiond-core/src/gemini_cli/watcher.rs"
      :symbol "gemini_cursor_persist_loop")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-core/src/sync/client.rs:9"
      :file "crates/missiond-core/src/sync/client.rs")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-core/src/ws/server.rs:3717"
      :file "crates/missiond-core/src/ws/server.rs"
      :symbol "handle_pty_subscription")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-core/src/ws/server.rs:3906"
      :file "crates/missiond-core/src/ws/server.rs"
      :symbol "handle_events_subscription")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-daemon/src/engine/lisp_code_sync.rs:291"
      :file "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
      :symbol "run_reconciler")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-daemon/src/engine/master_control.rs:1104"
      :file "crates/missiond-daemon/src/engine/master_control.rs"
      :symbol "spawn_master_decision_loop")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-daemon/src/engine/nightly_evolution.rs:143"
      :file "crates/missiond-daemon/src/engine/nightly_evolution.rs"
      :symbol "run_schedule_loop")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-daemon/src/workers/local/conversation_organizer.rs:58"
      :file "crates/missiond-daemon/src/workers/local/conversation_organizer.rs"
      :symbol "run_organizer")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs:56"
      :file "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
      :symbol "run")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-daemon/src/workers/local/reconcile_worker.rs:51"
      :file "crates/missiond-daemon/src/workers/local/reconcile_worker.rs"
      :symbol "run")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-daemon/src/workers/local/tagger_chunker.rs:109"
      :file "crates/missiond-daemon/src/workers/local/tagger_chunker.rs"
      :symbol "run_tagger_chunker")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-daemon/src/workers/local/tagger_chunker.rs:111"
      :file "crates/missiond-daemon/src/workers/local/tagger_chunker.rs"
      :symbol "run_tagger_chunker")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs:27"
      :file "crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs"
      :symbol "run")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-pty/src/manager.rs:596"
      :file "crates/missiond-pty/src/manager.rs"
      :symbol "spawn")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-pty/src/session.rs:952"
      :file "crates/missiond-pty/src/session.rs"
      :symbol "start")
    (anchor
      :role scheduler
      :observed "scheduler:crates/skill-store/src/main.rs:77"
      :file "crates/skill-store/src/main.rs"
      :symbol "main")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/App.tsx:186"
      :file "packages/board/src/App.tsx"
      :symbol "activeSlotTask")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/architecture/DiffPanel.tsx:20"
      :file "packages/board/src/components/architecture/DiffPanel.tsx"
      :symbol "handleCopy")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/AutopilotMonitor.tsx:30"
      :file "packages/board/src/components/AutopilotMonitor.tsx"
      :symbol "fetchScreen")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/CodexReplayDashboard.tsx:164"
      :file "packages/board/src/components/CodexReplayDashboard.tsx"
      :symbol "CodexReplayDashboard")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/DecisionDashboard.tsx:514"
      :file "packages/board/src/components/DecisionDashboard.tsx"
      :symbol "DecisionDashboard")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/DecisionDashboard.tsx:520"
      :file "packages/board/src/components/DecisionDashboard.tsx"
      :symbol "DecisionDashboard")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/DeployDashboard.tsx:241"
      :file "packages/board/src/components/DeployDashboard.tsx"
      :symbol "DeployDashboard")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/EngineDashboard.tsx:332"
      :file "packages/board/src/components/EngineDashboard.tsx"
      :symbol "EngineDashboard")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/JarvisChat.tsx:371"
      :file "packages/board/src/components/JarvisChat.tsx"
      :symbol "poll")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/JarvisChat.tsx:635"
      :file "packages/board/src/components/JarvisChat.tsx"
      :symbol "poll")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/MemoryDashboard.tsx:403"
      :file "packages/board/src/components/MemoryDashboard.tsx"
      :symbol "MemoryDashboard")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/PendingQuestions.tsx:24"
      :file "packages/board/src/components/PendingQuestions.tsx"
      :symbol "PendingQuestions")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/Terminal.tsx:245"
      :file "packages/board/src/components/Terminal.tsx"
      :symbol "scheduleReconnect")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/Terminal.tsx:385"
      :file "packages/board/src/components/Terminal.tsx"
      :symbol "connectWs")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/Terminal.tsx:396"
      :file "packages/board/src/components/Terminal.tsx"
      :symbol "connectWs")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/components/Terminal.tsx:75"
      :file "packages/board/src/components/Terminal.tsx"
      :symbol "TerminalInner")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/eventStream.ts:181"
      :file "packages/board/src/eventStream.ts"
      :symbol "route")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/eventStream.ts:52"
      :file "packages/board/src/eventStream.ts")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/eventStream.ts:60"
      :file "packages/board/src/eventStream.ts"
      :symbol "debouncedBump")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/hooks/useTimelineGestures.ts:198"
      :file "packages/board/src/hooks/useTimelineGestures.ts"
      :symbol "center")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/hooks/useTimelineGestures.ts:292"
      :file "packages/board/src/hooks/useTimelineGestures.ts"
      :symbol "handleWheel")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/hooks/useTimelineGestures.ts:58"
      :file "packages/board/src/hooks/useTimelineGestures.ts"
      :symbol "useTimelineGestures")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/hooks/useTimelineGestures.ts:65"
      :file "packages/board/src/hooks/useTimelineGestures.ts"
      :symbol "useTimelineGestures")
    (anchor
      :role scheduler
      :observed "scheduler:packages/board/src/lib/missiond.ts:42"
      :file "packages/board/src/lib/missiond.ts"
      :symbol "callMissiond")
    (anchor
      :role scheduler
      :observed "scheduler:packages/node-client/src/client.ts:565"
      :file "packages/node-client/src/client.ts"
      :symbol "getDefaultWsUrl")
    (anchor
      :role scheduler
      :observed "scheduler:packages/node-client/src/daemon.ts:124"
      :file "packages/node-client/src/daemon.ts"
      :symbol "cleanup")
    (anchor
      :role scheduler
      :observed "scheduler:packages/node-client/src/daemon.ts:260"
      :file "packages/node-client/src/daemon.ts"
      :symbol "cleanup")
    (anchor
      :role scheduler
      :observed "scheduler:packages/node-client/src/daemon.ts:300"
      :file "packages/node-client/src/daemon.ts"
      :symbol "sleep")
    (anchor
      :role scheduler
      :observed "scheduler:packages/node-client/src/pty.ts:242"
      :file "packages/node-client/src/pty.ts")
    (anchor
      :role scheduler
      :observed "scheduler:packages/node-client/src/pty.ts:489"
      :file "packages/node-client/src/pty.ts"
      :symbol "connectPTY")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/context-pack-append.mjs:238"
      :file "scripts/context-pack-append.mjs"
      :symbol "withLock")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/dispatch-memory-review-direct-wave.mjs:122"
      :file "scripts/dispatch-memory-review-direct-wave.mjs"
      :symbol "sleep")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/dispatch-memory-review-direct-wave.mjs:61"
      :file "scripts/dispatch-memory-review-direct-wave.mjs"
      :symbol "callTool")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/dispatch-memory-review-wave.mjs:125"
      :file "scripts/dispatch-memory-review-wave.mjs"
      :symbol "sleep")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/dispatch-memory-review-wave.mjs:55"
      :file "scripts/dispatch-memory-review-wave.mjs"
      :symbol "callTool")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/mission-mcp-call.mjs:57"
      :file "scripts/mission-mcp-call.mjs")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/report-claude-role-attribution.mjs:170"
      :file "scripts/report-claude-role-attribution.mjs"
      :symbol "callMissiond")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/report-codex-conversation-duplicates.mjs:144"
      :file "scripts/report-codex-conversation-duplicates.mjs"
      :symbol "callMissiond")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/task-runner-submit-dispatch.mjs:256"
      :file "scripts/task-runner-submit-dispatch.mjs"
      :symbol "callToolViaIpc"))

  (behavior
    :id missiond-navigation-subprocess
    :kind subprocess
    :owner navigation-gate
    :observed ["subprocess:crates/missiond-daemon/src/context/slot_env.rs:360"
              "subprocess:crates/missiond-daemon/src/engine/codex_replay.rs:1049"
              "subprocess:crates/missiond-daemon/src/engine/commit_convergence.rs:499"
              "subprocess:crates/missiond-daemon/src/engine/commit_convergence.rs:508"
              "subprocess:crates/missiond-daemon/src/engine/lisp_code_sync.rs:980"
              "subprocess:crates/missiond-daemon/src/engine/master_control.rs:1302"
              "subprocess:crates/missiond-daemon/src/engine/master_control.rs:1787"
              "subprocess:crates/missiond-daemon/src/engine/nightly_evolution.rs:334"
              "subprocess:crates/missiond-daemon/src/engine/nightly_evolution.rs:380"
              "subprocess:crates/missiond-daemon/src/engine/nightly_evolution.rs:407"
              "subprocess:crates/missiond-daemon/src/handlers/comm/conversation/events.rs:214"
              "subprocess:crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs:216"
              "subprocess:crates/missiond-daemon/src/handlers/compute/forge.rs:109"
              "subprocess:crates/missiond-daemon/src/handlers/compute/forge.rs:46"
              "subprocess:crates/missiond-daemon/src/handlers/compute/slot.rs:69"
              "subprocess:crates/missiond-daemon/src/handlers/compute/task_delegate.rs:2668"
              "subprocess:crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_porcelain.rs:77"
              "subprocess:crates/missiond-daemon/src/handlers/knowledge/kb/discovery.rs:88"
              "subprocess:crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs:258"
              "subprocess:crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs:265"
              "subprocess:crates/missiond-daemon/src/handlers/knowledge/project/registry.rs:583"
              "subprocess:crates/missiond-daemon/src/handlers/knowledge/project/survey.rs:33"
              "subprocess:crates/missiond-daemon/src/handlers/sysinfra/infra.rs:285"
              "subprocess:crates/missiond-daemon/src/handlers/sysinfra/infra.rs:314"
              "subprocess:crates/missiond-daemon/src/handlers/sysinfra/infra.rs:344"
              "subprocess:crates/missiond-daemon/src/handlers/sysinfra/infra.rs:701"
              "subprocess:crates/missiond-daemon/src/handlers/sysinfra/system.rs:373"
              "subprocess:crates/missiond-daemon/src/handlers/sysinfra/system.rs:461"
              "subprocess:crates/missiond-daemon/src/handlers/sysinfra/system.rs:487"
              "subprocess:crates/missiond-daemon/src/handlers/sysinfra/system.rs:503"
              "subprocess:crates/missiond-daemon/src/handlers/sysinfra/system.rs:547"
              "subprocess:crates/missiond-daemon/src/infra/mcp_client.rs:147"
              "subprocess:crates/missiond-daemon/src/llm/codex_cli.rs:268"
              "subprocess:crates/missiond-daemon/src/llm/gemini_cli.rs:497"
              "subprocess:crates/missiond-daemon/src/llm/minimax_client.rs:140"
              "subprocess:crates/missiond-daemon/src/workers/local/ast_sync_worker.rs:391"
              "subprocess:crates/missiond-daemon/src/workers/local/ast_sync_worker.rs:428"
              "subprocess:crates/missiond-daemon/src/workers/local/ast_sync_worker.rs:445"
              "subprocess:crates/missiond-daemon/src/workers/local/ast_sync_worker.rs:463"
              "subprocess:crates/missiond-daemon/src/workers/local/ast_sync_worker.rs:497"
              "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:179"
              "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:222"
              "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:249"
              "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:284"
              "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:298"
              "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:311"
              "subprocess:crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs:245"
              "subprocess:crates/missiond-mcp/src/bin/mission-mcp.rs:227"
              "subprocess:crates/missiond-pty/src/manager.rs:261"
              "subprocess:crates/missiond-runner/src/runner.rs:73"
              "subprocess:packages/node-client/src/binary.ts:13"
              "subprocess:packages/node-client/src/client.ts:182"
              "subprocess:packages/node-client/src/client.ts:700"
              "subprocess:packages/node-client/src/daemon.ts:10"
              "subprocess:packages/node-client/src/daemon.ts:180"
              "subprocess:packages/node-client/src/pty.ts:382"
              "subprocess:packages/semantic-terminal/index.js:20"
              "subprocess:scripts/analyze-v3-self-evolution.mjs:132"
              "subprocess:scripts/analyze-v3-self-evolution.mjs:6"
              "subprocess:scripts/audit-claudecode-conversations.mjs:2"
              "subprocess:scripts/audit-claudecode-conversations.mjs:31"
              "subprocess:scripts/audit-codex-history-ingestion.mjs:13"
              "subprocess:scripts/audit-codex-history-ingestion.mjs:2"
              "subprocess:scripts/audit-gemini-conversations.mjs:6"
              "subprocess:scripts/backfill-memory-review-results.mjs:39"
              "subprocess:scripts/backfill-memory-review-results.mjs:5"
              "subprocess:scripts/backfill-memory-review-results.mjs:50"
              "subprocess:scripts/check-m6-deployment-status.mjs:379"
              "subprocess:scripts/check-m6-deployment-status.mjs:391"
              "subprocess:scripts/check-m6-deployment-status.mjs:6"
              "subprocess:scripts/check-missiond-hooks.mjs:3"
              "subprocess:scripts/check-project-domain-hardening.mjs:22"
              "subprocess:scripts/check-project-domain-hardening.mjs:3"
              "subprocess:scripts/check-project-maturity.mjs:580"
              "subprocess:scripts/check-project-maturity.mjs:6"
              "subprocess:scripts/check-project-ssot-universe.mjs:180"
              "subprocess:scripts/check-project-ssot-universe.mjs:192"
              "subprocess:scripts/check-project-ssot-universe.mjs:5"
              "subprocess:scripts/check-staged-source-hygiene.mjs:3"
              "subprocess:scripts/check-staged-source-hygiene.mjs:355"
              "subprocess:scripts/check-staged-source-hygiene.mjs:365"
              "subprocess:scripts/check-v3-autopilot-runtime-isomorphism.mjs:208"
              "subprocess:scripts/check-v3-code-isomorphism-complete.mjs:174"
              "subprocess:scripts/check-v3-code-isomorphism-complete.mjs:20"
              "subprocess:scripts/check-v3-code-isomorphism-complete.mjs:512"
              "subprocess:scripts/check-v3-direct-code-drift-policy.mjs:5"
              "subprocess:scripts/check-v3-direct-code-drift-policy.mjs:86"
              "subprocess:scripts/check-v3-final-convergence.mjs:13"
              "subprocess:scripts/check-v3-final-convergence.mjs:730"
              "subprocess:scripts/check-v3-final-convergence.mjs:847"
              "subprocess:scripts/check-v3-runtime-artifact-catalog.mjs:5"
              "subprocess:scripts/check-v3-runtime-artifact-catalog.mjs:95"
              "subprocess:scripts/check-v3-task-lifecycle-isomorphism.mjs:340"
              "subprocess:scripts/check-v3-task-lifecycle-isomorphism.mjs:521"
              "subprocess:scripts/check-v3-workstation-config-isomorphism.mjs:570"
              "subprocess:scripts/check-v3-workstation-config-isomorphism.mjs:931"
              "subprocess:scripts/cleanup-pty-diagnostics.mjs:249"
              "subprocess:scripts/cleanup-pty-diagnostics.mjs:5"
              "subprocess:scripts/cleanup-pty-diagnostics.mjs:51"
              "subprocess:scripts/collect-memory-review-wave.mjs:113"
              "subprocess:scripts/collect-memory-review-wave.mjs:4"
              "subprocess:scripts/context-pack-materialize-wave.mjs:17"
              "subprocess:scripts/context-pack-materialize-wave.mjs:386"
              "subprocess:scripts/context-pack-materialize-wave.mjs:524"
              "subprocess:scripts/dispatch-memory-review-direct-wave.mjs:4"
              "subprocess:scripts/dispatch-memory-review-direct-wave.mjs:45"
              "subprocess:scripts/dispatch-memory-review-wave.mjs:39"
              "subprocess:scripts/dispatch-memory-review-wave.mjs:4"
              "subprocess:scripts/export-human-user-utterances.mjs:2"
              "subprocess:scripts/export-human-user-utterances.mjs:32"
              "subprocess:scripts/import-claude-history-jsonl.mjs:106"
              "subprocess:scripts/import-claude-history-jsonl.mjs:2"
              "subprocess:scripts/import-claude-history-jsonl.mjs:231"
              "subprocess:scripts/install-missiond-hooks.mjs:3"
              "subprocess:scripts/install-missiond-hooks.mjs:366"
              "subprocess:scripts/kb-memory-triage.mjs:108"
              "subprocess:scripts/kb-memory-triage.mjs:2"
              "subprocess:scripts/label-claudecode-message-origin.mjs:2"
              "subprocess:scripts/label-claudecode-message-origin.mjs:24"
              "subprocess:scripts/lib/ocaml_lispc.mjs:3"
              "subprocess:scripts/lib/ocaml_lispc.mjs:33"
              "subprocess:scripts/lib/ocaml_lispc.mjs:95"
              "subprocess:scripts/lib/v3_workstation_runtime.mjs:2"
              "subprocess:scripts/lib/v3_workstation_runtime.mjs:238"
              "subprocess:scripts/mission-mcp-call.mjs:19"
              "subprocess:scripts/mission-mcp-call.mjs:2"
              "subprocess:scripts/missiond-work-order.mjs:210"
              "subprocess:scripts/missiond-work-order.mjs:5"
              "subprocess:scripts/normalize-claudecode-conversations.mjs:2"
              "subprocess:scripts/normalize-claudecode-conversations.mjs:305"
              "subprocess:scripts/normalize-claudecode-conversations.mjs:36"
              "subprocess:scripts/plan-task-runner.mjs:13"
              "subprocess:scripts/run-memory-review-supervisor.mjs:160"
              "subprocess:scripts/run-memory-review-supervisor.mjs:172"
              "subprocess:scripts/run-memory-review-supervisor.mjs:211"
              "subprocess:scripts/run-memory-review-supervisor.mjs:234"
              "subprocess:scripts/run-memory-review-supervisor.mjs:4"
              "subprocess:scripts/run-memory-review-supervisor.mjs:82"
              "subprocess:scripts/run-memory-review-supervisor.mjs:96"
              "subprocess:scripts/task-runner-append-event.mjs:6"
              "subprocess:scripts/task-runner-append-event.mjs:800"
              "subprocess:scripts/task-scope-guard.mjs:197"
              "subprocess:scripts/task-scope-guard.mjs:3"
              "subprocess:scripts/verify-task-contract.mjs:3"
              "subprocess:scripts/verify-task-contract.mjs:334"
              "subprocess:scripts/verify-task-contract.mjs:364"
              "subprocess:scripts/verify-task-contract.mjs:853"
              "subprocess:scripts/verify-task-run.mjs:14"]
    :code ["crates/missiond-daemon/src/context/slot_env.rs"
          "crates/missiond-daemon/src/engine/codex_replay.rs"
          "crates/missiond-daemon/src/engine/commit_convergence.rs"
          "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
          "crates/missiond-daemon/src/engine/master_control.rs"
          "crates/missiond-daemon/src/engine/nightly_evolution.rs"
          "crates/missiond-daemon/src/handlers/comm/conversation/events.rs"
          "crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs"
          "crates/missiond-daemon/src/handlers/compute/forge.rs"
          "crates/missiond-daemon/src/handlers/compute/slot.rs"
          "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
          "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_porcelain.rs"
          "crates/missiond-daemon/src/handlers/knowledge/kb/discovery.rs"
          "crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs"
          "crates/missiond-daemon/src/handlers/knowledge/project/registry.rs"
          "crates/missiond-daemon/src/handlers/knowledge/project/survey.rs"
          "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
          "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
          "crates/missiond-daemon/src/infra/mcp_client.rs"
          "crates/missiond-daemon/src/llm/codex_cli.rs"
          "crates/missiond-daemon/src/llm/gemini_cli.rs"
          "crates/missiond-daemon/src/llm/minimax_client.rs"
          "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
          "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
          "crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs"
          "crates/missiond-mcp/src/bin/mission-mcp.rs"
          "crates/missiond-pty/src/manager.rs"
          "crates/missiond-runner/src/runner.rs"
          "packages/node-client/src/binary.ts"
          "packages/node-client/src/client.ts"
          "packages/node-client/src/daemon.ts"
          "packages/node-client/src/pty.ts"
          "packages/semantic-terminal/index.js"
          "scripts/analyze-v3-self-evolution.mjs"
          "scripts/audit-claudecode-conversations.mjs"
          "scripts/audit-codex-history-ingestion.mjs"
          "scripts/audit-gemini-conversations.mjs"
          "scripts/backfill-memory-review-results.mjs"
          "scripts/check-m6-deployment-status.mjs"
          "scripts/check-missiond-hooks.mjs"
          "scripts/check-project-domain-hardening.mjs"
          "scripts/check-project-maturity.mjs"
          "scripts/check-project-ssot-universe.mjs"
          "scripts/check-staged-source-hygiene.mjs"
          "scripts/check-v3-autopilot-runtime-isomorphism.mjs"
          "scripts/check-v3-code-isomorphism-complete.mjs"
          "scripts/check-v3-direct-code-drift-policy.mjs"
          "scripts/check-v3-final-convergence.mjs"
          "scripts/check-v3-runtime-artifact-catalog.mjs"
          "scripts/check-v3-task-lifecycle-isomorphism.mjs"
          "scripts/check-v3-workstation-config-isomorphism.mjs"
          "scripts/cleanup-pty-diagnostics.mjs"
          "scripts/collect-memory-review-wave.mjs"
          "scripts/context-pack-materialize-wave.mjs"
          "scripts/dispatch-memory-review-direct-wave.mjs"
          "scripts/dispatch-memory-review-wave.mjs"
          "scripts/export-human-user-utterances.mjs"
          "scripts/import-claude-history-jsonl.mjs"
          "scripts/install-missiond-hooks.mjs"
          "scripts/kb-memory-triage.mjs"
          "scripts/label-claudecode-message-origin.mjs"
          "scripts/lib/ocaml_lispc.mjs"
          "scripts/lib/v3_workstation_runtime.mjs"
          "scripts/mission-mcp-call.mjs"
          "scripts/missiond-work-order.mjs"
          "scripts/normalize-claudecode-conversations.mjs"
          "scripts/plan-task-runner.mjs"
          "scripts/run-memory-review-supervisor.mjs"
          "scripts/task-runner-append-event.mjs"
          "scripts/task-scope-guard.mjs"
          "scripts/verify-task-contract.mjs"
          "scripts/verify-task-run.mjs"]
    :effects []
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/context/slot_env.rs:360"
      :file "crates/missiond-daemon/src/context/slot_env.rs"
      :symbol "resolve_cmd_value")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/codex_replay.rs:1049"
      :file "crates/missiond-daemon/src/engine/codex_replay.rs"
      :symbol "connect")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/commit_convergence.rs:499"
      :file "crates/missiond-daemon/src/engine/commit_convergence.rs"
      :symbol "command_status")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/commit_convergence.rs:508"
      :file "crates/missiond-daemon/src/engine/commit_convergence.rs"
      :symbol "git_diff_tree_changed_files")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/lisp_code_sync.rs:980"
      :file "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
      :symbol "run_command")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/master_control.rs:1302"
      :file "crates/missiond-daemon/src/engine/master_control.rs"
      :symbol "detect_code_first_drift")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/master_control.rs:1787"
      :file "crates/missiond-daemon/src/engine/master_control.rs"
      :symbol "probe_codex_mcp_ready")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/nightly_evolution.rs:334"
      :file "crates/missiond-daemon/src/engine/nightly_evolution.rs"
      :symbol "read_final_convergence_snapshot")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/nightly_evolution.rs:380"
      :file "crates/missiond-daemon/src/engine/nightly_evolution.rs"
      :symbol "ensure_compiled_runtime_available")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/nightly_evolution.rs:407"
      :file "crates/missiond-daemon/src/engine/nightly_evolution.rs"
      :symbol "run_self_evolution_analyzer")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/comm/conversation/events.rs:214"
      :file "crates/missiond-daemon/src/handlers/comm/conversation/events.rs"
      :symbol "handle_events")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs:216"
      :file "crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs"
      :symbol "gemini_watch_loop")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/compute/forge.rs:109"
      :file "crates/missiond-daemon/src/handlers/compute/forge.rs"
      :symbol "handle_lint")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/compute/forge.rs:46"
      :file "crates/missiond-daemon/src/handlers/compute/forge.rs"
      :symbol "handle_build")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/compute/slot.rs:69"
      :file "crates/missiond-daemon/src/handlers/compute/slot.rs"
      :symbol "mission_convergence_status")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/compute/task_delegate.rs:2668"
      :file "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
      :symbol "run_mechanic_repair_subprocess")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_porcelain.rs:77"
      :file "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_porcelain.rs"
      :symbol "run_git_status")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/knowledge/kb/discovery.rs:88"
      :file "crates/missiond-daemon/src/handlers/knowledge/kb/discovery.rs"
      :symbol "handle_kb_discover")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs:258"
      :file "crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs"
      :symbol "emit_plan_contract_json_via_lispc_sync")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs:265"
      :file "crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs"
      :symbol "emit_plan_contract_json_via_lispc_sync")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/knowledge/project/registry.rs:583"
      :file "crates/missiond-daemon/src/handlers/knowledge/project/registry.rs"
      :symbol "github_url_for_path")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/knowledge/project/survey.rs:33"
      :file "crates/missiond-daemon/src/handlers/knowledge/project/survey.rs"
      :symbol "handle_survey")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/sysinfra/infra.rs:285"
      :file "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
      :symbol "handle_inner")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/sysinfra/infra.rs:314"
      :file "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
      :symbol "handle_inner")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/sysinfra/infra.rs:344"
      :file "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
      :symbol "handle_inner")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/sysinfra/infra.rs:701"
      :file "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
      :symbol "handle_inner")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/sysinfra/system.rs:373"
      :file "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
      :symbol "daemon_update")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/sysinfra/system.rs:461"
      :file "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
      :symbol "daemon_update")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/sysinfra/system.rs:487"
      :file "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
      :symbol "daemon_update")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/sysinfra/system.rs:503"
      :file "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
      :symbol "daemon_update")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/sysinfra/system.rs:547"
      :file "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
      :symbol "daemon_update")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/infra/mcp_client.rs:147"
      :file "crates/missiond-daemon/src/infra/mcp_client.rs"
      :symbol "spawn")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/llm/codex_cli.rs:268"
      :file "crates/missiond-daemon/src/llm/codex_cli.rs"
      :symbol "exec_cli")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/llm/gemini_cli.rs:497"
      :file "crates/missiond-daemon/src/llm/gemini_cli.rs"
      :symbol "spawn_cli")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/llm/minimax_client.rs:140"
      :file "crates/missiond-daemon/src/llm/minimax_client.rs"
      :symbol "load_api_key")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/local/ast_sync_worker.rs:391"
      :file "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
      :symbol "git_diff_files")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/local/ast_sync_worker.rs:428"
      :file "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
      :symbol "git_ls_code_files")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/local/ast_sync_worker.rs:445"
      :file "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
      :symbol "git_head_hash")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/local/ast_sync_worker.rs:463"
      :file "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
      :symbol "git_status_changed_files")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/local/ast_sync_worker.rs:497"
      :file "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
      :symbol "git_repo_root")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:179"
      :file "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
      :symbol "resolve_repo_path")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:222"
      :file "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
      :symbol "git_diff_files")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:249"
      :file "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
      :symbol "git_diff_content")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:284"
      :file "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
      :symbol "commit_yaml_update")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:298"
      :file "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
      :symbol "commit_yaml_update")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs:311"
      :file "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
      :symbol "commit_yaml_update")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs:245"
      :file "crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs"
      :symbol "process_survey")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-mcp/src/bin/mission-mcp.rs:227"
      :file "crates/missiond-mcp/src/bin/mission-mcp.rs"
      :symbol "spawn_daemon")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-pty/src/manager.rs:261"
      :file "crates/missiond-pty/src/manager.rs"
      :symbol "spawn")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-runner/src/runner.rs:73"
      :file "crates/missiond-runner/src/runner.rs"
      :symbol "run")
    (anchor
      :role subprocess
      :observed "subprocess:packages/node-client/src/binary.ts:13"
      :file "packages/node-client/src/binary.ts")
    (anchor
      :role subprocess
      :observed "subprocess:packages/node-client/src/client.ts:182"
      :file "packages/node-client/src/client.ts")
    (anchor
      :role subprocess
      :observed "subprocess:packages/node-client/src/client.ts:700"
      :file "packages/node-client/src/client.ts"
      :symbol "getDefaultWsUrl")
    (anchor
      :role subprocess
      :observed "subprocess:packages/node-client/src/daemon.ts:10"
      :file "packages/node-client/src/daemon.ts")
    (anchor
      :role subprocess
      :observed "subprocess:packages/node-client/src/daemon.ts:180"
      :file "packages/node-client/src/daemon.ts"
      :symbol "cleanup")
    (anchor
      :role subprocess
      :observed "subprocess:packages/node-client/src/pty.ts:382"
      :file "packages/node-client/src/pty.ts")
    (anchor
      :role subprocess
      :observed "subprocess:packages/semantic-terminal/index.js:20"
      :file "packages/semantic-terminal/index.js"
      :symbol "isMusl")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/analyze-v3-self-evolution.mjs:132"
      :file "scripts/analyze-v3-self-evolution.mjs"
      :symbol "runFinalConvergence")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/analyze-v3-self-evolution.mjs:6"
      :file "scripts/analyze-v3-self-evolution.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/audit-claudecode-conversations.mjs:2"
      :file "scripts/audit-claudecode-conversations.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/audit-claudecode-conversations.mjs:31"
      :file "scripts/audit-claudecode-conversations.mjs"
      :symbol "run")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/audit-codex-history-ingestion.mjs:13"
      :file "scripts/audit-codex-history-ingestion.mjs"
      :symbol "run")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/audit-codex-history-ingestion.mjs:2"
      :file "scripts/audit-codex-history-ingestion.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/audit-gemini-conversations.mjs:6"
      :file "scripts/audit-gemini-conversations.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/backfill-memory-review-results.mjs:39"
      :file "scripts/backfill-memory-review-results.mjs"
      :symbol "psqlJson")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/backfill-memory-review-results.mjs:5"
      :file "scripts/backfill-memory-review-results.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/backfill-memory-review-results.mjs:50"
      :file "scripts/backfill-memory-review-results.mjs"
      :symbol "psqlExec")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-m6-deployment-status.mjs:379"
      :file "scripts/check-m6-deployment-status.mjs"
      :symbol "changedPathsSinceDeploy")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-m6-deployment-status.mjs:391"
      :file "scripts/check-m6-deployment-status.mjs"
      :symbol "resolveCommit")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-m6-deployment-status.mjs:6"
      :file "scripts/check-m6-deployment-status.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-missiond-hooks.mjs:3"
      :file "scripts/check-missiond-hooks.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-project-domain-hardening.mjs:22"
      :file "scripts/check-project-domain-hardening.mjs"
      :symbol "main")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-project-domain-hardening.mjs:3"
      :file "scripts/check-project-domain-hardening.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-project-maturity.mjs:580"
      :file "scripts/check-project-maturity.mjs"
      :symbol "hasBehaviorClosureEvidence")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-project-maturity.mjs:6"
      :file "scripts/check-project-maturity.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-project-ssot-universe.mjs:180"
      :file "scripts/check-project-ssot-universe.mjs"
      :symbol "main")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-project-ssot-universe.mjs:192"
      :file "scripts/check-project-ssot-universe.mjs"
      :symbol "main")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-project-ssot-universe.mjs:5"
      :file "scripts/check-project-ssot-universe.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-staged-source-hygiene.mjs:3"
      :file "scripts/check-staged-source-hygiene.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-staged-source-hygiene.mjs:355"
      :file "scripts/check-staged-source-hygiene.mjs"
      :symbol "failedCheck")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-staged-source-hygiene.mjs:365"
      :file "scripts/check-staged-source-hygiene.mjs"
      :symbol "failedCheck")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-autopilot-runtime-isomorphism.mjs:208"
      :file "scripts/check-v3-autopilot-runtime-isomorphism.mjs"
      :symbol "delegated")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-code-isomorphism-complete.mjs:174"
      :file "scripts/check-v3-code-isomorphism-complete.mjs"
      :symbol "has")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-code-isomorphism-complete.mjs:20"
      :file "scripts/check-v3-code-isomorphism-complete.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-code-isomorphism-complete.mjs:512"
      :file "scripts/check-v3-code-isomorphism-complete.mjs"
      :symbol "runPerSurfaceCheckers")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-direct-code-drift-policy.mjs:5"
      :file "scripts/check-v3-direct-code-drift-policy.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-direct-code-drift-policy.mjs:86"
      :file "scripts/check-v3-direct-code-drift-policy.mjs"
      :symbol "checkDiff")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-final-convergence.mjs:13"
      :file "scripts/check-v3-final-convergence.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-final-convergence.mjs:730"
      :file "scripts/check-v3-final-convergence.mjs"
      :symbol "runCheck")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-final-convergence.mjs:847"
      :file "scripts/check-v3-final-convergence.mjs"
      :symbol "readGitStatus")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-runtime-artifact-catalog.mjs:5"
      :file "scripts/check-v3-runtime-artifact-catalog.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-runtime-artifact-catalog.mjs:95"
      :file "scripts/check-v3-runtime-artifact-catalog.mjs"
      :symbol "gitTrackedRuntimeArtifacts")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-task-lifecycle-isomorphism.mjs:340"
      :file "scripts/check-v3-task-lifecycle-isomorphism.mjs"
      :symbol "readCommit")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-task-lifecycle-isomorphism.mjs:521"
      :file "scripts/check-v3-task-lifecycle-isomorphism.mjs"
      :symbol "fxBytes")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-workstation-config-isomorphism.mjs:570"
      :file "scripts/check-v3-workstation-config-isomorphism.mjs"
      :symbol "checkFiles")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-workstation-config-isomorphism.mjs:931"
      :file "scripts/check-v3-workstation-config-isomorphism.mjs"
      :symbol "buildFixture")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/cleanup-pty-diagnostics.mjs:249"
      :file "scripts/cleanup-pty-diagnostics.mjs"
      :symbol "applyDbCleanup")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/cleanup-pty-diagnostics.mjs:5"
      :file "scripts/cleanup-pty-diagnostics.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/cleanup-pty-diagnostics.mjs:51"
      :file "scripts/cleanup-pty-diagnostics.mjs"
      :symbol "psql")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/collect-memory-review-wave.mjs:113"
      :file "scripts/collect-memory-review-wave.mjs"
      :symbol "redactSecrets")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/collect-memory-review-wave.mjs:4"
      :file "scripts/collect-memory-review-wave.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/context-pack-materialize-wave.mjs:17"
      :file "scripts/context-pack-materialize-wave.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/context-pack-materialize-wave.mjs:386"
      :file "scripts/context-pack-materialize-wave.mjs"
      :symbol "validateTaskSources")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/context-pack-materialize-wave.mjs:524"
      :file "scripts/context-pack-materialize-wave.mjs"
      :symbol "runFixtures")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/dispatch-memory-review-direct-wave.mjs:4"
      :file "scripts/dispatch-memory-review-direct-wave.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/dispatch-memory-review-direct-wave.mjs:45"
      :file "scripts/dispatch-memory-review-direct-wave.mjs"
      :symbol "callTool")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/dispatch-memory-review-wave.mjs:39"
      :file "scripts/dispatch-memory-review-wave.mjs"
      :symbol "callTool")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/dispatch-memory-review-wave.mjs:4"
      :file "scripts/dispatch-memory-review-wave.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/export-human-user-utterances.mjs:2"
      :file "scripts/export-human-user-utterances.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/export-human-user-utterances.mjs:32"
      :file "scripts/export-human-user-utterances.mjs"
      :symbol "psql")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/import-claude-history-jsonl.mjs:106"
      :file "scripts/import-claude-history-jsonl.mjs"
      :symbol "psql")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/import-claude-history-jsonl.mjs:2"
      :file "scripts/import-claude-history-jsonl.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/import-claude-history-jsonl.mjs:231"
      :file "scripts/import-claude-history-jsonl.mjs"
      :symbol "copyRowsToTemp")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/install-missiond-hooks.mjs:3"
      :file "scripts/install-missiond-hooks.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/install-missiond-hooks.mjs:366"
      :file "scripts/install-missiond-hooks.mjs"
      :symbol "runInstall")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/kb-memory-triage.mjs:108"
      :file "scripts/kb-memory-triage.mjs"
      :symbol "psql")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/kb-memory-triage.mjs:2"
      :file "scripts/kb-memory-triage.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/label-claudecode-message-origin.mjs:2"
      :file "scripts/label-claudecode-message-origin.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/label-claudecode-message-origin.mjs:24"
      :file "scripts/label-claudecode-message-origin.mjs"
      :symbol "psql")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/lib/ocaml_lispc.mjs:3"
      :file "scripts/lib/ocaml_lispc.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/lib/ocaml_lispc.mjs:33"
      :file "scripts/lib/ocaml_lispc.mjs"
      :symbol "runLispc")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/lib/ocaml_lispc.mjs:95"
      :file "scripts/lib/ocaml_lispc.mjs"
      :symbol "commandExists")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/lib/v3_workstation_runtime.mjs:2"
      :file "scripts/lib/v3_workstation_runtime.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/lib/v3_workstation_runtime.mjs:238"
      :file "scripts/lib/v3_workstation_runtime.mjs"
      :symbol "emitWorkstationRuntimeConfigViaLispc")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/mission-mcp-call.mjs:19"
      :file "scripts/mission-mcp-call.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/mission-mcp-call.mjs:2"
      :file "scripts/mission-mcp-call.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/missiond-work-order.mjs:210"
      :file "scripts/missiond-work-order.mjs"
      :symbol "runGit")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/missiond-work-order.mjs:5"
      :file "scripts/missiond-work-order.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/normalize-claudecode-conversations.mjs:2"
      :file "scripts/normalize-claudecode-conversations.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/normalize-claudecode-conversations.mjs:305"
      :file "scripts/normalize-claudecode-conversations.mjs"
      :symbol "applySourceState")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/normalize-claudecode-conversations.mjs:36"
      :file "scripts/normalize-claudecode-conversations.mjs"
      :symbol "run")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/plan-task-runner.mjs:13"
      :file "scripts/plan-task-runner.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/run-memory-review-supervisor.mjs:160"
      :file "scripts/run-memory-review-supervisor.mjs"
      :symbol "runDirectWave")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/run-memory-review-supervisor.mjs:172"
      :file "scripts/run-memory-review-supervisor.mjs"
      :symbol "collectWaveOutput")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/run-memory-review-supervisor.mjs:211"
      :file "scripts/run-memory-review-supervisor.mjs"
      :symbol "terminateSlot")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/run-memory-review-supervisor.mjs:234"
      :file "scripts/run-memory-review-supervisor.mjs"
      :symbol "callMissionTool")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/run-memory-review-supervisor.mjs:4"
      :file "scripts/run-memory-review-supervisor.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/run-memory-review-supervisor.mjs:82"
      :file "scripts/run-memory-review-supervisor.mjs"
      :symbol "psqlJson")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/run-memory-review-supervisor.mjs:96"
      :file "scripts/run-memory-review-supervisor.mjs"
      :symbol "freeDiskGb")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/task-runner-append-event.mjs:6"
      :file "scripts/task-runner-append-event.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/task-runner-append-event.mjs:800"
      :file "scripts/task-runner-append-event.mjs"
      :symbol "runChild")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/task-scope-guard.mjs:197"
      :file "scripts/task-scope-guard.mjs"
      :symbol "runCommitMode")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/task-scope-guard.mjs:3"
      :file "scripts/task-scope-guard.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/verify-task-contract.mjs:3"
      :file "scripts/verify-task-contract.mjs")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/verify-task-contract.mjs:334"
      :file "scripts/verify-task-contract.mjs"
      :symbol "validateCommitArtifacts")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/verify-task-contract.mjs:364"
      :file "scripts/verify-task-contract.mjs"
      :symbol "readCommitBytes")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/verify-task-contract.mjs:853"
      :file "scripts/verify-task-contract.mjs"
      :symbol "runArtifactPlanWithBytes")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/verify-task-run.mjs:14"
      :file "scripts/verify-task-run.mjs"))

  (behavior
    :id missiond-navigation-worker
    :kind worker
    :owner navigation-gate
    :observed ["worker:arch-maintenance-worker"
              "worker:ast-sync-worker"
              "worker:codex-ingestion-worker"
              "worker:conversation-logger-worker"
              "worker:conversation-organizer-worker"
              "worker:embedding-loop-worker"
              "worker:gemini-logger-worker"
              "worker:gemini-reconcile-worker"
              "worker:lisp-survey-worker"
              "worker:pty-event-worker"
              "worker:reconcile-worker"
              "worker:retro-worker"
              "worker:strategy-worker"
              "worker:tagger-chunker-worker"
              "worker:vision-worker"
              "worker:xjpcode-briefing-worker"]
    :code ["crates/missiond-daemon/src/workers/codex/vision_worker.rs"
          "crates/missiond-daemon/src/workers/gemini/strategy_worker.rs"
          "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
          "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"
          "crates/missiond-daemon/src/workers/local/conversation_logger.rs"
          "crates/missiond-daemon/src/workers/local/conversation_organizer.rs"
          "crates/missiond-daemon/src/workers/local/gemini_logger.rs"
          "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
          "crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
          "crates/missiond-daemon/src/workers/local/reconcile_worker.rs"
          "crates/missiond-daemon/src/workers/local/tagger_chunker.rs"
          "crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs"
          "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
          "crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs"
          "crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs"
          "crates/missiond-daemon/src/workers/sonnet/retro_worker.rs"]
    :effects []
    (anchor
      :role worker
      :observed "worker:arch-maintenance-worker"
      :file "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
      :symbol "ArchMaintenanceWorker")
    (anchor
      :role worker
      :observed "worker:ast-sync-worker"
      :file "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
      :symbol "AstSyncWorker")
    (anchor
      :role worker
      :observed "worker:codex-ingestion-worker"
      :file "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"
      :symbol "CodexIngestionWorker")
    (anchor
      :role worker
      :observed "worker:conversation-logger-worker"
      :file "crates/missiond-daemon/src/workers/local/conversation_logger.rs"
      :symbol "ConversationLoggerWorker")
    (anchor
      :role worker
      :observed "worker:conversation-organizer-worker"
      :file "crates/missiond-daemon/src/workers/local/conversation_organizer.rs"
      :symbol "ConversationOrganizerWorker")
    (anchor
      :role worker
      :observed "worker:embedding-loop-worker"
      :file "crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs"
      :symbol "EmbeddingLoopWorker")
    (anchor
      :role worker
      :observed "worker:gemini-logger-worker"
      :file "crates/missiond-daemon/src/workers/local/gemini_logger.rs"
      :symbol "GeminiLoggerWorker")
    (anchor
      :role worker
      :observed "worker:gemini-reconcile-worker"
      :file "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
      :symbol "GeminiReconcileWorker")
    (anchor
      :role worker
      :observed "worker:lisp-survey-worker"
      :file "crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs"
      :symbol "LispSurveyWorker")
    (anchor
      :role worker
      :observed "worker:pty-event-worker"
      :file "crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
      :symbol "PtyEventWorker")
    (anchor
      :role worker
      :observed "worker:reconcile-worker"
      :file "crates/missiond-daemon/src/workers/local/reconcile_worker.rs"
      :symbol "ReconcileWorker")
    (anchor
      :role worker
      :observed "worker:retro-worker"
      :file "crates/missiond-daemon/src/workers/sonnet/retro_worker.rs"
      :symbol "RetroWorker")
    (anchor
      :role worker
      :observed "worker:strategy-worker"
      :file "crates/missiond-daemon/src/workers/gemini/strategy_worker.rs"
      :symbol "StrategyWorker")
    (anchor
      :role worker
      :observed "worker:tagger-chunker-worker"
      :file "crates/missiond-daemon/src/workers/local/tagger_chunker.rs"
      :symbol "TaggerChunkerWorker")
    (anchor
      :role worker
      :observed "worker:vision-worker"
      :file "crates/missiond-daemon/src/workers/codex/vision_worker.rs"
      :symbol "VisionWorker")
    (anchor
      :role worker
      :observed "worker:xjpcode-briefing-worker"
      :file "crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs"
      :symbol "XjpcodeBriefingWorker"))
  ;; END GENERATED NAVIGATION ANCHORS

  (effect
    :id global-claude-md-managed-section
    :feature global-claude-md-sync
    :kind filesystem-write
    :operation write
    :path-pattern "~/.claude/CLAUDE.md"
    :scope external-home
    :default disabled
    :kill-switch MISSIOND_CLAUDE_MD_SYNC
    :audit tracing)

  (effect
    :id mission-global-instruction-write
    :feature mission_global_instruction
    :kind filesystem-write
    :operation write
    :path-pattern "~/.claude/CLAUDE.md"
    :scope external-home
    :default enabled
    :kill-switch none
    :audit tool-result)

  (effect
    :id xjpcode-briefing-write
    :feature xjpcode-briefing-worker
    :kind filesystem-write
    :operation write
    :path-pattern "~/.xjpcode/xjpcode.md"
    :scope external-home
    :default enabled
    :kill-switch none
    :audit tracing)

  (effect
    :id project-vault-sync-write
    :feature project-vault-sync
    :kind filesystem-write
    :operation write
    :path-pattern "~/.missiond/vault/**"
    :scope external-home
    :default enabled
    :kill-switch none
    :audit tracing)

  (effect
    :id gemini-shadow-settings-write
    :feature gemini-cli-auth-shadow-home
    :kind filesystem-write
    :operation write
    :path-pattern "$MISSIOND_HOME/gemini-*-home/.gemini/settings.json"
    :scope runtime
    :default enabled
    :kill-switch none
    :audit tracing)

  (effect
    :id missiond-repo-file-write
    :feature missiond-runtime-artifacts
    :kind filesystem-write
    :operation write
    :path-pattern "**/*"
    :scope repo
    :default enabled
    :kill-switch none
    :audit runtime-report)

  (effect
    :id missiond-repo-file-append
    :feature missiond-runtime-artifacts
    :kind filesystem-write
    :operation append
    :path-pattern "**/*"
    :scope repo
    :default enabled
    :kill-switch none
    :audit runtime-report)

  (effect
    :id missiond-repo-file-rename
    :feature missiond-runtime-artifacts
    :kind filesystem-write
    :operation rename
    :path-pattern "**/*"
    :scope repo
    :default enabled
    :kill-switch none
    :audit runtime-report)

  (effect
    :id missiond-repo-file-delete
    :feature missiond-runtime-artifacts
    :kind filesystem-write
    :operation delete
    :path-pattern "**/*"
    :scope repo
    :default enabled
    :kill-switch none
    :audit runtime-report))
