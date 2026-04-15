;; MissionD — Data Flows
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

  (flows
    (flow user-message-to-knowledge
      (description "raw PTY output → persisted message → extracted knowledge")
      (steps
        pty-session -> semantic-parser -> conversation-logger
        -> tagger-chunker -> embedding-worker -> knowledge))

    (flow board-task-lifecycle
      (description "task creation → autopilot claim → slot dispatch → completion")
      (steps
        board-create -> autopilot-tick -> slot-dispatch
        -> cc-controller -> pty-session -> result-harvest -> board-done))

    (flow decision-cascade
      (description "agent question → multi-tier resolution")
      (steps
        question-raised -> kb-lookup -> gemini-consult
        -> decision-slot -> human-escalation -> answer-routed))

    (flow mcp-request
      (description "external MCP call → daemon handler → response")
      (steps
        stdio-jsonrpc -> mcp-server -> ipc-bridge
        -> handler-dispatch -> db-query -> jsonrpc-response))

    (flow context-assembly
      (description "slot activation → budget-constrained prompt building")
      (steps
        slot-activated -> context-pipeline -> budget-allocator
        -> source-ranker -> kb-fetch -> skill-fetch -> history-fetch
        -> truncation -> assembled-prompt))

    (flow retrospective
      (description "session end → analysis → knowledge extraction")
      (steps
        session-end -> retro-worker -> tool-stats -> pattern-analysis
        -> sonnet-summarize -> retrospective-result -> knowledge-upsert))

    (flow embedding-pipeline
      (description "new content → embedding generation → vector storage")
      (steps
        content-created -> embedding-worker -> model-inference
        -> vector-storage -> search-index-ready))

    (flow gemini-llm-call
      (description "LLM request → rate limiting → provider dispatch → logging")
      (steps
        handler-request -> llm-gate -> rate-check
        -> gemini-client -> api-call -> gemini-logger -> response))

    ;; ── Project 隔离体系流 (P1-P5, 2026-04-10) ──────────────────────────────
    (flow project-scoped-kb-search
      (description "带 project_id 的 KB 检索：项目知识 + 全局知识并集")
      (steps
        mcp-kb-query-with-project -> kb-handler-routes-to-scoped-variant
        -> kb_search_fts_ranked_scoped
        -> sql-WHERE-project_id-eq-OR-IS-NULL -> ranked-results-returned)
      :note "project_id=NULL 条目对所有项目可见；项目专属条目仅对该项目可见")

    (flow project-cwd-resolution
      (description "从 CWD 自动解析 project_id — 已实现 (commit e18d0bf)")
      :status done
      (steps
        message-arrives-with-cwd
        -> message_handler.ingest-extracts-first-msg-cwd
        -> state.project_registry.read()
        -> ProjectRegistry.resolve(cwd)-longest-prefix-match
        -> project_id-set-on-Conversation-struct
        -> persisted-to-PG-via-upsert_conversation)
      :note "daemon init 时从 PG list_projects() 加载 → SharedProjectRegistry 注入 AppState; message_handler 每次 ingest 时自动解析")

    (flow project-sync
      (description "自动发现并注册 ~/.claude/projects/ 下的项目")
      (steps
        mission_project-sync-action -> scan-~/.claude/projects/ -> for-each-dir
        -> decode-dir-name-as-path -> skip-if-exists -> upsert_project-to-PG))

    (flow project-init
      :added "84ac1a6"
      (description "一步注册新项目：path → 完整项目元数据 → DB + 历史回填 + 注册表热重载")
      (steps
        mission_project-init-action{path,id?,slots?}
        -> canonicalize-path
        -> derive-id-from-dir-name
        -> git-remote-get-url-origin → github_url
        -> scan-intent-lisp-candidates(".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp")
        -> upsert_project-to-PG
        -> backfill_project_id(path%) → N rows updated
        -> backfill_project_id(claude-encoded-pattern%) → M rows updated
        -> reload-SharedProjectRegistry
        -> return{id path githubUrl intentPath backfilledConversations status})))
