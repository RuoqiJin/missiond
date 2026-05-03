;; MissionD — Pillar: transport-bootstrap
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

  (pillar transport-bootstrap
    (purpose "IPC, WebSocket, PTY transport + daemon initialization")

    (component ws-server
      :target "crates/missiond-core/src/ws/server.rs"
      (sub-components
        (screenshot-broker :target "crates/missiond-core/src/ws/screenshot_broker.rs")
        (jarvis-trace      :target "crates/missiond-core/src/ws/jarvis_trace.rs")))

    (component ipc
      :target "crates/missiond-core/src/ipc/mod.rs")

    (component pty-manager
      :target "crates/missiond-pty/src/manager.rs"
      (sub-components
        (session    :target "crates/missiond-pty/src/session.rs")
        (screenshot :target "crates/missiond-pty/src/screenshot.rs")
        (extractor  :target "crates/missiond-pty/src/extractor.rs")
        (anomaly    :target "crates/missiond-pty/src/anomaly.rs")))

    (component daemon-init
      :target "crates/missiond-daemon/src/main.rs"
      (init-order
        ;; Phase 1: Infrastructure
        db -> embed_model -> event_bus
        ;; Phase 1.5: Project Registry (commit e18d0bf — loaded from PG before slot_manager)
        -> project_registry
        ;; Phase 2: Core modules
        -> pty_manager -> slot_manager -> mission_control
        ;; Phase 3: Gateways
        -> gemini_gateway -> sonnet_gateway -> llm_gateway
        ;; Phase 4: Pipelines
        -> context_pipeline -> worker_registry -> control_tree
        ;; Phase 5: Workers (18 spawns)
        -> all-workers
        ;; Phase 6: Engines
        -> autopilot -> ipc-handler -> ws-server)

      (depends-graph
        (project_registry (db)
          :note "store.list_projects() → ProjectRegistry::new(projects) → SharedProjectRegistry")
        (pty_manager     (event_bus))
        (slot_manager    (db pty_manager event_bus))
        (mission_control (db slot_manager event_bus))
        (gemini_gateway  (db))
        (sonnet_gateway  (slot_manager))
        (llm_gateway     (gemini_gateway sonnet_gateway))
        (context_pipeline (db slot_manager))
        (autopilot       (db slot_manager event_bus llm_gateway context_pipeline))))

    (component state-management
      :target "crates/missiond-daemon/src/state.rs"
      ;; commit e18d0bf: AppState 新增 project_registry: SharedProjectRegistry
      ;; Arc<RwLock<ProjectRegistry>> — path→project_id 解析 + 项目元数据缓存
      (field project_registry :type SharedProjectRegistry
        :doc "loaded from PG at daemon init, used by message_handler for CWD→project_id resolution"))

    (component supervisor
      :target "crates/missiond-daemon/src/supervisor.rs")

    (component infra-modules
      (module aiops             :target "crates/missiond-daemon/src/infra/aiops.rs")
      (module daemon-stats      :target "crates/missiond-daemon/src/infra/daemon_stats.rs")
      ;; ⚠ git-watcher REMOVED (commit 65c8b59) — replaced by event_analyzer_worker (local BackgroundWorker)
      ;; was: 30s git-log polling via crates/missiond-daemon/src/infra/git_watcher.rs
      (module ingestion-router  :target "crates/missiond-daemon/src/infra/ingestion_router.rs")
      (module ipc-handler       :target "crates/missiond-daemon/src/infra/ipc_handler.rs"
        ;; commit ec269d7: added instruction to slots to use mission_conversation_query(action=get, sessionId=…, tail=N)
        ;; for reading past turn content (auto-memory was defaulting to own memory directory)
        )
      (module mcp-client        :target "crates/missiond-daemon/src/infra/mcp_client.rs")
      (module message-handler   :target "crates/missiond-daemon/src/infra/message_handler.rs"
        ;; commit e18d0bf: ingest() 内 project_id 从 state.project_registry.read().resolve(cwd) 自动填充
        ;; 取第一条消息的 cwd 字段（fallback 到 project_path），对 ProjectRegistry 做最长前缀匹配
        ;; role mapping: ingest() delegates to events_sync::normalize_claude_message_role —
        ;;   raw_role kept verbatim, role column receives canonical taxonomy
        ;;   (tool_result | thinking | compact_summary | agent_user/agent_assistant | worker_user | passthrough).
        ;;   semantic_roles label sidecar is populated only when normalized role differs from raw_role.
        )
      (module session-util      :target "crates/missiond-daemon/src/infra/session_util.rs")))

