;; ============================================================
;; MissionD Intent Declaration (Phase 1: High-Dimensional Survey)
;; Generated: 2026-04-02  |  Forge Methodology v1
;; Status: DRAFT — surveyed, not yet validated against source
;; ============================================================
;;
;; Pattern Coverage Legend:
;;   [P1] crud-gateway      — DB CRUD layer
;;   [P2] event-listener    — Event-driven workers
;;   [P3] cron-worker       — Interval/tick workers
;;   [P4] state-machine     — Finite state automata
;;   [P5] mcp-tool          — MCP tool registration & dispatch
;;   [P6] bootstrap         — DI & initialization wiring
;;   [GAP:xxx]              — No existing mold, needs new pattern
;;
;; ============================================================

(intent missiond
  (granularity L3-Implementation)
  (governance-mode cartography)
  (generated-at 2026-04-02T00:00:00Z)
  (survey-phase 1)

  ;; ============================================================
  ;; GLOBAL DESIGN CONSTRAINTS
  ;; ============================================================
  (design-constraints
    (crate-boundary
      (missiond-core   "types, DB, PTY, semantic, embedding, IPC")
      (missiond-daemon "business logic, handlers, engines, workers, LLM")
      (missiond-mcp    "MCP JSON-RPC server, tool schema definitions")
      (missiond-attach "CLI PTY attach utility")
      (missiond-runner "Claude CLI wrapper")
      (semantic-terminal-napi "Node N-API bindings")
      (skill-store     "skill/workflow management"))

    (communication
      (internal  "DaemonEvent broadcast + per-worker MPSC channels")
      (external  "IPC (Unix socket/TCP) between mcp↔daemon")
      (frontend  "WebSocket (tokio-tungstenite) for realtime UI"))

    (db-access
      (backend "PostgreSQL (sqlx) + SQLite (rusqlite) dual-mode")
      (gateway "missiond-core/db/ is sole DB gateway")
      (migrations "20+ SQL migration files"))

    (authority
      (pty-state         "semantic parser is authority")
      (task-lifecycle    "autopilot tick loop is authority")
      (slot-lifecycle    "slot_manager is authority")
      (knowledge-truth   "PostgreSQL KB table is authority")))

  ;; ============================================================
  ;; PILLAR 1: DATA LAYER (missiond-core/db/)
  ;; Pattern: [P1] crud-gateway
  ;; ============================================================
  (pillar data-layer
    (purpose "sole DB gateway — all tables, all CRUD, all migrations")

    ;; --- Task & Inbox Domain ---
    (component tasks-db
      :pattern crud-gateway
      :target "missiond-core/src/db/mod.rs"  ;; or split files
      (table tasks
        (col id :type text :pk)
        (col role :type text)
        (col status :type text :default "queued")
        (col slot_id :type text)
        (col prompt :type text)
        (col result :type text)
        (col error :type text)
        (col created_at :type timestamptz)
        (col updated_at :type timestamptz))
      (table inbox
        (col id :type integer :pk :auto-increment)
        (col task_id :type text)
        (col from_role :type text)
        (col content :type text)
        (col read :type boolean :default false)
        (col created_at :type timestamptz))
      (table events
        (col id :type integer :pk :auto-increment)
        (col task_id :type text)
        (col event_type :type text)
        (col data :type jsonb)
        (col timestamp :type timestamptz)))

    ;; --- Knowledge Domain ---
    (component knowledge-db
      :pattern crud-gateway
      :target "missiond-core/src/db/knowledge.rs"
      (table knowledge
        (col id :type integer :pk :auto-increment)
        (col category :type text)
        (col key :type text)
        (col summary :type text)
        (col detail :type jsonb)
        (col confidence :type float8 :default 1.0)
        (col embedding_vec :type blob)
        (col utility_score :type float8)
        (col created_at :type timestamptz)
        (col updated_at :type timestamptz))
      (table knowledge_edges
        (col id :type integer :pk)
        (col source_id :type integer)
        (col target_id :type integer)
        (col relation_type :type text)
        (col weight :type float8))
      (table kb_access_log
        (col id :type integer :pk)
        (col kb_id :type integer)
        (col co_accessed_ids :type jsonb)
        (col context_source :type text)
        (col accessed_at :type timestamptz))
      (table kb_operation_queue
        (col id :type integer :pk)
        (col operation :type text)
        (col target_keys :type jsonb)
        (col status :type text)
        (col created_at :type timestamptz))
      (table kb_ast_links
        (col id :type integer :pk)
        (col kb_id :type integer)
        (col symbol_name :type text)
        (col file_path :type text)
        (col relation :type text)
        (col confidence :type float8)))

    ;; --- Conversation Domain ---
    (component conversation-db
      :pattern crud-gateway
      :target "missiond-core/src/db/conversation.rs"
      (table conversations
        (col id :type text :pk)
        (col slot_id :type text)
        (col status :type text)
        (col message_count :type integer)
        (col analyzed_at :type timestamptz)
        (col memory_forwarded_at :type timestamptz)
        (col created_at :type timestamptz))
      (table conversation_messages
        (col id :type integer :pk)
        (col session_id :type text)
        (col role :type text)
        (col content :type text)
        (col timestamp :type timestamptz)
        (col tool_name :type text)
        (col fts_content :type text))
      (table conversation_events
        (col id :type integer :pk)
        (col session_id :type text)
        (col event_type :type text)
        (col timestamp :type timestamptz))
      (table conversation_tool_calls
        (col id :type integer :pk)
        (col session_id :type text)
        (col tool_name :type text)
        (col input_summary :type text)
        (col output_summary :type text))
      (table conversation_topic_vectors
        (col id :type integer :pk)
        (col session_id :type text)
        (col chunk_idx :type integer)
        (col embedding_vec :type blob)))

    ;; --- Board Domain ---
    (component board-db
      :pattern crud-gateway
      :target "missiond-core/src/db/board.rs"
      (table board_tasks
        (col id :type text :pk)
        (col title :type text)
        (col description :type text)
        (col status :type text :default "open")
        (col priority :type text :default "medium")
        (col parent_id :type text)
        (col flow_phase :type text)
        (col depends_on :type jsonb)
        (col assignee :type text)
        (col created_at :type timestamptz)
        (col updated_at :type timestamptz))
      (table board_task_notes
        (col id :type integer :pk)
        (col task_id :type text)
        (col content :type text)
        (col note_type :type text)
        (col author :type text)
        (col created_at :type timestamptz)))

    ;; --- Agent Questions ---
    (component question-db
      :pattern crud-gateway
      :target "missiond-core/src/db/question.rs"
      (table agent_questions
        (col id :type text :pk)
        (col question :type text)
        (col status :type text :default "pending")
        (col target :type text)
        (col decision_type :type text)
        (col routing_trace :type jsonb)
        (col answer :type text)
        (col created_at :type timestamptz)))

    ;; --- Slot & State ---
    (component slot-state-db
      :pattern crud-gateway
      (table slot_sessions
        (col slot_id :type text :pk)
        (col session_id :type text)
        (col updated_at :type timestamptz))
      (table dynamic_slots
        (col id :type text :pk)
        (col template :type text)
        (col objective :type text)
        (col ttl :type integer)
        (col status :type text)
        (col created_at :type timestamptz))
      (table daemon_state
        (col key :type text :pk)
        (col value :type jsonb)
        (col updated_at :type timestamptz)))

    ;; --- Slot Tasks & Tokens ---
    (component slot-tasks-db
      :pattern crud-gateway
      (table slot_tasks
        (col id :type integer :pk)
        (col slot_id :type text)
        (col task_type :type text)
        (col status :type text)
        (col duration_ms :type integer)
        (col created_at :type timestamptz))
      (table token_usage_ledger
        (col id :type integer :pk)
        (col slot_id :type text)
        (col conversation_id :type text)
        (col model :type text)
        (col input_tokens :type integer)
        (col output_tokens :type integer)
        (col cache_tokens :type integer)
        (col recorded_at :type timestamptz)))

    ;; --- Skills ---
    (component skill-db
      :pattern crud-gateway
      :target "missiond-core/src/db/skill.rs"
      (table skill_topics
        (col id :type integer :pk)
        (col topic :type text)
        (col description :type text)
        (col file_path :type text)
        (col hit_count :type integer :default 0)
        (col embedding_vec :type blob))
      (table skill_blocks
        (col id :type integer :pk)
        (col topic :type text)
        (col block_type :type text)
        (col title :type text)
        (col content :type text)
        (col fts_doc :type text))
      (table skill_versions
        (col id :type integer :pk)
        (col topic :type text)
        (col checksum :type text)
        (col created_at :type timestamptz))
      (table skill_executions
        (col id :type integer :pk)
        (col skill_topic :type text)
        (col action_id :type text)
        (col status :type text)
        (col steps_total :type integer)
        (col steps_completed :type integer)))

    ;; --- Observability ---
    (component observability-db
      :pattern crud-gateway
      (table gemini_requests
        (col id :type integer :pk)
        (col caller :type text)
        (col model :type text)
        (col prompt_chars :type integer)
        (col duration_ms :type integer)
        (col status :type text)
        (col error_msg :type text)
        (col created_at :type timestamptz))
      (table gemini_file_uploads
        (col id :type integer :pk)
        (col file_hash :type text)
        (col file_uri :type text)
        (col expires_at :type timestamptz))
      (table incidents
        (col id :type integer :pk)
        (col severity :type text)
        (col source :type text)
        (col title :type text)
        (col server_id :type text)
        (col dedupe_key :type text)
        (col created_at :type timestamptz))
      (table system_timeline
        (col seq :type integer :pk)
        (col trace_id :type text)
        (col event_type :type text)
        (col summary :type text)
        (col payload :type jsonb)
        (col fts_doc :type text)
        (col created_at :type timestamptz))
      (table router_chat_archive
        (col id :type integer :pk)
        (col session_id :type text)
        (col role :type text)
        (col content :type text)
        (col archive_reason :type text)
        (col created_at :type timestamptz)))

    ;; --- Watcher & Cursor ---
    (component cursor-db
      :pattern crud-gateway
      (table watcher_cursors
        (col file_path :type text :pk)
        (col byte_offset :type bigint)
        (col session_id :type text))
      (table consumer_watermarks
        (col consumer_name :type text :pk)
        (col session_id :type text)
        (col last_processed_msg_id :type integer)))

    ;; --- Code Intelligence ---
    (component code-intel-db
      :pattern crud-gateway
      :target "missiond-core/src/db/ast.rs"
      (table ast_nodes
        (col id :type integer :pk)
        (col repo :type text)
        (col file_path :type text)
        (col node_type :type text)
        (col name :type text)
        (col signature :type text)
        (col start_line :type integer)
        (col embedding_vec :type blob))
      (table ast_file_meta
        (col id :type integer :pk)
        (col repo :type text)
        (col file_path :type text)
        (col commit_hash :type text)
        (col node_count :type integer))
      (table beacons
        (col id :type integer :pk)
        (col name :type text)
        (col description :type text)
        (col harvest_count :type integer :default 0))
      (table beacon_nodes
        (col id :type integer :pk)
        (col beacon_id :type integer)
        (col repo :type text)
        (col symbol_name :type text)))

    ;; --- Analysis & Narrative ---
    (component narrative-db
      :pattern crud-gateway
      (table message_narrations
        (col id :type integer :pk)
        (col message_id :type integer)
        (col step_title :type text)
        (col step_intent :type text)
        (col step_result :type text))
      (table narration_cursors
        (col session_id :type text :pk)
        (col last_processed_id :type integer)
        (col batch_index :type integer)
        (col status :type text))
      (table retrospective_results
        (col id :type integer :pk)
        (col session_id :type text)
        (col trigger_reason :type text)
        (col quick_stats :type jsonb)
        (col full_analysis :type text)))

    ;; --- Caching ---
    (component cache-db
      :pattern crud-gateway
      (table image_descriptions
        (col image_hash :type text :pk)
        (col media_type :type text)
        (col description :type text))
      (table message_translations
        (col message_id :type integer :pk)
        (col translation :type text)
        (col source_lang :type text)
        (col target_lang :type text)))

    ;; --- Prompt & Labeling ---
    (component prompt-db
      :pattern crud-gateway
      (table prompt_snapshots
        (col id :type integer :pk)
        (col task_id :type text)
        (col prompt :type text)
        (col cited_kb_ids :type jsonb)
        (col category :type text)
        (col task_outcome :type text))
      (table message_labels
        (col id :type integer :pk)
        (col message_id :type integer)
        (col label :type text)
        (col value :type text)
        (col source :type text)))

    ;; --- Backfill ---
    (component backfill-db
      :pattern crud-gateway
      (table backfill_progress
        (col phase :type text :pk)
        (col status :type text)
        (col last_cursor :type text)
        (col processed :type integer)
        (col failed :type integer))
      (table backfill_failures
        (col id :type integer :pk)
        (col session_id :type text)
        (col phase :type text)
        (col retry_count :type integer)
        (col last_error :type text))))

  ;; ============================================================
  ;; PILLAR 2: STATE MACHINES
  ;; Pattern: [P4] state-machine
  ;; ============================================================
  (pillar state-machines
    (purpose "all finite state automata governing lifecycle transitions")

    (component pty-state
      :pattern state-machine
      :target "missiond-core/src/semantic/types.rs"
      (state-machine pty-session-state
        :enum State
        :derive (Debug Clone Copy PartialEq Eq Serialize Deserialize)
        (states
          (Starting  :doc "PTY spawned, waiting for Claude Code TUI")
          (Idle      :doc "Waiting for user input, prompt visible")
          (SlashMenu :doc "Slash command menu open")
          (Thinking  :doc "Claude processing, spinner visible")
          (Responding :doc "Claude generating output")
          (ToolRunning :doc "Tool execution active")
          (Confirming :doc "Permission dialog pending")
          (Error     :doc "Error state"))
        (transitions
          (Starting -> Idle :trigger "prompt-detected")
          (Starting -> Error :trigger "process-crash")
          (Idle -> Thinking :trigger "spinner-detected")
          (Idle -> SlashMenu :trigger "slash-menu-detected")
          (Idle -> ToolRunning :trigger "tool-activity")
          (Idle -> Confirming :trigger "permission-dialog")
          (SlashMenu -> Idle :trigger "menu-dismissed")
          (Thinking -> Responding :trigger "output-begins")
          (Thinking -> ToolRunning :trigger "tool-hint")
          (Responding -> Idle :trigger "prompt-returns")
          (Responding -> ToolRunning :trigger "tool-invoked")
          (ToolRunning -> Idle :trigger "prompt-returns")
          (ToolRunning -> Confirming :trigger "permission-dialog")
          (Confirming -> ToolRunning :trigger "confirmed")
          (Confirming -> Idle :trigger "denied"))
        (trait SessionStateDetector
          (detect-from-screen :params ((lines "&[String]") (current "&State")) :returns "Option<State>"))))

    (component extraction-phase
      :pattern state-machine
      :target "missiond-daemon/src/engine/learning_engine/extraction.rs"
      (state-machine extraction-phase
        :enum ExtractionPhase
        (states
          (Idle     :doc "No extraction in progress")
          (Sending  :doc "Sending content to extraction slot")
          (WaitingForIdleness :doc "Waiting for slot to finish")
          (Complete :doc "Extraction round finished"))
        (transitions
          (Idle -> Sending :trigger "extraction-triggered")
          (Sending -> WaitingForIdleness :trigger "content-sent")
          (WaitingForIdleness -> Complete :trigger "slot-idle")
          (WaitingForIdleness -> Idle :trigger "timeout")
          (Complete -> Idle :trigger "reset"))))

    (component engineering-phase
      :pattern state-machine
      :target "missiond-core/src/types/board.rs"
      (state-machine engineering-phase
        :enum EngineeringPhase
        (states
          (Investigate :doc "Gathering context and requirements")
          (Consult     :doc "Consulting Gemini for architecture review")
          (Plan        :doc "Writing implementation plan")
          (Execute     :doc "Executing the plan")
          (Finalize    :doc "Verifying and closing"))
        (transitions
          (Investigate -> Consult :trigger "context-gathered")
          (Consult -> Plan :trigger "review-complete")
          (Plan -> Execute :trigger "plan-approved")
          (Execute -> Finalize :trigger "implementation-done")
          (Finalize -> Investigate :trigger "issues-found"))))

    (component task-status
      :pattern state-machine
      (state-machine task-status
        :enum TaskStatus
        (states (Queued) (Running) (Completed) (Failed))
        (transitions
          (Queued -> Running :trigger "slot-claimed")
          (Running -> Completed :trigger "result-received")
          (Running -> Failed :trigger "error-or-timeout"))))

    (component board-task-status
      :pattern state-machine
      (state-machine board-task-status
        :enum BoardTaskStatus
        (states (Open) (Running) (Done) (Failed) (Blocked))
        (transitions
          (Open -> Running :trigger "claimed")
          (Open -> Blocked :trigger "dependency-unmet")
          (Running -> Done :trigger "completed")
          (Running -> Failed :trigger "error")
          (Blocked -> Open :trigger "dependency-resolved"))))

    (component async-job-status
      :pattern state-machine
      (state-machine async-job-status
        :enum AsyncJobStatus
        (states (Queued) (Running) (Done) (Failed))
        (transitions
          (Queued -> Running :trigger "worker-picks-up")
          (Running -> Done :trigger "success")
          (Running -> Failed :trigger "error"))))

    (component question-status
      :pattern state-machine
      (state-machine question-status
        :enum AgentQuestionStatus
        (states (Pending) (Answered) (Dismissed))
        (transitions
          (Pending -> Answered :trigger "answer-provided")
          (Pending -> Dismissed :trigger "user-dismissed")))))

  ;; ============================================================
  ;; PILLAR 3: WORKERS
  ;; Pattern: [P2] event-listener + [P3] cron-worker
  ;; ============================================================
  (pillar workers
    (purpose "18 background workers with unified BackgroundWorker trait")

    ;; --- Event-Driven Workers [P2] ---
    (component conversation-logger
      :pattern event-listener
      :target "missiond-daemon/src/workers/conversation_logger.rs"
      :struct ConversationLogger
      (listens-to (JsonlMessageIngested :fields (session_id message)))
      (writes-to conversations conversation_messages))

    (component conversation-organizer
      :pattern event-listener
      :target "missiond-daemon/src/workers/conversation_organizer.rs"
      :struct ConversationOrganizer
      (listens-to (MessagePersisted :fields (session_id msg_id)))
      (writes-to message_labels conversation_events))

    (component pty-event-worker
      :pattern event-listener
      :target "missiond-daemon/src/workers/pty_event_worker.rs"
      :struct PtyEventWorker
      (listens-to (PtyStateChanged :fields (slot_id old_state new_state)))
      (emits (SlotBecameIdle) (SlotStuck)))

    (component tagger-chunker
      :pattern event-listener
      :target "missiond-daemon/src/workers/tagger_chunker.rs"
      :struct TaggerChunker
      (listens-to (MessagePersisted :fields (session_id msg_id)))
      (writes-to message_labels))

    (component step-narrator
      :pattern event-listener
      :target "missiond-daemon/src/workers/step_narrator.rs"
      :struct StepNarrator
      (listens-to (MessagePersisted :fields (session_id msg_id)))
      (writes-to message_narrations narration_cursors))

    ;; --- Cron/Tick Workers [P3] ---
    (component embedding-worker
      :pattern cron-worker
      :target "missiond-daemon/src/workers/embedding_worker.rs"
      :struct EmbeddingWorker
      :deps ((db "DbExecutor") (embed_model "FastEmbed") (sonnet "SonnetGateway"))
      (schedule :channel-driven "embedding_rx MPSC")
      (writes-to knowledge conversation_topic_vectors))

    (component translation-worker
      :pattern cron-worker
      :target "missiond-daemon/src/workers/translation_worker.rs"
      :struct TranslationWorker
      :deps ((db "DbExecutor") (sonnet "SonnetGateway"))
      (schedule :interval 5s)
      (writes-to message_translations))

    (component briefing-worker
      :pattern cron-worker
      :target "missiond-daemon/src/workers/briefing_worker.rs"
      :struct BriefingWorker
      :deps ((db "DbExecutor") (sonnet "SonnetGateway"))
      (schedule :on-demand)
      (writes-to slot context briefings))

    (component vision-worker
      :pattern cron-worker
      :target "missiond-daemon/src/workers/vision_worker.rs"
      :struct VisionWorker
      (schedule :channel-driven)
      (writes-to image_descriptions))

    (component experience-harvester
      :pattern cron-worker
      :target "missiond-daemon/src/workers/experience_harvester.rs"
      :struct ExperienceHarvester
      (schedule :interval 60s)
      (writes-to knowledge))

    (component reconcile-worker
      :pattern cron-worker
      :target "missiond-daemon/src/workers/reconcile_worker.rs"
      :struct ReconcileWorker
      (schedule :interval 10s)
      (writes-to slot_sessions))

    (component gemini-reconcile-worker
      :pattern cron-worker
      :target "missiond-daemon/src/workers/gemini_reconcile_worker.rs"
      :struct GeminiReconcileWorker
      (schedule :interval 10s))

    (component ast-sync-worker
      :pattern cron-worker
      :target "missiond-daemon/src/workers/ast_sync_worker.rs"
      :struct AstSyncWorker
      :deps ((db "DbExecutor") (treesitter "tree-sitter"))
      (schedule :channel-driven "ast_sync_rx MPSC")
      (writes-to ast_nodes ast_file_meta))

    (component code-prefetch
      :pattern cron-worker
      :target "missiond-daemon/src/workers/code_prefetch.rs"
      :struct CodePrefetch
      (schedule :on-demand))

    (component strategy-worker
      :pattern cron-worker
      :target "missiond-daemon/src/workers/strategy_worker.rs"
      :struct StrategyWorker
      :deps ((gemini "GeminiGateway"))
      (schedule :interval 300s :flag-gated true)
      (reads-from board_tasks knowledge conversations))

    (component retro-worker
      :pattern cron-worker
      :target "missiond-daemon/src/workers/retro_worker.rs"
      :struct RetroWorker
      (schedule :on-session-end)
      (writes-to retrospective_results))

    (component arch-maintenance-worker
      :pattern cron-worker
      :target "missiond-daemon/src/workers/arch_maintenance_worker.rs"
      :struct ArchMaintenanceWorker
      (schedule :interval 3600s)
      (writes-to knowledge))

    (component gemini-logger
      :pattern cron-worker
      :target "missiond-daemon/src/workers/gemini_logger.rs"
      :struct GeminiLogger
      (schedule :channel-driven)
      (writes-to gemini_requests)))

  ;; ============================================================
  ;; PILLAR 4: MCP TOOLS
  ;; Pattern: [P5] mcp-tool
  ;; ============================================================
  (pillar mcp-tools
    (purpose "60+ MCP tools across 4 domains, JSON-RPC dispatch")

    ;; --- Compute Domain ---
    (mcp-module compute-pty
      :target "missiond-daemon/src/handlers/compute/pty.rs"
      (tool mission_pty_spawn :description "Spawn new PTY session"
        (input (slot_id string :required) (command string) (args array) (cwd string)))
      (tool mission_pty_send :description "Send input to PTY"
        (input (slot_id string :required) (content string :required)))
      (tool mission_pty_read :description "Read PTY output"
        (input (slot_id string :required) (lines integer)))
      (tool mission_pty_screenshot :description "Capture PTY screenshot"
        (input (slot_id string :required)))
      (tool mission_pty_status :description "Get PTY state"
        (input (slot_id string :required)))
      (tool mission_pty_confirm :description "Respond to permission dialog"
        (input (slot_id string :required) (allow boolean :required)))
      (tool mission_pty_signal :description "Send signal to PTY"
        (input (slot_id string :required) (signal string :required))))

    (mcp-module compute-task
      :target "missiond-daemon/src/handlers/compute/task.rs"
      (tool mission_task_submit :description "Submit async task"
        (input (prompt string :required) (slot_id string) (priority string)))
      (tool mission_task_query :description "Query task status"
        (input (task_id string :required)))
      (tool mission_task_cancel :description "Cancel running task"
        (input (task_id string :required)))
      (tool mission_task_delegate :description "Delegate task to slot"
        (input (slot_id string :required) (objective string :required) (context object))))

    (mcp-module compute-slot
      :target "missiond-daemon/src/handlers/compute/slot.rs"
      (tool mission_slots :description "List slot configurations")
      (tool mission_slot_history :description "Slot task history"
        (input (slot_id string :required) (limit integer)))
      (tool mission_compute_slot :description "Dynamic compute slot"
        (input (action string :required :enum (create status destroy))
               (template string) (objective string) (ttl integer))))

    (mcp-module compute-cc
      :target "missiond-daemon/src/handlers/compute/cc_tasks.rs"
      (tool mission_cc_query :description "Query Claude Code tasks"
        (input (slot_id string) (status string) (limit integer)))
      (tool mission_cc_swarm :description "Multi-slot swarm execution"
        (input (objective string :required) (slot_ids array))))

    (mcp-module compute-worker
      :target "missiond-daemon/src/handlers/compute/worker.rs"
      (tool mission_worker :description "Worker status and control"
        (input (action string :required :enum (list status pause resume restart))
               (worker_name string))))

    (mcp-module compute-job
      :target "missiond-daemon/src/handlers/compute/job.rs"
      (tool mission_job_poll :description "Poll async job result"
        (input (job_id string :required))))

    ;; --- Knowledge Domain ---
    (mcp-module knowledge-kb
      :target "missiond-daemon/src/handlers/knowledge/kb.rs"
      (tool mission_kb_query :description "Search KB entries"
        (input (query string :required) (category string) (limit integer :default 10)))
      (tool mission_kb_remember :description "Upsert KB entry"
        (input (category string :required) (key string :required)
               (summary string :required) (detail object) (confidence number)))
      (tool mission_kb_mutate :description "KB mutation operations"
        (input (action string :required :enum (forget merge consolidate))
               (key string) (keys array)))
      (tool mission_kb_ops :description "KB maintenance"
        (input (action string :required :enum (stats export import reindex)))))

    (mcp-module knowledge-memory
      :target "missiond-daemon/src/handlers/knowledge/memory.rs"
      (tool mission_memory :description "Memory extraction pipeline control"
        (input (action string :required :enum (status trigger pause)))))

    (mcp-module knowledge-board
      :target "missiond-daemon/src/handlers/knowledge/board.rs"
      (tool mission_board_query :description "Query board tasks"
        (input (action string :required :enum (list get)) (id string) (status string)))
      (tool mission_board_create :description "Create board task"
        (input (title string :required) (description string) (priority string)
               (category string) (assignee string) (parent_id string)))
      (tool mission_board_update :description "Update board task"
        (input (id string :required) (status string) (title string) (priority string)))
      (tool mission_board_delete :description "Delete board task"
        (input (id string :required)))
      (tool mission_board_claim :description "Claim board task for execution"
        (input (id string :required)))
      (tool mission_board_decompose :description "Decompose task into subtasks"
        (input (id string :required)))
      (tool mission_board_note_add :description "Add note to board task"
        (input (task_id string :required) (content string :required) (note_type string)))
      (tool mission_board_retry :description "Retry failed board task"
        (input (id string :required))))

    (mcp-module knowledge-skill
      :target "missiond-daemon/src/handlers/knowledge/skill.rs"
      (tool mission_skill_query :description "Search skills"
        (input (query string) (topic string)))
      (tool mission_skill_exec :description "Execute skill workflow"
        (input (topic string :required) (action string)))
      (tool mission_skill_mutate :description "Create/update skill"
        (input (action string :required) (topic string :required) (content string)))
      (tool mission_skill_context :description "Build skill context"
        (input (topic string :required))))

    (mcp-module knowledge-insight
      :target "missiond-daemon/src/handlers/knowledge/insight.rs"
      (tool mission_insight :description "Strategic insight analysis"
        (input (query string :required) (depth string :enum (quick detailed)))))

    ;; --- Communication Domain ---
    (mcp-module comm-conversation
      :target "missiond-daemon/src/handlers/comm/conversation.rs"
      (tool mission_conversation_query :description "List conversations"
        (input (slot_id string) (status string) (limit integer)))
      (tool mission_conversation_analyze :description "Analyze conversation"
        (input (session_id string :required)))
      (tool mission_conversation_reconcile :description "Reconcile conversation state"
        (input (session_id string :required))))

    (mcp-module comm-question
      :target "missiond-daemon/src/handlers/comm/question.rs"
      (tool mission_question :description "Manage agent questions"
        (input (action string :required :enum (create list answer dismiss))
               (question string) (id string) (answer string))))

    (mcp-module comm-router-chat
      :target "missiond-daemon/src/handlers/comm/router_chat.rs"
      (tool mission_router_chat :description "Chat with Gemini via router"
        (input (message string :required) (session_id string)
               (files array) (thinking boolean)))
      (tool mission_router_chat_manage :description "Manage router chat sessions"
        (input (action string :required :enum (list get archive delete))
               (session_id string))))

    (mcp-module comm-timeline
      :target "missiond-daemon/src/handlers/comm/timeline.rs"
      (tool mission_timeline :description "Query system timeline"
        (input (event_type string) (trace_id string) (since string) (limit integer))))

    (mcp-module comm-audit
      :target "missiond-daemon/src/handlers/comm/audit.rs"
      (tool mission_audit :description "Audit trail query"
        (input (action string :required :enum (trace detail stats export))
               (trace_id string) (tool_name string))))

    (mcp-module comm-retrospective
      :target "missiond-daemon/src/handlers/comm/retrospective.rs"
      (tool mission_retrospective_manage :description "Session retrospective"
        (input (session_id string :required) (depth string :enum (quick detailed full)))))

    ;; --- System/Infra Domain ---
    (mcp-module sysinfra
      :target "missiond-daemon/src/handlers/sysinfra/"
      (tool mission_infra_query :description "Query infrastructure"
        (input (action string :required :enum (list get discover))))
      (tool mission_infra_ops :description "Infrastructure operations"
        (input (action string :required) (server_id string)))
      (tool mission_permission_query :description "Query permissions"
        (input (slot_id string) (tool_name string)))
      (tool mission_permission_mutate :description "Update permissions"
        (input (action string :required) (slot_id string) (rules object)))
      (tool mission_sys_config :description "System configuration"
        (input (action string :required :enum (get set list))))
      (tool mission_sys_logs :description "System logs"
        (input (lines integer) (level string)))
      (tool mission_power_control :description "Physical server power"
        (input (server_id string :required) (action string :required)))
      (tool mission_control :description "Master control interface"
        (input (action string :required)))
      (tool mission_pause :description "Pause/resume subsystems"
        (input (target string :required) (action string :required :enum (pause resume))))))

  ;; ============================================================
  ;; PILLAR 5: BOOTSTRAP / DI WIRING
  ;; Pattern: [P6] bootstrap
  ;; ============================================================
  (pillar bootstrap
    (purpose "daemon initialization — topological dependency injection")

    (component daemon-init
      :pattern bootstrap
      :target "missiond-daemon/src/main.rs"

      (infra
        (component db :type "DbExecutor"
          :init "DbExecutor::new(&config).await?"
          :post-init "db.migrate().await?")
        (component embed_model :type "FastEmbed"
          :init "FastEmbed::new()?")
        (component event_bus :type "EventBus"
          :init "EventBus::new(512)")
        (component pty_manager :type "PTYManager" :wrap "Arc::new"
          :deps (event_bus))
        (component slot_manager :type "SlotManager" :wrap "Arc::new"
          :deps (db pty_manager event_bus)
          :post-init "slot_manager.load_config().await?")
        (component mission_control :type "MissionControl" :wrap "Arc::new"
          :deps (db slot_manager event_bus))
        (component gemini_gateway :type "GeminiGateway"
          :deps (db))
        (component sonnet_gateway :type "SonnetGateway"
          :deps (slot_manager))
        (component llm_gateway :type "LlmGateway"
          :deps (gemini_gateway sonnet_gateway))
        (component context_pipeline :type "ContextPipeline"
          :deps (db slot_manager))
        (component worker_registry :type "WorkerRegistry"
          :deps (event_bus))
        (component control_tree :type "ControlTree"
          :deps (worker_registry)))

      (workers
        (spawn conversation-logger    :deps (db event_bus))
        (spawn conversation-organizer :deps (db event_bus))
        (spawn embedding-worker       :deps (db embed_model sonnet_gateway))
        (spawn translation-worker     :deps (db sonnet_gateway))
        (spawn briefing-worker        :deps (db sonnet_gateway))
        (spawn vision-worker          :deps (db))
        (spawn experience-harvester   :deps (db event_bus))
        (spawn pty-event-worker       :deps (event_bus slot_manager))
        (spawn reconcile-worker       :deps (db slot_manager))
        (spawn gemini-reconcile-worker :deps (db pty_manager))
        (spawn ast-sync-worker        :deps (db))
        (spawn code-prefetch          :deps (db))
        (spawn step-narrator          :deps (db sonnet_gateway))
        (spawn strategy-worker        :deps (db gemini_gateway) :flag "strategy_enabled")
        (spawn retro-worker           :deps (db sonnet_gateway event_bus))
        (spawn tagger-chunker         :deps (db event_bus))
        (spawn arch-maintenance-worker :deps (db))
        (spawn gemini-logger          :deps (db)))

      (engines
        (spawn autopilot :deps (db slot_manager event_bus llm_gateway context_pipeline))
        (spawn ipc-handler :deps (mission_control))
        (spawn ws-server :deps (event_bus pty_manager)))))

  ;; ============================================================
  ;; PILLAR 6: COMPONENTS WITHOUT EXISTING PATTERNS
  ;; Pattern: [GAP] — needs new mold design
  ;; ============================================================
  (pillar uncovered-components
    (purpose "components that do NOT fit any existing Forge pattern")

    ;; --- GAP 1: LLM Gateway ---
    (component llm-gateway
      :pattern-gap "llm-gateway"
      :certainty 0
      :target "missiond-daemon/src/llm/"
      :files (gemini_client.rs gemini_gateway.rs gemini_cli.rs
              sonnet_gateway.rs minimax_client.rs minimax_gateway.rs
              llm_gateway.rs llm_gate.rs gemini_driver.rs
              gemini_file_api.rs gemini_pty.rs prompts.rs)
      :description "Queue-driven LLM dispatch with priority channels,
        rate limiting, backpressure, retry, and multi-provider routing.
        Not CRUD, not Worker, not StateMachine — it is a gateway pattern
        with channel-based priority queues (interactive/embedding/translation/briefing)."
      :sub-patterns
        ((queue-channel   "MPSC-based priority queue per use-case")
         (rate-limiter    "429 backoff, quota tracking")
         (provider-router "Gemini HTTP / Gemini CLI / Sonnet slot / MiniMax HTTP")
         (prompt-builder  "System prompt assembly from templates")))

    ;; --- GAP 2: Slot Orchestrator ---
    (component slot-orchestrator
      :pattern-gap "slot-orchestrator"
      :certainty 0
      :target "missiond-daemon/src/slot_orchestrator/"
      :files (agent.rs cc_controller.rs gemini_controller.rs spawner.rs types.rs)
      :description "Process lifecycle controller for multi-engine AI sessions.
        Combines state machine + PTY management + health monitoring + restart.
        More than a Worker or StateMachine — it is a supervisor pattern
        managing heterogeneous child processes (Claude/Gemini/Codex)."
      :sub-patterns
        ((process-lifecycle "spawn/monitor/restart/kill agent processes")
         (engine-adapter   "per-engine controller interface (CC/Gemini/Codex)")
         (context-monitor  "track context window usage, trigger compaction/restart")
         (task-dispatch    "route tasks to available slots")))

    ;; --- GAP 3: Autopilot / Tick Engine ---
    (component autopilot-engine
      :pattern-gap "tick-engine"
      :certainty 0
      :target "missiond-daemon/src/engine/intent_engine/"
      :files (autopilot.rs flow_engine.rs memory_scheduler.rs workflow_executor.rs)
      :description "Composite orchestrator tick loop that sequences multiple
        sub-engines (memory→extraction→task→decision→flow→supervision).
        More complex than cron-worker — it is a pipeline-of-engines pattern
        where each tick runs a chain of sub-ticks in order."
      :sub-patterns
        ((tick-pipeline   "ordered sequence of sub-engine ticks")
         (flow-lifecycle  "Board task progression through EngineeringPhase")
         (memory-trigger  "condition-based memory extraction scheduling")
         (workflow-exec   "skill-driven workflow step execution")))

    ;; --- GAP 4: Learning Engine ---
    (component learning-engine
      :pattern-gap "learning-engine"
      :certainty 0
      :target "missiond-daemon/src/engine/learning_engine/"
      :files (decision_engine.rs extraction.rs decision_harvest.rs
              intent_analyst.rs timeline_analyst.rs idle_explorer.rs
              historical_scanner.rs)
      :description "Multi-strategy decision routing and knowledge extraction.
        Routes questions through KB→LLM→human cascade. Extracts patterns,
        decisions, and intent from conversation history. Not a simple worker —
        it is a decision-cascade + extraction-pipeline composite."
      :sub-patterns
        ((decision-cascade  "KB lookup → Gemini → decision slot → human")
         (extraction-fsm    "ExtractionPhase state machine for memory harvest")
         (intent-analysis   "extract user intent from conversation turns")
         (pattern-mining    "historical session analysis for recurring patterns")))

    ;; --- GAP 5: Context Pipeline ---
    (component context-pipeline
      :pattern-gap "context-pipeline"
      :certainty 0
      :target "missiond-daemon/src/context/"
      :files (context_pipeline.rs context_budget.rs slot_env.rs
              claude_md_sync.rs topology_map.rs)
      :description "LLM prompt builder with budget constraints.
        Assembles context from KB, skills, history, and topology within
        a token budget. Not CRUD, not Worker — it is a builder/pipeline
        with prioritized source assembly and truncation rules."
      :sub-patterns
        ((budget-allocator  "token budget partitioning across context sources")
         (source-ranker     "prioritize KB/skill/history by relevance")
         (claude-md-sync    "sync preferences to ~/.claude/CLAUDE.md")
         (topology-infer    "infer cross-slot relationships")))

    ;; --- GAP 6: Semantic Parser ---
    (component semantic-parser
      :pattern-gap "semantic-parser"
      :certainty 0
      :target "missiond-core/src/semantic/"
      :files (types.rs state.rs confirm.rs tool.rs
              fingerprint.rs patterns.rs gemini_state.rs)
      :description "Terminal output pattern matching and state inference.
        Parses raw PTY screen lines into structured states using regex
        fingerprints. Not a state-machine (it FEEDS state machines).
        It is a parser/recognizer pattern."
      :sub-patterns
        ((screen-parser    "line-by-line terminal output analysis")
         (fingerprint-db   "regex pattern database for state detection")
         (confirm-parser   "permission dialog structure extraction")
         (tool-recognizer  "tool invocation output parsing")
         (multi-engine     "per-engine parser variants (Claude/Gemini)")))

    ;; --- GAP 7: Event Bus / Timeline ---
    (component event-infrastructure
      :pattern-gap "event-bus"
      :certainty 0
      :target "missiond-daemon/src/"
      :files (event_bus.rs event_router.rs events_sync.rs)
      :description "Publish-subscribe event infrastructure with persistent
        timeline. More than a broadcast channel — includes event routing,
        persistence to system_timeline table, and multi-consumer fan-out."
      :sub-patterns
        ((broadcast-hub    "tokio broadcast channel with DaemonEvent enum")
         (event-router     "route events to registered handlers")
         (timeline-writer  "persist events to system_timeline with FTS")
         (frontend-bridge  "relay events to WebSocket for UI")))

    ;; --- GAP 8: Worker Registry / Control Tree ---
    (component worker-lifecycle
      :pattern-gap "worker-registry"
      :certainty 0
      :target "missiond-daemon/src/workers/"
      :files (registry.rs ../control_tree.rs)
      :description "Supervisor pattern for worker lifecycle management.
        BackgroundWorker trait + registry + hierarchical pause/resume.
        Not a single worker — it is the META-pattern that manages workers."
      :sub-patterns
        ((trait-contract    "BackgroundWorker trait: name/start/stop/status")
         (registry          "track all workers, health checks")
         (control-tree      "hierarchical pause/resume: provider→worker→sub")
         (graceful-shutdown "ordered shutdown with drain")))

    ;; --- GAP 9: IPC / MCP Protocol ---
    (component ipc-protocol
      :pattern-gap "ipc-protocol"
      :certainty 0
      :target "missiond-mcp/src/"
      :files (server.rs protocol.rs)
      :description "JSON-RPC 2.0 over stdio transport layer.
        The mcp-tool pattern covers tool DEFINITIONS but not the
        protocol layer that dispatches JSON-RPC requests to handlers."
      :sub-patterns
        ((jsonrpc-server   "JSON-RPC 2.0 request/response/notification")
         (stdio-transport  "stdin/stdout message framing")
         (tool-dispatch    "route tool_name → handler function")
         (ipc-bridge       "Unix socket / TCP bridge to daemon")))

    ;; --- GAP 10: WebSocket / Frontend Bridge ---
    (component ws-bridge
      :pattern-gap "ws-bridge"
      :certainty 0
      :target "missiond-core/src/ws/"
      :files (server.rs screenshot_broker.rs jarvis_trace.rs)
      :description "WebSocket server for realtime frontend communication.
        Distributes PTY screenshots, trace events, and state updates
        to connected UI clients. A distinct transport/broadcast pattern."
      :sub-patterns
        ((ws-server         "tokio-tungstenite WebSocket acceptor")
         (screenshot-broker "distribute PTY frames to subscribers")
         (trace-relay       "forward request traces to UI"))))
)
