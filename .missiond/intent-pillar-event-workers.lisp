;; MissionD — Pillar: event-workers
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

  (pillar event-workers
    (purpose "pub-sub event backbone + 21 background workers organized by LLM dependency: sonnet(6) codex(2) gemini(1) local(12)")
    ;; local count: +codex_ingestion_worker (ec269d7); EventAnalyzerWorker(65c8b59) absorbed into tagger-chunker(1ea1838); +xjpcode_briefing_worker (8e5efaf)

    (component event-bus
      :target "crates/missiond-daemon/src/event_bus.rs"
      (description "tokio broadcast channel hub for DaemonEvent enum")
      (event-categories
        (pty-events       "PtyStateChanged, PtyOutput, PtyScreenshot")
        (message-events   "JsonlMessageIngested, MessagePersisted")
        (task-events      "TaskSubmitted, TaskCompleted, TaskFailed")
        (board-events     "BoardTaskCreated, BoardTaskUpdated, BoardTaskClaimed")
        (slot-events      "SlotBecameIdle, SlotStuck, SlotSessionChanged")
        (knowledge-events "KbEntryCreated, KbEntryUpdated")
        (system-events    "WorkerStatusChanged, ShutdownRequested")
        (cascade-events   "CascadeTriggered, CascadeCompleted")
        ;; commit 65c8b59: ContextualCommitDetected 替换旧的 CommitDetected 事件
        ;; 携带 conversation_id/session_id/slot_id — 桥接"哪个 commit" 与"哪个会话产生的"
        (commit-events    "ContextualCommitDetected{conversation_id, session_id, slot_id, commit_hash, message}")))

    (component event-router
      :target "crates/missiond-daemon/src/event_router.rs"
      (depends event-bus)
      ;; commit c8b76b0: demoted to Notify signal emitter — no longer spawns orphan coroutines
      ;; strategy/retro/ast-sync moved to BackgroundWorker trait; event_router is now a thin broadcaster
      (note "signal emitter only — use BackgroundWorker trait for all event-driven processing"))

    (component events-sync
      :target "crates/missiond-daemon/src/events_sync.rs"
      (depends event-bus)
      (writes-to system_timeline))

    (component worker-registry
      :target "crates/missiond-daemon/src/workers/registry.rs"
      (trait BackgroundWorker
        (const KIND :type WorkerKind :enum (Sonnet Codex Gemini Local))
        (methods (name extra-deps run)))
      (note "KIND must match the subdirectory the worker lives in; ControlTree provider deps auto-injected by spawn_worker")
      ;; commit c8b76b0: strategy-worker(Gemini), retro-worker(Sonnet), ast-sync-worker(Local)
      ;; converted from loose async fns → BackgroundWorker implementations with ControlTree pause/resume.
      ;; Closes ControlTree enforcement gap — all event-driven workers now respect cascade pause/resume.
      )

    (component control-tree
      :target "crates/missiond-daemon/src/control_tree.rs"
      (depends worker-registry)

      (struct ControlTree
        (field global_paused :type bool)
        (field providers     :type "HashMap<CtlProvider, bool>")
        (field domains       :type "HashMap<CtlDomain, bool>")
        (field workers       :type "HashMap<String, bool>"
          :note "true=force-paused  false=force-resumed(debug override)  absent=follow cascade")
        (field slot_roles    :type "HashMap<String, bool>")
        (field projects      :type "HashMap<String, bool>"
          :note "project_id → paused; added P2+P3 (commit 50a5296). is_project_paused() = projects.get(id).copied().unwrap_or(false)")
        (field domain_paused_at :type "HashMap<CtlDomain, i64>" :note "informational only"))

      (cascade-priority
        :note "is_effectively_paused() evaluation order"
        (1 worker-explicit-override
          :semantics "workers[name]=true  → always paused; workers[name]=false → always resumed (debug mode)")
        (2 global-kill-switch
          :semantics "global_paused=true  → all workers paused unless worker override present")
        (3 provider-domain-cascade
          :semantics "each Dependency::Provider / Dependency::Domain checked; any true → paused"))
      ;; project pause: is_project_paused(id) is checked independently by handlers,
      ;; NOT part of is_effectively_paused() worker cascade — project gates data flow, not workers

      (method is_worker_paused
        :semantics "worker explicit override beats global; absent → follow global_paused")
      (method is_effectively_paused
        :args (worker_name "deps: &[Dependency]")
        :semantics "full cascade: worker override > global > provider/domain deps")
      (method status_summary
        :returns serde_json::Value
        :fields (global_paused providers domains workers_paused workers_force_resumed slot_roles)
        :note "workers split into paused/force_resumed lists for debug visibility")

      (struct ControlManager
        :pattern "push-based watch broadcast (NOT polling)"
        :transport "tokio::watch::channel<ControlTree>"
        :semantics "mutations → atomic update via send_modify() → all subscribers notified via changed().await"
        :persistence "control_tree.json — crash recovery via spawn_blocking write"
        :note "Workers await watch::Receiver::changed() in select! — zero-cost async push, not HashMap polling"
        (mutations (set_global_paused set_provider set_domain set_worker set_slot_role set_project))
        ;; set_project: paused=true → insert project_id; paused=false → remove (no false entry stored)
        ))

    ;; workers/sonnet/ — Sonnet API via SonnetGateway
    (component workers-sonnet
      (worker embedding-worker
        :target "crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs"
        :trigger "channel embedding_rx MPSC"
        :writes-to (knowledge conversation_topic_vectors))
      (worker translation-worker
        :target "crates/missiond-daemon/src/workers/sonnet/translation_worker.rs"
        :trigger "interval 5s"
        :writes-to translations)
      (worker briefing-worker
        :target "crates/missiond-daemon/src/workers/sonnet/briefing_worker.rs"
        :trigger on-demand)
      (worker arch-maintenance-worker
        :target "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
        :trigger "ContextualCommitDetected event (commit 65c8b59: was interval 3600s polling)"
        :writes-to knowledge
        :note "commit 65c8b59: now consumes ContextualCommitDetected (richer event with conversation context) instead of polling git-log")
      (worker retro-worker
        :target "crates/missiond-daemon/src/workers/sonnet/retro_worker.rs"
        :trigger on-session-end
        :writes-to retrospective_results
        :note "commit c8b76b0: now BackgroundWorker impl, respects ControlTree pause/resume")
      ;; commit 79a877f: NEW
      (worker lisp-survey-worker
        :target "crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs"
        :added "79a877f"
        :trigger "ContextualCommitDetected event"
        :routes-to "lisp-surveyor (persistent Sonnet slot, timeout=900s)"
        :writes-to "project intent.lisp (via slot Edit tool)"
        :debounce "60s per project_id"
        :filters "self-trigger: skips commits where slot_id == lisp-surveyor"
        :diff-truncation "max 8000 chars"
        :flow ("ContextualCommitDetected{project_id, slot_id, commit_hash, diff}"
               "→ self-trigger filter (slot_id == lisp-surveyor → skip)"
               "→ ProjectRegistry::resolve(project_id) → intent_path lookup"
               "→ skip if no intent.lisp configured for project"
               "→ debounce 60s (HashMap<String, Instant>)"
               "→ build incremental survey prompt (diff + intent.lisp path)"
               "→ slot_manager.execute(\"lisp_survey\", prompt)"
               "→ parse response: NO_CHANGE → skip; otherwise intent.lisp updated by slot")))

    ;; workers/codex/ — Codex CLI / Claude Code PTY via SlotManager
    (component workers-codex
      (worker step-narrator
        :target "crates/missiond-daemon/src/workers/codex/step_narrator.rs"
        :listens-to MessagePersisted
        :writes-to (message_narrations narration_cursors))
      (worker vision-worker
        :target "crates/missiond-daemon/src/workers/codex/vision_worker.rs"
        :trigger channel
        :writes-to image_descriptions))

    ;; workers/gemini/ — Gemini CLI PTY via SlotManager
    (component workers-gemini
      (worker strategy-worker
        :target "crates/missiond-daemon/src/workers/gemini/strategy_worker.rs"
        :trigger "interval 300s flag-gated"
        :note "commit c8b76b0: now BackgroundWorker impl, respects ControlTree pause/resume"))

    ;; workers/local/ — pure computation, no LLM dependency
    ;; count: 12 workers (was 11; +xjpcode_briefing_worker 8e5efaf; prev: +codex_ingestion_worker ec269d7; EventAnalyzerWorker absorbed into tagger-chunker 1ea1838)
    (component workers-local
      (worker conversation-logger
        :target "crates/missiond-daemon/src/workers/local/conversation_logger.rs"
        :listens-to JsonlMessageIngested
        :writes-to (conversations conversation_messages))
      (worker conversation-organizer
        :target "crates/missiond-daemon/src/workers/local/conversation_organizer.rs"
        :listens-to ConversationMessageLogged
        :emits SessionOrganized
        ;; commit ad889d1: 去除 agent-only 过滤 — 原仅处理 agent-* session，用户前台 session 被忽略
        ;; 修复后：所有 session_id 均进 dirty 集合，统一在 5s debounce 后 emit SessionOrganized
        ;; P0 compaction link 和 P1 orphan parent fix 仍仅对 agent-* 执行，非 agent session 跳过
        ;; commit 43c80f4: broadcast lagged warning 语义明确为 "events permanently lost"
        ;;   (TaggerChunker 的 reconcile sweep 可在 60s 内补处理这些丢失的 session)
        )
      (worker pty-event-worker
        :target "crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
        :listens-to PtyStateChanged
        :emits (SlotBecameIdle SlotStuck)
        ;; commit ec269d7: auto-approve confirm dialogs containing "don't ask again"/"always"/"trust"/"不再"
        ;; sends ConfirmResponse::Option(2) as two sequential PTY writes (digit + Enter, 80ms apart)
        ;; unicode curly apostrophe (U+2019) normalized so ' matches like ASCII '
        )
      (worker tagger-chunker
        :target "crates/missiond-daemon/src/workers/local/tagger_chunker.rs"
        :listens-to SessionOrganized
        ;; commit 21a3b63: TurnBuilder extracts files_read/files_changed/outcome from tool calls
        ;; build_turn_embedding_text: keyword-dense format (project:X files:Y intent:Z outcome:W)
        ;; filters <task-notification> noise turns; triggers per-turn embedding after backfill (dc45dcb)
        ;; commit 1ea1838: commit detection logic moved here from EventAnalyzerWorker
        ;; commit 081b4f9: parse actual command from [Tool: Bash] content format
        ;; commit ce3f3ec: commit detection 移到 early-return 之前，活跃 session 不再丢失 commit 事件
        ;; commit d8f47de: process_session 拆分为两独立流水线:
        ;;   - analyze_messages(stage2): noise-labels + tool分类 + commit detection — 必然执行
        ;;   - chunk_turns(stage3): turn提取/tail-trim/持久化 — 活跃session可返回0无副作用
        ;; commit 9684b50: analyze_messages 新增 tracing — user_msg count + regex MATCH 逐条日志
        ;; commit 43c80f4: 新增 reconcile_tick(60s interval) + reconcile_missed_sessions()
        ;;   - 调用 sessions_recently_active_without_turns(since_minutes=5, limit=50) 查漏补处理
        ;;   - 修复 broadcast Lagged 场景导致 SessionOrganized 永久丢失时的 session 漏处理
        ;;   - broadcast lagged → warn! "events permanently lost"
        ;;   - embedding_tx 发送失败 → warn! (非 panic，graceful degradation logging)
        )
      ;; EventAnalyzerWorker (65c8b59) was added then absorbed into tagger-chunker (1ea1838)
      ;; Commit detection is now done at schema-on-write stage in tagger_chunker with full message content
      (worker experience-harvester
        :target "crates/missiond-daemon/src/workers/local/experience_harvester.rs"
        :trigger "interval 60s"
        :writes-to knowledge)
      (worker reconcile-worker
        :target "crates/missiond-daemon/src/workers/local/reconcile_worker.rs"
        :trigger "interval 10s"
        :writes-to slot_sessions
        :note "commit 0adbb18: ensure_conversation_exists 现传入 jsonl 中首条消息时间戳作为 started_at")
      (worker gemini-reconcile-worker
        :target "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
        :trigger "interval 10s"
        :note "commit 0adbb18: ensure_gemini_conversation 现传入首条消息时间戳(Utc::now()→real timestamp)")
      (worker ast-sync-worker
        :target "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
        :trigger "channel ast_sync_rx MPSC"
        :writes-to (ast_nodes)
        :note "commit c8b76b0: now BackgroundWorker impl, respects ControlTree pause/resume")
      (worker code-prefetch
        :target "crates/missiond-daemon/src/workers/local/code_prefetch.rs"
        :trigger on-demand)
      (worker codex-ingestion-worker
        :target "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"
        :added "ec269d7"
        :trigger "polling ~/.codex/state_5.sqlite via rusqlite"
        :writes-to "conversation timeline (system_timeline) + conversation_messages (text messages)"
        :note "reads Codex CLI operation history from local SQLite DB; ingests operation logs into MissionD conversation timeline. rusqlite added to missiond-daemon Cargo.toml. spawned in main.rs at daemon boot."
        ;; commit 43c80f4: parse_jsonl_tool_calls → parse_jsonl，新增 ParsedMessage struct
        ;;   - 同时提取 tool calls + text messages (agent assistant + user 消息)
        ;;   - insert_conversation_messages_batch 写入 conversation_messages 表
        ;;   - poll interval 30s → 10s (POLL_INTERVAL_SECS=10)
        )
      (worker gemini-logger
        :target "crates/missiond-daemon/src/workers/local/gemini_logger.rs"
        :trigger channel
        :writes-to gemini_requests)
      ;; commit 8e5efaf: NEW — xjpcode neural link briefing
      (worker xjpcode-briefing-worker
        :target "crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs"
        :added "8e5efaf"
        :trigger "interval 60s (15s initial delay, MissedTickBehavior::Skip)"
        :writes-to "~/.xjpcode/xjpcode.md (filesystem, atomic std::fs::write)"
        :sources "list_board_tasks(running+failed) + list_incidents(5) + list_projects()"
        :pauses "ControlTree ctx.wait_if_paused()"
        :doc "generates system briefing markdown for xjpcode CLI; xjpcode reads at startup → system prompt context. Part of xjpcode↔missiond neural link.")))

