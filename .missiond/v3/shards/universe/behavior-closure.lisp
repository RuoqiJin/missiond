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
    :observed ["subprocess:scripts/check-v3-autopilot-runtime-isomorphism.mjs:216"]
    :code ["scripts/check-v3-autopilot-runtime-isomorphism.mjs"]
    :effects []
    (anchor
      :role subprocess
      :observed "subprocess:scripts/check-v3-autopilot-runtime-isomorphism.mjs:216"
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
    :observed ["scheduler:scripts/smoke-jarvis-intent-plan-dispatch.mjs:53"
               "scheduler:scripts/smoke-jarvis-intent-plan-dispatch.mjs:243"
               "scheduler:scripts/smoke-jarvis-intent-plan-dispatch.mjs:267"]
    :code ["scripts/smoke-jarvis-intent-plan-dispatch.mjs"]
    :effects []
    (anchor
      :role scheduler
      :observed "scheduler:scripts/smoke-jarvis-intent-plan-dispatch.mjs:53"
      :file "scripts/smoke-jarvis-intent-plan-dispatch.mjs"
      :symbol "postInteraction")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/smoke-jarvis-intent-plan-dispatch.mjs:243"
      :file "scripts/smoke-jarvis-intent-plan-dispatch.mjs"
      :symbol "followTaskUntilTerminal")
    (anchor
      :role scheduler
      :observed "scheduler:scripts/smoke-jarvis-intent-plan-dispatch.mjs:267"
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
