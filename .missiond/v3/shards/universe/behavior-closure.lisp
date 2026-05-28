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
    :id gemini-cli-watcher-background-tasks
    :kind scheduler
    :owner conversation-ingestion
    :observed ["background-task:crates/missiond-core/src/gemini_cli/watcher.rs:66"
               "background-task:crates/missiond-core/src/gemini_cli/watcher.rs:145"]
    :code ["crates/missiond-core/src/gemini_cli/watcher.rs"]
    :effects []
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/gemini_cli/watcher.rs:66"
      :file "crates/missiond-core/src/gemini_cli/watcher.rs"
      :symbol "new")
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/gemini_cli/watcher.rs:145"
      :file "crates/missiond-core/src/gemini_cli/watcher.rs"
      :symbol "start")
    (trigger
      :from-file "crates/missiond-core/src/gemini_cli/watcher.rs"
      :from-symbol "GeminiCliWatcher::new"
      :calls "spawn ack-based Gemini cursor persistence loop")
    (trigger
      :from-file "crates/missiond-core/src/gemini_cli/watcher.rs"
      :from-symbol "GeminiCliWatcher::start"
      :calls "spawn Gemini CLI file watcher event handler"))

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
    :id board-runtime-metadata-backfill-psql
    :kind subprocess
    :owner control-plane-kernel
    :observed ["subprocess:scripts/backfill-board-runtime-metadata.mjs:249"]
    :code ["scripts/backfill-board-runtime-metadata.mjs"]
    :effects []
    (anchor
      :role subprocess
      :observed "subprocess:scripts/backfill-board-runtime-metadata.mjs:249"
      :file "scripts/backfill-board-runtime-metadata.mjs"
      :symbol "runPsql")
    (trigger
      :from-file "scripts/backfill-board-runtime-metadata.mjs"
      :from-symbol "applyBackfill"
      :calls "psql -X -v ON_ERROR_STOP=1 -f <generated-backfill-sql>"))

  (behavior
    :id agent-navigation-quality-subprocess
    :kind subprocess
    :owner agent-navigation
    :observed ["subprocess:scripts/agent-navigation.mjs:111"]
    :code ["scripts/agent-navigation.mjs"
           "scripts/check-v3-agent-navigation-quality.mjs"]
    :effects []
    (anchor
      :role subprocess
      :observed "subprocess:scripts/agent-navigation.mjs:111"
      :file "scripts/agent-navigation.mjs"
      :symbol "evaluateQuality")
    (trigger
      :from-file "scripts/agent-navigation.mjs"
      :from-symbol "evaluateQuality"
      :calls "scripts/check-v3-agent-navigation-quality.mjs"))

  (behavior
    :id autopilot-runtime-isomorphism-checker-subprocess
    :kind subprocess
    :owner autopilot-runtime
    :observed ["subprocess:scripts/check-v3-autopilot-runtime-isomorphism.mjs:223"]
    :code ["scripts/check-v3-autopilot-runtime-isomorphism.mjs"]
    :effects []
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-autopilot-runtime-isomorphism.mjs:223"
      :file "scripts/check-v3-autopilot-runtime-isomorphism.mjs"
      :symbol "delegated")
    (trigger
      :from-file "scripts/check-v3-autopilot-runtime-isomorphism.mjs"
      :from-symbol "delegated"
      :calls "static-source-needle"))

  (behavior
    :id v3-code-isomorphism-aggregate-subprocess
    :kind subprocess
    :owner typed-lisp-compiler
    :observed ["subprocess:scripts/check-v3-code-isomorphism-complete.mjs:179"
               "subprocess:scripts/check-v3-code-isomorphism-complete.mjs:517"]
    :code ["scripts/check-v3-code-isomorphism-complete.mjs"]
    :effects []
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-code-isomorphism-complete.mjs:179"
      :file "scripts/check-v3-code-isomorphism-complete.mjs"
      :symbol "has")
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-code-isomorphism-complete.mjs:517"
      :file "scripts/check-v3-code-isomorphism-complete.mjs"
      :symbol "runPerSurfaceCheckers")
    (trigger
      :from-file "scripts/check-v3-code-isomorphism-complete.mjs"
      :from-symbol "runPerSurfaceCheckers"
      :calls "per-surface-checker-subprocess"))

  (behavior
    :id final-convergence-runtime-bootstrap-subprocess
    :kind subprocess
    :owner typed-lisp-compiler
    :observed ["subprocess:scripts/check-v3-final-convergence.mjs:*"]
    :code ["scripts/check-v3-final-convergence.mjs"
           "scripts/compile-v3-runtime.mjs"]
    :effects []
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-final-convergence.mjs:*"
      :file "scripts/check-v3-final-convergence.mjs"
      :symbol "ensureCompiledRuntimeProjections")
    (trigger
      :from-file "scripts/check-v3-final-convergence.mjs"
      :from-symbol "ensureCompiledRuntimeProjections"
      :calls "node scripts/compile-v3-runtime.mjs --write --json"))

  (behavior
    :id jarvis-intent-plan-dispatch-smoke-secret-read
    :kind subprocess
    :owner interaction-gateway
    :observed ["subprocess:scripts/smoke-jarvis-intent-plan-dispatch.mjs:36"]
    :code ["scripts/smoke-jarvis-intent-plan-dispatch.mjs"]
    :effects []
    (anchor
      :role subprocess
      :observed "subprocess:scripts/smoke-jarvis-intent-plan-dispatch.mjs:36"
      :file "scripts/smoke-jarvis-intent-plan-dispatch.mjs"
      :symbol "tokenFromSecretStore")
    (trigger
      :from-file "scripts/smoke-jarvis-intent-plan-dispatch.mjs"
      :from-symbol "tokenFromSecretStore"
      :calls "xjp secret get --raw"))

  (behavior
    :id jarvis-intent-plan-dispatch-smoke-timeout
    :kind scheduler
    :owner interaction-gateway
    :observed ["scheduler:scripts/smoke-jarvis-intent-plan-dispatch.mjs:54"
               "scheduler:scripts/smoke-jarvis-intent-plan-dispatch.mjs:*"]
    :code ["scripts/smoke-jarvis-intent-plan-dispatch.mjs"]
    :effects []
    (anchor
      :role scheduler
      :observed "scheduler:scripts/smoke-jarvis-intent-plan-dispatch.mjs:54"
      :file "scripts/smoke-jarvis-intent-plan-dispatch.mjs"
      :symbol "postInteraction")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/smoke-jarvis-intent-plan-dispatch.mjs:*"
      :file "scripts/smoke-jarvis-intent-plan-dispatch.mjs"
      :symbol "followTaskUntilTerminal")
    (trigger
      :from-file "scripts/smoke-jarvis-intent-plan-dispatch.mjs"
      :from-symbol "postInteraction"
      :calls "AbortController timeout for bounded Jarvis SSE smoke"))

  (behavior
    :id stale-boardtask-final-audit-ipc-timeout
    :kind scheduler
    :owner interaction-gateway
    :observed ["scheduler:scripts/audit-stale-boardtask-finals.mjs:183"]
    :code ["scripts/audit-stale-boardtask-finals.mjs"]
    :effects []
    (anchor
      :role scheduler
      :observed "scheduler:scripts/audit-stale-boardtask-finals.mjs:183"
      :file "scripts/audit-stale-boardtask-finals.mjs"
      :symbol "callMissiond")
    (trigger
      :from-file "scripts/audit-stale-boardtask-finals.mjs"
      :from-symbol "callMissiond"
      :calls "bounded MissionD IPC timeout for stale-final audit"))

  (behavior
    :id control-plane-worktree-verifier-git-subprocesses
    :kind subprocess
    :owner control-plane-kernel
    :observed ["subprocess:crates/missiond-daemon/src/engine/shared_memory.rs:*"]
    :code ["crates/missiond-daemon/src/engine/shared_memory.rs"]
    :effects []
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/shared_memory.rs:*"
      :file "crates/missiond-daemon/src/engine/shared_memory.rs"
      :symbol "git_status_changed_paths")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/shared_memory.rs:*"
      :file "crates/missiond-daemon/src/engine/shared_memory.rs"
      :symbol "git_changed_paths_between")
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/engine/shared_memory.rs:*"
      :file "crates/missiond-daemon/src/engine/shared_memory.rs"
      :symbol "git_head")
    (trigger
      :from-file "crates/missiond-daemon/src/engine/shared_memory.rs"
      :from-symbol "git_status_changed_paths"
      :calls "git -C <project_root> status --porcelain=v1 for attempt-bound changed path verification")
    (trigger
      :from-file "crates/missiond-daemon/src/engine/shared_memory.rs"
      :from-symbol "git_changed_paths_between"
      :calls "git -C <project_root> diff --name-only <pre>..<post> for attempt-bound changed path verification")
    (trigger
      :from-file "crates/missiond-daemon/src/engine/shared_memory.rs"
      :from-symbol "git_head"
      :calls "git -C <project_root> rev-parse HEAD for attempt-bound worktree manifest verification"))

  (behavior
    :id ws-server-interaction-frontend-streaming
    :kind scheduler
    :owner interaction-gateway
    :observed ["background-task:crates/missiond-core/src/ws/server.rs:*"
               "scheduler:crates/missiond-core/src/ws/server.rs:*"]
    :code ["crates/missiond-core/src/ws/server.rs"]
    :effects []
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-core/src/ws/server.rs:*"
      :file "crates/missiond-core/src/ws/server.rs"
      :symbol "handle_chat_completions")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-core/src/ws/server.rs:*"
      :file "crates/missiond-core/src/ws/server.rs"
      :symbol "handle_pty_subscription")
    (anchor
      :role scheduler
      :observed "scheduler:crates/missiond-core/src/ws/server.rs:*"
      :file "crates/missiond-core/src/ws/server.rs"
      :symbol "handle_events_subscription")
    (trigger
      :from-file "crates/missiond-core/src/ws/server.rs"
      :from-symbol "handle_chat_completions"
      :calls "bridge Jarvis chat task execution into SSE activity/result stream")
    (trigger
      :from-file "crates/missiond-core/src/ws/server.rs"
      :from-symbol "handle_pty_subscription"
      :calls "send PTY replay buffer to late-joining Terminal clients")
    (trigger
      :from-file "crates/missiond-core/src/ws/server.rs"
      :from-symbol "handle_events_subscription"
      :calls "close frontend EventBus websocket with typed unavailable diagnostic when publisher is missing"))

  (behavior
    :id conversation-maintenance-reconcile-background
    :kind scheduler
    :owner conversation-ingestion
    :observed ["background-task:crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs:*"]
    :code ["crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs"]
    :effects []
    (anchor
      :role scheduler
      :observed "background-task:crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs:*"
      :file "crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs"
      :symbol "handle_maintenance")
    (trigger
      :from-file "crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs"
      :from-symbol "handle_maintenance"
      :calls "spawn full conversation reconciliation worker from explicit maintenance request"))

  (behavior
    :id conversation-events-git-log-subprocess
    :kind subprocess
    :owner conversation-ingestion
    :observed ["subprocess:crates/missiond-daemon/src/handlers/comm/conversation/events.rs:*"]
    :code ["crates/missiond-daemon/src/handlers/comm/conversation/events.rs"]
    :effects []
    (anchor
      :role subprocess
      :observed "subprocess:crates/missiond-daemon/src/handlers/comm/conversation/events.rs:*"
      :file "crates/missiond-daemon/src/handlers/comm/conversation/events.rs"
      :symbol "handle_events")
    (trigger
      :from-file "crates/missiond-daemon/src/handlers/comm/conversation/events.rs"
      :from-symbol "handle_events"
      :calls "git log --oneline bounded by requested time range for conversation event context"))

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
