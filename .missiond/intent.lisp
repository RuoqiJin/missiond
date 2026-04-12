;; ══════════════════════════════════════════════════════
;; MissionD — Implementation Detail (Full System)
;; Generated: 2026-04-07 | Forge Deep Cartography v3
;; Updated: 2026-04-12 (79a877f)
;; ══════════════════════════════════════════════════════

(intent missiond
  (granularity L3-Implementation)
  (survey-hash "e17db52-50a5296-76900d1-e18d0bf-5671c95-ac96b3c-eae9bbd-c15f363-5ec3517-84ac1a6-0adbb18-3c10d21-dc45dcb-21a3b63-65c8b59-1ea1838-081b4f9-a9517ec-20813d5-5a5f805-ec269d7-79a877f")
  (survey-date "2026-04-12T00:00:00Z")
  (last-updated "2026-04-12 — 11 new commits (incremental Apr 9-12 +79a877f): [dc45dcb] feat: conversation_turns 新增 skeleton 列(JSON turn index, on-demand message retrieval); [21a3b63] feat: turn embedding digest — files_read/files_changed/outcome 列 + keyword-dense embedding text 重写(project:X files:Y intent:Z outcome:W); [65c8b59] feat: 用 EventAnalyzerWorker 替代 git_watcher 30s 轮询 — ContextualCommitDetected 事件携带 conversation/session/slot_id; git_watcher.rs REMOVED; [1ea1838] refactor: commit detection 移入 tagger-chunker; [081b4f9] fix: 解析真实 Bash 命令格式 [Tool: Bash]; [a9517ec] fix: SlotConfig.initial_prompt 通过 pty_spawn handler 传入; [20813d5] feat: SlotConfig 新增 initial_prompt + serde alias 修复(dangerously_skip_permissions/mcp_config/auto_start); [5a5f805] refactor: missiond-semantic 提取为独立 open-source crate (semantic-terminal); [ec269d7] feat: 权限持久化全链路(perm_injector新模块+LearnedPermissions.REQUIRES_PARAM_PATTERN+pty confirm Option(2)自动审批+sequential PTY writes) + mission_intent MCP工具(read/section/summary/list,模糊项目匹配) + mission_codex_ops MCP工具 + CodexIngestionWorker(读~/.codex/state_5.sqlite写会话时间线); [79a877f] feat: add LispSurveyWorker — commit-triggered intent.lisp maintenance (ContextualCommitDetected→debounce 60s→lisp-surveyor slot, self-trigger filtered, diff截断)")

  ;; ══════════════════════════════════════════════════════
  ;; DESIGN CONSTRAINTS
  ;; ══════════════════════════════════════════════════════
  (design-constraints
    (crate-boundary
      (missiond-shared   "CliEngine enum + default_mission_home — re-exports CliEngine from semantic-terminal")
      ;; ⚠ missiond-semantic REMOVED (commit 5a5f805) — now external crate:
      ;;   workspace dep: semantic-terminal = { path = "../semantic-terminal/..." }
      ;;   (github.com/ruoqijin/semantic-terminal, independent open-source crate)
      ;;   pure_parsing/ (Forge GenGap) moved to missiond-core/src/semantic_parsing/
      (semantic-terminal "[EXTERNAL] semantic terminal parser: fingerprints, state, confirm, title, tool, status, patterns")
      (missiond-pty      "PTY session management: PTYManager, PTYSession, IncrementalExtractor, screenshot")
      (missiond-core     "types, DB traits, IPC, embedding, context — depends on pty + semantic-terminal(external) + shared")
      (missiond-daemon   "business logic, handlers, engines, workers (sonnet/codex/gemini/local), LLM gateways")
      (missiond-mcp      "MCP JSON-RPC stdio server, tool schema + dispatch")
      (missiond-attach   "CLI PTY attach utility")
      (missiond-runner   "Claude CLI process wrapper")
      (semantic-terminal-napi "Node N-API bindings for semantic parser")
      (skill-store       "standalone skill marketplace microservice"))

    (communication
      (internal  "DaemonEvent broadcast + per-worker MPSC channels")
      (external  "IPC Unix-socket/TCP between mcp↔daemon")
      (frontend  "WebSocket tokio-tungstenite for realtime board UI"))

    (db-access
      (backend "PostgreSQL (sqlx) + SQLite (rusqlite) dual-backend via MissionDB trait")
      (gateway "missiond-core/src/db/ is sole DB gateway — 100+ query functions")
      (generated "crates/missiond-core/src/db/gen/*.rs from Forge codegen, custom hand-written in sibling db/ modules"))

    (authority
      (pty-state         "semantic parser pipeline is authority")
      (task-lifecycle    "autopilot tick loop is authority")
      (slot-lifecycle    "SlotManager is authority")
      (knowledge-truth   "PostgreSQL KB tables are authority")
      (permission        "PermissionPolicy + LearnedPermissions is authority")))

  ;; ══════════════════════════════════════════════════════
  ;; UPSTREAM DEPENDENCIES
  ;; ══════════════════════════════════════════════════════
  (upstream-dependency
    (service "forge"
      :consumes ("forge-build-cli" "generated-rs-output" "generation-gap-pattern")
      :breaks-if ("codegen-pattern-change" "ir-whitelist-change")))

  (upstream-dependency
    (service "mechanic"
      :consumes ("mechanic-repair-cli" "governance-mode-repair")
      :breaks-if ("repair-cli-interface-change" "governance-mode-change")))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 1: DATABASE SCHEMA — CORE BUSINESS TABLES
  ;; ══════════════════════════════════════════════════════
  (pillar db-core
    (purpose "primary business entities: projects, board tasks, conversations, knowledge, compute")

    ;; ── Project Registry (P1: 2026-04-10) ──────────────────────────────────
    (component project-tables
      :pattern crud-gateway
      :target "crates/missiond-core/src/db/pg/project.rs"
      :types-target "crates/missiond-core/src/types/project.rs"
      :migration ("20260410000000_projects.sql" "20260410200000_backfill_project_id.sql")

      (struct ProjectConfig
        (field id          :type text :pk :comment "project identifier, e.g. project name or UUID")
        (field path        :type text :unique :comment "root filesystem path of the project")
        (field intent_path :type "Option<String>" :comment "optional path to .missiond/intent.lisp")
        (field active      :type bool :default true)
        (field slots       :type "Vec<String>" :comment "slot names associated with this project")
        (field github_url  :type "Option<String>" :comment "GitHub remote URL; persisted in DB via migration 20260410300000; resolved at init/sync via git remote get-url origin" :added "84ac1a6")
        (field created_at  :type "Option<DateTime<Utc>>")
        (field updated_at  :type "Option<DateTime<Utc>>"))

      (struct ProjectRegistry
        :note "in-memory cache with longest-prefix CWD resolution; loaded from PG at startup"
        (field projects    :type "Vec<ProjectConfig>")
        (field path_index  :type "Vec<(String, String)>" :comment "sorted by path length desc for longest-prefix match"))

      (method resolve
        :args "cwd: &str"
        :returns "Option<&str>"
        :doc "最长前置路径匹配：returns project_id if cwd starts_with any registered path")

      (method exclusive_slots
        :args "project_id: &str"
        :returns "Vec<String>"
        :doc "slots belonging exclusively to this project (not shared with other active projects)")

      (type SharedProjectRegistry :alias "Arc<RwLock<ProjectRegistry>>")

      (table projects
        (col id          :type text :pk)
        (col path        :type text :unique :not-null)
        (col intent_path :type text :nullable)
        (col active      :type boolean :not-null :default true)
        (col slots       :type "text[]" :not-null :default "{}")
        (col github_url  :type text :nullable :added "20260410300000_project_github_url.sql"
          :note "ALTER TABLE projects ADD COLUMN IF NOT EXISTS github_url TEXT; 含已知4项目初始值回填")
        (col created_at  :type timestamptz :not-null :default now)
        (col updated_at  :type timestamptz :not-null :default now)

        (op list   :returns "Vec<ProjectConfig>" :note "list action响应额外附加lispFiles/lispCount(handler层readdir扫描,非DB字段)")
        (op get    :binds id :returns "Option<ProjectConfig>")
        (op list-active :where "active=true")
        (op upsert :binds (id path intent_path active slots github_url) :conflict-on id
          :note "github_url ON CONFLICT: COALESCE(EXCLUDED.github_url, projects.github_url) — 不覆盖已有值")
        (op set-active :binds (id active) :returns "bool (rows_affected>0)")
        (op set-slots  :binds (id slots))
        (op delete :binds id))

      (trait ProjectStore
        :target "crates/missiond-core/src/db/traits.rs"
        (list_projects   :returns "DbResult<Vec<ProjectConfig>>")
        (get_project     :binds id :returns "DbResult<Option<ProjectConfig>>")
        (upsert_project  :binds "config: &ProjectConfig" :returns "DbResult<()>")
        (set_project_active :binds (id active) :returns "DbResult<bool>")
        (backfill_project_id :binds (project_id path_pattern) :returns "DbResult<u64>"
          :sql "UPDATE conversations SET project_id=$1 WHERE project_id IS NULL AND project LIKE $2"
          :added "84ac1a6"))

      ;; ── Backfill Migration (20260410200000, commit e18d0bf) ──
      (backfill backfill_project_id
        :migration "20260410200000_backfill_project_id.sql"
        :doc "一次性回填 + 种子: (1) conversations.project_id 通过 path 前缀匹配回填 9 个已知项目; (2) 向 projects 表 INSERT ON CONFLICT DO NOTHING 种子 9 行"
        (seed-projects
          ("missiond"            "/Users/jinchen/Projects/missiond"            :intent ".missiond/intent.lisp" :active true)
          ("jarvis-forge"        "/Users/jinchen/Projects/jarvis-forge"        :intent ".jarvis/intent.lisp"   :active true :slots ("lisp-surveyor"))
          ("jarvis"              "/Users/jinchen/Projects/jarvis"              :active true)
          ("jarvis-mechanic"     "/Users/jinchen/Projects/jarvis-mechanic"     :active false)
          ("xjp-deploy-agent"    "/Users/jinchen/Projects/xjp-deploy-agent"    :active true)
          ("pcea-video-vault"    "/Users/jinchen/Downloads/PCEA/develop/pcea-video-vault" :active true)
          ("xiaojinpro-backend"  "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend" :active true)
          ("srteditor"           "/Users/jinchen/development/SRTeditor"         :active true)
          ("xiaojincut"          "/Users/jinchen/development/xiaojincut/xiaojincut" :active false))))

    (component board-tables
      :pattern crud-gateway
      :target "crates/missiond-core/src/db/board.rs"
      :gen-target "crates/missiond-core/src/db/gen/board.rs"

      (table board_tasks
        (col id :type uuid :pk)
        (col title :type text :not-null)
        (col description :type text)
        (col status :type text :not-null :default "open"
          :enum ("open" "running" "done" "failed" "blocked"))
        (col priority :type text :default "medium"
          :enum ("critical" "high" "medium" "low"))
        (col engineering_phase :type text
          :enum ("investigate" "consult" "plan" "execute" "finalize"))
        (col category :type text)
        (col executor :type text)
        (col assigned_slot :type text)
        (col depends_on :type jsonb)
        (col dedupe_key :type text)
        (col lease_until :type timestamptz)
        (col retry_count :type integer :default 0)
        (col autopilot :type boolean :default false)
        (col context :type jsonb)
        (col project_id :type text :nullable :comment "project scope; NULL=global" :added "20260410100000")
        (col created_at :type timestamptz :not-null)
        (col updated_at :type timestamptz :not-null)

        (op insert (binds title description status priority category))
        (op create (binds title description priority category engineering_phase depends_on dedupe_key autopilot context))
        (op select-one (binds id) :alias get_board_task)
        (op select-many (binds status) (order-by updated_at :desc) (limit 100))
        (op update (binds title description status priority engineering_phase category executor assigned_slot) (where id))
        (op delete (where id))
        (op toggle (binds status) (where id))
        (op claim (binds executor assigned_slot) (where id status="open"))
        (op retry (binds retry_count status) (where id))
        (op search (binds query status category priority) :fts true)
        (op find-by-dedupe (binds dedupe_key status="open"))
        (op close-by-dedupe (binds dedupe_key))
        (op list-running-autopilot (where autopilot=true status="running"))
        (op list-autopilot (where autopilot=true))
        (op board-summary :aggregate true)
        (op count-by-priority (group-by priority) (where status="open"))
        (op count-by-category (group-by category))
        (op check-dependencies (binds depends_on))
        (op find-downstream (binds id))
        (op set-lease (binds lease_until) (where id))
        (op release-claims (binds executor))
        (op recover-stale (where lease_until < now()))
        (op clear-done)
        (op query-in-range (binds since until))
        (op query-in-range-with-status (binds since until status)))

      (table board_task_notes
        (col id :type uuid :pk)
        (col task_id :type uuid :fk board_tasks.id)
        (col author :type text :not-null)
        (col content :type text :not-null)
        (col created_at :type timestamptz :not-null)

        (op insert (binds task_id author content))
        (op select-by-task (binds task_id) (order-by created_at :asc))))

    (component conversation-tables
      :pattern crud-gateway
      :target "crates/missiond-core/src/db/conversation.rs"
      :gen-target "crates/missiond-core/src/db/gen/conversation.rs"

      (table conversations
        (col id :type text :pk :comment "session_id from Claude Code")
        (col slot_name :type text)
        (col source :type text :comment "claude-code|gemini-cli|codex")
        (col status :type text :default "active" :enum ("active" "completed"))
        (col project_id :type text :nullable :comment "project scope; resolved from CWD via ProjectRegistry" :added "20260410100000")
        (col parent_id :type text :fk conversations.id)
        (col task_id :type text)
        (col summary :type text)
        (col summary_embedding :type bytea)
        (col topic_vectors :type bytea)
        (col timeline :type jsonb)
        (col jsonl_path :type text)
        (col exit_code :type integer)
        (col deep_checkpoint :type text)
        (col created_at :type timestamptz :not-null)
        (col updated_at :type timestamptz :not-null)
        (col completed_at :type timestamptz)
        (col analyzed_at :type timestamptz)
        (col compacted_at :type timestamptz)
        (col habit_scanned_at :type timestamptz)

        ;; started_at fix (commit 0adbb18): 3 code paths were writing NOW() instead of real timestamp
        ;; Fix: ensure_conversation_exists accepts started_at param, COALESCE(started_at::timestamptz, NOW())
        ;; upsert ON CONFLICT: updates started_at=CASE WHEN $11 != '' AND $11 != 'unknown' THEN $11 ELSE conversations.started_at END
        ;; Migration 20260410400000: fixed 6623 broken records via MIN(conversation_messages.timestamp)
        (op upsert (binds id slot_name source status jsonl_path)
          :on-conflict "updates started_at when real timestamp available (0adbb18)")
        (op ensure-conversation-exists
          (binds session_id project_path jsonl_path status conversation_type parent_session_id started_at)
          :note "started_at: Option<&str>; COALESCE with NOW() if None (commit 0adbb18)")
        (op select-one (binds id))
        (op select-children (binds parent_id))
        (op list (binds status source limit offset since until conv_type task_id))
        (op complete (binds completed_at) (where id))
        (op save-exit-code (binds exit_code) (where id))
        (op mark-analyzed (binds analyzed_at) (where id))
        (op mark-compacted (binds compacted_at) (where id))
        (op mark-habit-scanned (binds habit_scanned_at) (where id))
        (op set-task-id (binds task_id) (where id))
        (op get-by-task-id (binds task_id))
        (op set-summary (binds summary) (where id))
        (op clear-summary (where id))
        (op set-embedding (binds summary_embedding) (where id))
        (op set-topic-vectors (binds topic_vectors) (where id))
        (op set-timeline (binds timeline) (where id))
        (op missing-summary (where summary=NULL completed_at!=NULL) (limit 50))
        (op stale-embedding (where summary_embedding=NULL summary!=NULL))
        (op needing-topic-vectors (where topic_vectors=NULL summary!=NULL))
        (op needing-timeline (where timeline=NULL completed_at!=NULL))
        (op load-embeddings :returns "Vec<(id, embedding)>")
        (op load-topic-vectors :returns "Vec<(id, vectors)>")
        (op reactivate (binds id))
        (op complete-stale (where status="active" updated_at < threshold))
        (op pending-deep-analysis)
        (op unscanned (where habit_scanned_at=NULL) (limit batch_size))
        (op compaction-fragments (binds id)))

      (table conversation_messages
        (col id :type bigint :pk :autoincrement)
        (col conversation_id :type text :fk conversations.id)
        (col role :type text :not-null :enum ("user" "assistant" "system" "tool"))
        (col content :type text)
        (col tool_name :type text)
        (col tool_call_id :type text)
        (col token_usage :type jsonb)
        (col created_at :type timestamptz :not-null)
        (col memory_forwarded_at :type timestamptz)
        (col user_voice_forwarded_at :type timestamptz)
        (col realtime_forwarded_at :type timestamptz)

        (op insert (binds conversation_id role content tool_name tool_call_id token_usage))
        (op insert-batch (binds "Vec<message>"))
        (op select-one (binds id))
        (op select-by-conversation (binds conversation_id limit offset))
        (op get-around (binds id context_count))
        (op search (binds query conversation_id) :fts true)
        (op search-filtered (binds query role slot_name since until))
        (op search-sessions-fts (binds query limit))
        (op search-sessions-fts-filtered (binds query slot_name since until limit))
        (op get-fts-snippets (binds conversation_id query))
        (op pending-memory (where memory_forwarded_at=NULL role="assistant"))
        (op pending-user-voice (where user_voice_forwarded_at=NULL role="user"))
        (op pending-realtime (where realtime_forwarded_at=NULL))
        (op pending-realtime-with-limit (binds limit))
        (op update-memory-forwarded (binds memory_forwarded_at) (where id))
        (op update-user-voice-forwarded (binds user_voice_forwarded_at) (where id))
        (op update-realtime-forwarded (binds realtime_forwarded_at) (where id))
        (op last-assistant-content (binds conversation_id))))

    (component knowledge-tables
      :pattern crud-gateway
      :target "crates/missiond-core/src/db/knowledge.rs"
      :gen-target "crates/missiond-core/src/db/gen/knowledge.rs"

      (table knowledge
        (col id :type uuid :pk)
        (col key :type text :unique :not-null)
        (col category :type text :not-null)
        (col scope :type text)
        (col content :type text :not-null)
        (col source :type text)
        (col embedding :type bytea)
        (col linked_task_id :type text)
        (col project_id :type text :nullable :comment "project scope; NULL=global knowledge accessible by all projects" :added "20260410100000")
        (col access_count :type integer :default 0)
        (col last_accessed_at :type timestamptz)
        (col created_at :type timestamptz :not-null)
        (col updated_at :type timestamptz :not-null)

        ;; commit 3c10d21: project_id 写入路径
        ;; - remember: INSERT增加project_id列; ON CONFLICT: project_id = COALESCE(EXCLUDED.project_id, knowledge.project_id)
        ;; - KBRememberInput.project_id: Option<String>, 透传自 mission_kb_remember 的 project 参数
        ;; - kb_update: 新增 new_project_id: Option<&str> 参数; 空串→清除归属(设为NULL)
        ;; - SQLite(MissionDB): project_id 字段返回 None (仅 PG 持久化)
        (op remember (binds key category scope content source project_id) :upsert-on key)
        (op update (binds key content category scope source project_id) (where key))
        (op select-one (binds id))
        (op get-id-by-key (binds key))
        (op search-fts (binds query category limit) :fts true)
        (op search-like (binds pattern category limit))
        (op search-ranked (binds query embedding category limit) :hybrid true)
        (op search (binds query category limit))
        ;; P4+P5 (commit 76900d1): project-scoped search variants
        (op search-fts-scoped  (binds query category project_id)
          :where "AND (project_id = $X OR project_id IS NULL)"
          :note "fallback to unscoped if project_id IS NULL")
        (op search-like-scoped (binds query category project_id)
          :where "AND (project_id = $X OR project_id IS NULL)"
          :note "dynamic SQL with keyword expansion")
        (op list (binds category limit offset))
        (op list-paginated (binds category cursor limit))
        (op list-by-scope (binds scope))
        (op forget (binds id))
        (op batch-forget (binds "Vec<id>"))
        (op clear-scope (binds scope))
        (op set-embedding (binds id embedding))
        (op set-linked-task-id (binds id linked_task_id))
        (op stale-embedding (where updated_at > embedded_at))
        (op missing-embedding (where embedding=NULL))
        (op load-embeddings :returns "Vec<(id, embedding)>")
        (op load-all-embeddings :returns "Vec<(id, key, embedding)>")
        (op update-access-stats (binds id))
        (op summary :aggregate true)
        (op hot-keys (order-by access_count :desc) (limit 20))
        (op find-stale (where updated_at < threshold))
        (op find-duplicates)
        (op embedding-stats :aggregate true))

      (table knowledge_edges
        (col from_id :type uuid :fk knowledge.id)
        (col to_id :type uuid :fk knowledge.id)
        (col relation :type text)
        (col created_at :type timestamptz)
        (op add-edge (binds from_id to_id relation)))

      (table prompt_snapshots
        (col id :type uuid :pk)
        (col prompt_text :type text :not-null)
        (col model :type text)
        (col outcome :type text)
        (col created_at :type timestamptz :not-null)
        (col updated_at :type timestamptz)
        (op save (binds prompt_text model))
        (op update-outcome (binds id outcome))
        (op list (order-by created_at :desc) (limit 50))))

    (component compute-tables
      :pattern crud-gateway
      :target "crates/missiond-core/src/db/slot.rs"
      :gen-target "crates/missiond-core/src/db/gen/compute.rs"

      (table slot_sessions
        (col slot_name :type text :pk)
        (col session_id :type text)
        (col pid :type integer)
        (col status :type text)
        (col started_at :type timestamptz)
        (op set (binds slot_name session_id pid status))
        (op delete (binds slot_name)))

      (table slot_tasks
        (col id :type uuid :pk)
        (col slot_name :type text :not-null)
        (col task_type :type text)
        (col prompt :type text)
        (col status :type text :default "queued" :enum ("queued" "running" "completed" "failed"))
        (col result :type text)
        (col created_at :type timestamptz :not-null)
        (col started_at :type timestamptz)
        (col completed_at :type timestamptz)
        (op insert (binds slot_name task_type prompt))
        (op set-running (binds started_at) (where id))
        (op set-completed (binds result completed_at) (where id))
        (op set-failed (binds result completed_at) (where id)))

      (table tasks
        (col id :type uuid :pk)
        (col description :type text :not-null)
        (col status :type text :default "queued" :enum ("queued" "running" "completed" "failed"))
        (col result :type text)
        (col slot_name :type text)
        (col created_at :type timestamptz :not-null)
        (col started_at :type timestamptz)
        (col completed_at :type timestamptz)
        (op insert (binds description))
        (op select-one (binds id)))

      (table daemon_state
        (col key :type text :pk)
        (col value :type text :not-null)
        (col updated_at :type timestamptz)
        (op set (binds key value)))

      (table inbox_messages
        (col id :type uuid :pk)
        (col slot_name :type text :not-null)
        (col content :type text :not-null)
        (col read :type boolean :default false)
        (col created_at :type timestamptz :not-null)
        (col read_at :type timestamptz)
        (op insert (binds slot_name content))
        (op mark-read (binds id)))))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 2: DATABASE SCHEMA — OBSERVABILITY
  ;; ══════════════════════════════════════════════════════
  (pillar db-observability
    (purpose "audit trail, tool calls, timeline, LLM traces, retrospectives")

    (component audit-tables
      :pattern crud-gateway
      :target "crates/missiond-core/src/db/audit.rs"
      :gen-target "crates/missiond-core/src/db/gen/audit.rs"

      (table tool_calls
        (col id :type uuid :pk)
        (col session_id :type text :not-null)
        (col conversation_id :type text)
        (col tool_name :type text :not-null)
        (col input :type jsonb)
        (col output :type jsonb)
        (col status :type text)
        (col error :type text)
        (col created_at :type timestamptz :not-null)
        (col completed_at :type timestamptz)

        (op insert (binds session_id conversation_id tool_name input))
        (op insert-batch (binds "Vec<tool_call>"))
        (op update-output (binds id output status error completed_at))
        (op select-one (binds id))
        (op select-by-session (binds session_id limit offset))
        (op stats (binds session_id) :aggregate true)
        (op count-pending (binds session_id))
        (op sessions-with-pending :returns "Vec<session_id>")
        (op for-detailed-analysis (binds session_id))
        (op with-status-timeline (binds session_id))
        (op name-sequence (binds session_id))
        (op error-samples (binds tool_name limit))
        (op retro-tool-stats (binds session_id))
        (op retro-meta (binds session_id))
        (op retro-repeat-patterns (binds session_id))
        (op retro-high-error-tools (binds session_id))
        (op first-user-message (binds session_id))
        (op for-backfill (binds session_id)))

      (table conversation_events
        (col id :type bigint :pk :autoincrement)
        (col conversation_id :type text :not-null)
        (col event_type :type text :not-null)
        (col data :type jsonb)
        (col created_at :type timestamptz :not-null)
        (op insert-batch (binds "Vec<event>"))
        (op select (binds conversation_id event_type since until limit))
        (op agent-trajectory (binds conversation_id))
        (op type-summary (binds conversation_id) :aggregate true)
        (op cleanup-old (binds threshold))
        (op sessions-with-events (binds since limit)))

      (table retrospective_results
        (col session_id :type text :pk)
        (col result :type jsonb :not-null)
        (col created_at :type timestamptz :not-null)
        (op save (binds session_id result))
        (op has (binds session_id) :returns bool)
        (op select-one (binds session_id))
        (op list (binds limit offset))
        (op needing-retro (where NOT EXISTS))
        (op for-backfill)))

    (component timeline-tables
      :target "crates/missiond-core/src/db/timeline.rs"

      (table system_timeline
        (col id :type bigint :pk :autoincrement)
        (col event_type :type text :not-null)
        (col slot_name :type text)
        (col session_id :type text)
        (col summary :type text)
        (col data :type jsonb)
        (col trace_id :type text)
        (col created_at :type timestamptz :not-null)
        (op insert (binds event_type slot_name session_id summary data trace_id))
        (op query (binds event_type slot_name since until limit offset))
        (op update-summary (binds id summary))
        (op stats (binds since) :aggregate true)
        (op traces (binds trace_id))))

    (component gemini-log-tables
      :target "crates/missiond-core/src/db/gemini_log.rs"

      (table gemini_requests
        (col id :type uuid :pk)
        (col model :type text :not-null)
        (col purpose :type text)
        (col prompt_preview :type text)
        (col response_preview :type text)
        (col input_tokens :type integer)
        (col output_tokens :type integer)
        (col status :type text)
        (col duration_ms :type integer)
        (col created_at :type timestamptz :not-null)
        (col completed_at :type timestamptz)
        (op insert-started (binds model purpose prompt_preview))
        (op update-completed (binds id response_preview input_tokens output_tokens status duration_ms))
        (op get-content (binds id))
        (op query (binds model purpose since until limit))
        (op stats (binds since) :aggregate true)
        (op cleanup (binds threshold)))

      (table gemini_file_cache
        (col file_hash :type text :pk)
        (col uri :type text :not-null)
        (col expires_at :type timestamptz :not-null)
        (op get (binds file_hash))
        (op put (binds file_hash uri expires_at))
        (op gc (where expires_at < now()))))

    (component incident-tables
      :target "crates/missiond-core/src/db/incident.rs"

      (table incidents
        (col id :type uuid :pk)
        (col incident_type :type text :not-null)
        (col severity :type text :not-null)
        (col message :type text :not-null)
        (col data :type jsonb)
        (col created_at :type timestamptz :not-null)
        (op insert (binds incident_type severity message data))
        (op has-recent (binds incident_type since) :returns bool)
        (op list (binds severity since limit)))

      (table token_usage
        (col id :type uuid :pk)
        (col session_id :type text)
        (col model :type text)
        (col input_tokens :type integer)
        (col output_tokens :type integer)
        (col cost_usd :type float8)
        (col created_at :type timestamptz :not-null)
        (op insert (binds session_id model input_tokens output_tokens))
        (op stats (binds since) :aggregate true)))

    (component narration-tables
      :target "crates/missiond-core/src/db/narration.rs"

      (table narration_cursors
        (col conversation_id :type text :pk)
        (col last_message_id :type bigint)
        (col processing :type boolean :default false)
        (col updated_at :type timestamptz)
        (op mark-processing (binds conversation_id)))

      (table message_narrations
        (col id :type bigint :pk :autoincrement)
        (col message_id :type bigint :fk conversation_messages.id)
        (col narration :type text :not-null)
        (col created_at :type timestamptz :not-null))))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 3: DATABASE SCHEMA — PIPELINE & CODE INTEL
  ;; ══════════════════════════════════════════════════════
  (pillar db-pipeline
    (purpose "conversation turns, AST sync, beacons, backfill phases, watermarks, translations")

    ;; ── Conversation Turns (S3 Tagger & Chunker pipeline stage) ──────────────
    (component turn-tables
      :target "crates/missiond-core/src/db/pg/conversation.rs"
      :migration ("20260322100000_conversation_turns.sql"
                  "20260409000000_turn_embedding_digest.sql"
                  "20260409100000_turn_skeleton.sql")

      (table conversation_turns
        (col id              :type bigserial :pk)
        (col session_id      :type text :not-null :fk conversations.id :on-delete cascade)
        (col turn_idx        :type integer :not-null)
        (col start_message_id :type bigint :not-null :fk conversation_messages.id)
        (col end_message_id  :type bigint :not-null :fk conversation_messages.id)

        ;; Pre-extracted summary fields (pure rules, no LLM)
        (col user_content    :type text :comment "User message text, truncated 2000 chars")
        (col tool_names      :type text :comment "CSV: Bash,Read,Edit")
        (col tool_call_count :type integer :default 0)
        (col message_count   :type integer :default 0)
        (col has_code_change :type boolean :default false)
        (col has_mcp_call    :type boolean :default false)
        (col started_at      :type text)
        (col ended_at        :type text)

        ;; Downstream fields (S4 Embedder / Intent Analyst)
        (col topic           :type text)
        (col intent_group_id :type bigint)

        ;; Embedding digest fields (commit 21a3b63 — 20260409000000)
        (col files_read    :type text :nullable
          :comment "short file names extracted from Read tool calls via regex")
        (col files_changed :type text :nullable
          :comment "short file names extracted from Write/Edit tool calls via regex")
        (col outcome       :type text :nullable
          :comment "last assistant non-tool text — embedding signal for what was accomplished")

        ;; Turn skeleton (commit dc45dcb — 20260409100000)
        (col skeleton      :type text :nullable
          :comment "JSON index: {user_msg_id, tool_calls:[{tool,file,req_msg_id,res_msg_id}], outcome_msg_id}; enables on-demand selective message retrieval instead of loading all 68+ messages")

        (unique (session_id turn_idx))
        (index idx_ct_session      :cols (session_id))
        (index idx_ct_session_idx  :cols (session_id turn_idx))
        (index idx_ct_start_msg    :cols (start_message_id))
        (index idx_ct_end_msg      :cols (end_message_id))

        (op get-last-turn-end-msg (binds session_id) :returns "Option<i64>")
        (op get-max-turn-idx      (binds session_id) :returns "Option<i32>")
        (op insert-batch
          (binds session_id base_idx "Vec<RawTurn>")
          :note "INSERT ON CONFLICT (session_id, turn_idx) DO UPDATE — idempotent upsert")
        (op missing-embeddings
          :where "NOT EXISTS (SELECT 1 FROM conversation_turns ct WHERE ct.session_id = c.id)"
          :returns "Vec<session_id>"))

      ;; TurnBuilder — logic inside tagger_chunker.rs (commit 21a3b63 refactor)
      (struct RawTurn
        :target "crates/missiond-core/src/types/conversation.rs"
        (field turn_idx      :type i32)
        (field start_message_id :type i64)
        (field end_message_id   :type i64)
        (field user_content  :type "Option<String>")
        (field tool_names    :type "Vec<String>")
        (field tool_call_count :type i32)
        (field message_count   :type i32)
        (field has_code_change :type bool)
        (field has_mcp_call    :type bool)
        (field started_at    :type "Option<String>")
        (field ended_at      :type "Option<String>")
        (field files_read    :type "Option<String>" :added "21a3b63")
        (field files_changed :type "Option<String>" :added "21a3b63")
        (field outcome       :type "Option<String>" :added "21a3b63")
        (field skeleton      :type "Option<String>" :added "dc45dcb"))

      (note "build_turn_embedding_text (commit 21a3b63): rewrote from natural-language to structured keyword-dense format: 'project:X files:Y intent:Z outcome:W' — optimized for MCP vector search by Claude Code agents. Filters <task-notification> noise turns."))

    (component ast-tables
      :target "crates/missiond-core/src/db/ast.rs"

      (table ast_nodes
        (col id :type uuid :pk)
        (col file_path :type text :not-null)
        (col symbol_name :type text :not-null)
        (col kind :type text :not-null
          :enum ("function" "struct" "enum" "trait" "impl" "mod" "const" "type"))
        (col start_line :type integer :not-null)
        (col end_line :type integer :not-null)
        (col signature :type text)
        (col is_exported :type boolean :default false)
        (col parent_symbol :type text)
        (col embedding :type bytea)
        (col beacon_tags :type jsonb)
        (col commit_hash :type text)
        (col created_at :type timestamptz :not-null)

        (op sync-file (binds file_path "Vec<CodeNode>" commit_hash))
        (op delete-file (binds file_path))
        (op search (binds query kind is_exported limit) :fts true)
        (op search-ranked (binds query embedding limit) :hybrid true)
        (op get-file-nodes (binds file_path))
        (op find-related (binds symbol_name kind))
        (op get-file-commit (binds file_path))
        (op stats :aggregate true)
        (op set-embedding (binds id embedding))
        (op find-unembedded (limit batch_size))
        (op load-all-embeddings :returns "Vec<(id, embedding)>")
        (op get-search-hit (binds id))
        (op get-node (binds id))
        (op module-summaries :returns "Vec<(module, count)>")))

    (component beacon-tables
      :target "crates/missiond-core/src/db/beacon.rs"
      :gen-target "crates/missiond-core/src/db/gen/pipeline.rs"

      (table beacons
        (col id :type uuid :pk)
        (col tag :type text :unique :not-null)
        (col description :type text)
        (col harvest_count :type integer :default 0)
        (col created_at :type timestamptz :not-null)
        (op list)
        (op ensure (binds tag) :upsert-on tag)
        (op increment-harvest (binds tag))
        (op set-description (binds tag description))
        (op search (binds query) :fts true)
        (op cleanup-empty (where harvest_count=0)))

      (table beacon_nodes
        (col id :type uuid :pk)
        (col beacon_id :type uuid :fk beacons.id)
        (col file_path :type text :not-null)
        (col symbol_name :type text)
        (col line :type integer)
        (col annotation :type text)
        (op upsert (binds beacon_id file_path symbol_name line))
        (op map (binds beacon_id))
        (op delete-file (binds file_path))
        (op annotate (binds id annotation))))

    (component pipeline-tables
      :gen-target "crates/missiond-core/src/db/gen/pipeline.rs"

      (table backfill_phases
        (col phase_name :type text :pk)
        (col status :type text :default "pending" :enum ("pending" "running" "completed" "failed"))
        (col progress :type integer :default 0)
        (col total :type integer)
        (col started_at :type timestamptz)
        (col completed_at :type timestamptz)
        (col error :type text)
        (op get (binds phase_name))
        (op list)
        (op start (binds phase_name total))
        (op update-progress (binds phase_name progress))
        (op complete (binds phase_name))
        (op record-failure (binds phase_name error))
        (op retryable-failures)
        (op clear-failure (binds phase_name)))

      (table watermarks
        (col key :type text :pk)
        (col time_value :type timestamptz)
        (col msg_id_value :type bigint)
        (op advance-time (binds key time_value))
        (op advance-msg-id (binds key msg_id_value)))

      (table labels
        (col key :type text :pk)
        (col value :type text :not-null)
        (op set (binds key value)))

      (table translations
        (col id :type uuid :pk)
        (col message_id :type bigint)
        (col language :type text :not-null)
        (col content :type text :not-null)
        (col created_at :type timestamptz :not-null)
        (op insert (binds message_id language content))
        (op has (binds message_id language) :returns bool)))

    (component vision-tables
      :target "crates/missiond-core/src/db/vision.rs"

      (table image_descriptions
        (col id :type uuid :pk)
        (col image_hash :type text :unique :not-null)
        (col description :type text :not-null)
        (col created_at :type timestamptz :not-null)
        (op save (binds image_hash description) :upsert-on image_hash)
        (op count :aggregate true))))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 4: DATABASE SCHEMA — AGENTS & SKILLS
  ;; ══════════════════════════════════════════════════════
  (pillar db-agents
    (purpose "questions, dynamic slots, skills, router chat")

    (component question-tables
      :target "crates/missiond-core/src/db/question.rs"
      :gen-target "crates/missiond-core/src/db/gen/misc.rs"

      (table agent_questions
        (col id :type uuid :pk)
        (col task_id :type uuid)
        (col slot_name :type text)
        (col question :type text :not-null)
        (col context :type jsonb)
        (col status :type text :default "pending" :enum ("pending" "answered" "dismissed"))
        (col answer :type text)
        (col routing_trace :type jsonb)
        (col retry_count :type integer :default 0)
        (col created_at :type timestamptz :not-null)
        (col answered_at :type timestamptz)
        (op select-one (binds id))
        (op list-for-task (binds task_id))
        (op increment-retry (binds id))
        (op set-routing-trace (binds id routing_trace))
        (op downgrade-to-user (binds id))))

    (component dynamic-slot-tables
      :target "crates/missiond-core/src/db/dynamic_slot.rs"
      :gen-target "crates/missiond-core/src/db/gen/misc.rs"

      (table dynamic_slots
        (col id :type uuid :pk)
        (col slot_name :type text :unique :not-null)
        (col config :type jsonb :not-null)
        (col status :type text :default "active")
        (col expires_at :type timestamptz)
        (col created_at :type timestamptz :not-null)
        (op create (binds slot_name config expires_at))
        (op select-one (binds id))
        (op list (binds status))
        (op count-active)
        (op terminate (binds id))
        (op extend (binds id expires_at))
        (op find-expired (where expires_at < now()))
        (op find-expiring (where expires_at < soon()))))

    (component skill-tables
      :target "crates/missiond-core/src/db/skill.rs"
      :gen-target "crates/missiond-core/src/db/gen/skill.rs"

      (table skills
        (col id :type uuid :pk)
        (col name :type text :unique :not-null)
        (col description :type text)
        (col content :type text :not-null)
        (col version :type integer :default 1)
        (col enabled :type boolean :default true)
        (col created_at :type timestamptz :not-null)
        (col updated_at :type timestamptz :not-null))

      (table skill_topics
        (col id :type uuid :pk)
        (col skill_id :type uuid :fk skills.id)
        (col topic :type text :not-null)
        (col hit_count :type integer :default 0)
        (col last_hit_at :type timestamptz)
        (op hit (binds id))
        (op update-stats (binds id hit_count)))

      (table skill_blocks
        (col id :type uuid :pk)
        (col skill_id :type uuid :fk skills.id)
        (col block_type :type text :not-null)
        (col content :type text :not-null)
        (col status :type text :default "active")
        (op set-status (binds id status))))

    (component router-chat-tables
      :target "crates/missiond-core/src/db/router_chat.rs"

      (table router_chat_sessions
        (col id :type uuid :pk)
        (col title :type text)
        (col model :type text)
        (col created_at :type timestamptz :not-null)
        (col updated_at :type timestamptz :not-null))

      (table router_chat_messages
        (col id :type uuid :pk)
        (col session_id :type uuid :fk router_chat_sessions.id)
        (col role :type text :not-null)
        (col content :type text :not-null)
        (col model :type text)
        (col tokens :type integer)
        (col created_at :type timestamptz :not-null))))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 5: STATE MACHINES
  ;; ══════════════════════════════════════════════════════
  (pillar state-machines
    (purpose "all finite state automata governing lifecycle transitions")

    (component pty-session-state
      :target "semantic-terminal crate (external) — src/types.rs"
      (state-machine pty-session
        (states (Starting) (Idle) (SlashMenu) (Thinking) (Responding)
                (ToolRunning) (Confirming) (Error))
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
          (Confirming -> Idle :trigger "denied"))))

    (component board-task-status
      :target "crates/missiond-core/src/types/board.rs"
      (state-machine board-task
        (states (Open) (Running) (Done) (Failed) (Blocked))
        (transitions
          (Open -> Running :trigger "claimed")
          (Open -> Blocked :trigger "dependency-unmet")
          (Running -> Done :trigger "completed")
          (Running -> Failed :trigger "error")
          (Blocked -> Open :trigger "dependency-resolved")))
      (state-machine engineering-phase
        (states (Investigate) (Consult) (Plan) (Execute) (Finalize))
        (transitions
          (Investigate -> Consult :trigger "context-gathered")
          (Consult -> Plan :trigger "review-complete")
          (Plan -> Execute :trigger "plan-approved")
          (Execute -> Finalize :trigger "implementation-done")
          (Finalize -> Investigate :trigger "issues-found"))))

    (component task-status
      :target "crates/missiond-core/src/types/task.rs"
      (state-machine task
        (states (Queued) (Running) (Completed) (Failed))
        (transitions
          (Queued -> Running :trigger "slot-claimed")
          (Running -> Completed :trigger "result-received")
          (Running -> Failed :trigger "error-or-timeout"))))

    (component question-status
      :target "crates/missiond-core/src/types/question.rs"
      (state-machine question
        (states (Pending) (Answered) (Dismissed))
        (transitions
          (Pending -> Answered :trigger "answer-provided")
          (Pending -> Dismissed :trigger "user-dismissed"))))

    (component extraction-phase
      :target "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
      (state-machine extraction
        (states (Idle) (Sending) (WaitingForIdleness) (Complete))
        (transitions
          (Idle -> Sending :trigger "extraction-triggered")
          (Sending -> WaitingForIdleness :trigger "content-sent")
          (WaitingForIdleness -> Complete :trigger "slot-idle")
          (WaitingForIdleness -> Idle :trigger "timeout")
          (Complete -> Idle :trigger "reset")))))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 6: EVENT BUS + WORKER TOPOLOGY
  ;; ══════════════════════════════════════════════════════
  (pillar event-workers
    (purpose "pub-sub event backbone + 20 background workers organized by LLM dependency: sonnet(6) codex(2) gemini(1) local(11)")
    ;; local count: +codex_ingestion_worker (ec269d7); EventAnalyzerWorker(65c8b59) absorbed into tagger-chunker(1ea1838)

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
        :note "mutation + tokio::watch broadcast + crash-recover from control_tree.json"
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
    ;; count: 11 workers (was 10; +codex_ingestion_worker ec269d7; EventAnalyzerWorker added 65c8b59 then absorbed into tagger-chunker 1ea1838)
    (component workers-local
      (worker conversation-logger
        :target "crates/missiond-daemon/src/workers/local/conversation_logger.rs"
        :listens-to JsonlMessageIngested
        :writes-to (conversations conversation_messages))
      (worker conversation-organizer
        :target "crates/missiond-daemon/src/workers/local/conversation_organizer.rs"
        :listens-to MessagePersisted)
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
        :listens-to MessagePersisted
        ;; commit 21a3b63: TurnBuilder extracts files_read/files_changed/outcome from tool calls
        ;; build_turn_embedding_text: keyword-dense format (project:X files:Y intent:Z outcome:W)
        ;; filters <task-notification> noise turns; triggers per-turn embedding after backfill (dc45dcb)
        ;; commit 1ea1838: commit detection logic moved here from EventAnalyzerWorker
        ;; commit 081b4f9: parse actual command from [Tool: Bash] content format
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
        :writes-to "conversation timeline (system_timeline)"
        :note "reads Codex CLI operation history from local SQLite DB; ingests operation logs into MissionD conversation timeline. rusqlite added to missiond-daemon Cargo.toml. spawned in main.rs at daemon boot.")
      (worker gemini-logger
        :target "crates/missiond-daemon/src/workers/local/gemini_logger.rs"
        :trigger channel
        :writes-to gemini_requests)))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 7: ENGINES
  ;; ══════════════════════════════════════════════════════
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
      (depends (db slot_manager)))

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
        ;; commit 20813d5 + a9517ec: SlotConfig now supports initial_prompt
        ;; After slot reaches Idle, spawner injects initial_prompt as first message.
        ;; Also fixes: serde aliases for dangerously_skip_permissions / mcp_config / auto_start
        ;; (were silently ignored due to camelCase mismatch in YAML deserialization).
        ;; Dynamic coder/ops slot templates now default to sonnet model.
        ;; packages/dashboard removed (superseded by board).
        (slot-config-fields
          (initial_prompt :type "Option<String>" :doc "first message injected after slot reaches Idle")
          (dangerously_skip_permissions :type bool :serde-alias "dangerouslySkipPermissions")
          (mcp_config :type "Option<McpConfig>" :serde-alias "mcpConfig")
          (auto_start :type bool :serde-alias "autoStart")))

      ;; commit ec269d7: NEW — perm_injector module
      ;; updated 2026-04-12 (Phase 1-5 architecture upgrade):
      ;;   - Now invoked from INSIDE spawn_tracked_slot — one entry point for all 10
      ;;     spawn paths (was: mission_pty_spawn + mission_compute_slot:create only)
      ;;   - sync signature: (cwd, role, project_id, slot_id, learned) — reads union via get_for_spawn
      (perm-injector :target "crates/missiond-daemon/src/slot_orchestrator/perm_injector.rs"
        :added "ec269d7"
        :updated "2026-04-12"
        :invoked-by ("slot_orchestrator::spawner::spawn_tracked_slot (all spawn paths)")
        :doc "before each slot spawn, reads global+role+project+slot union from learned_permissions.yaml (LearnedPermissions::get_for_spawn) and merges into <cwd>/.claude/settings.local.json (idempotent, preserves existing entries, dedups on (tool_pattern, param_pattern))"
        (note "closes the gap where 8 of 10 spawn paths (task/process/cc_controller/flow_engine/memory_scheduler/gemini_driver/slots_reload) previously bypassed perm injection"))

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

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 8: SEMANTIC PARSER
  ;; ══════════════════════════════════════════════════════
  (pillar semantic-parser
    (purpose "multi-layer recognizer: raw PTY screen → structured states")

    ;; commit 5a5f805: missiond-semantic EXTRACTED → standalone open-source crate
    ;; All targets below are in semantic-terminal crate (external workspace dep)
    ;; Forge GenGap (pure_parsing/) moved to missiond-core/src/semantic_parsing/
    (component parser-pipeline
      :target "semantic-terminal/src/ (external crate)"
      (pipeline
        pattern-config -> fingerprint-registry -> state-parser
        -> confirm-parser -> tool-output-parser)
      (shared-resource "Arc<CompiledPatterns> from YAML hot-reload"))

    (component pattern-config
      :target "semantic-terminal/src/patterns.rs"
      (dispatch "CliEngine enum → engine-specific YAML + parser"))

    (component claude-code-parser
      :target "semantic-terminal/src/state.rs"
      (detection-order
        trust-dialog -> confirm-dialog -> idle-or-slash
        -> processing -> responding -> error))

    (component gemini-parser
      :target "semantic-terminal/src/gemini_state.rs"
      (detection-order
        error -> thinking -> responding -> tool-running
        -> idle -> idle-placeholder -> idle-transitional))

    (component fingerprint :target "semantic-terminal/src/fingerprint.rs")
    (component confirm     :target "semantic-terminal/src/confirm.rs")
    (component tool-parser :target "semantic-terminal/src/tool.rs")
    (component status      :target "semantic-terminal/src/status.rs")
    (component title       :target "semantic-terminal/src/title.rs")

    (component semantic-parsing-gengap
      :target "crates/missiond-core/src/semantic_parsing/"
      :note "Forge GenGap (generated.rs + custom.rs + mod.rs) — moved here from former missiond-semantic"))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 9: LLM GATEWAYS + CONTEXT PIPELINE
  ;; ══════════════════════════════════════════════════════
  (pillar llm-context
    (purpose "multi-provider LLM dispatch + budget-constrained prompt assembly")

    (component llm-gateway
      :target "crates/missiond-daemon/src/llm/llm_gateway.rs"
      (providers
        (gemini-gateway  :target "crates/missiond-daemon/src/llm/gemini_client.rs")
        (sonnet-gateway  :target "crates/missiond-daemon/src/llm/sonnet_gateway.rs")
        (minimax-gateway :target "crates/missiond-daemon/src/llm/minimax_gateway.rs")))

    (component llm-gate
      :target "crates/missiond-daemon/src/llm/llm_gate.rs"
      (description "rate limiter + 429 backoff + quota tracking"))

    (component gemini-driver  :target "crates/missiond-daemon/src/llm/gemini_driver.rs")
    (component gemini-file-api :target "crates/missiond-daemon/src/llm/gemini_file_api.rs")
    (component gemini-cli     :target "crates/missiond-daemon/src/llm/gemini_cli.rs")
    (component codex-cli      :target "crates/missiond-daemon/src/llm/codex_cli.rs")
    (component prompts        :target "crates/missiond-daemon/src/llm/prompts.rs")

    (component context-pipeline
      :target "crates/missiond-daemon/src/context/context_pipeline.rs"
      (source-priority
        slot-env -> skill-context -> kb-entries -> conversation-history
        -> topology-map -> claude-md)
      (depends (db slot_manager)))

    (component context-budget  :target "crates/missiond-daemon/src/context/context_budget.rs")
    (component slot-env        :target "crates/missiond-daemon/src/context/slot_env.rs")
    (component topology-map    :target "crates/missiond-daemon/src/context/topology_map.rs")
    (component claude-md-sync  :target "crates/missiond-daemon/src/context/claude_md_sync.rs"))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 10: MCP DISPATCH — 60+ TOOLS
  ;; ══════════════════════════════════════════════════════
  (pillar mcp-dispatch
    (purpose "JSON-RPC stdio server → handler dispatch across 4 domains")

    (component mcp-server
      :target "crates/missiond-mcp/src/bin/mission-mcp.rs"
      (gateway-impl :target "crates/missiond-mcp/src/gateway_impl.rs")
      (gen-gateway  :target "crates/missiond-mcp/src/gen_gateway.rs"))

    (component compute-tools
      :target "crates/missiond-mcp/src/tools/compute/"
      (tool mission_pty_spawn      :handler "handlers::compute::pty")
      (tool mission_pty_send       :handler "handlers::compute::pty")
      (tool mission_pty_read       :handler "handlers::compute::pty")
      (tool mission_pty_screenshot :handler "handlers::compute::pty")
      (tool mission_pty_status     :handler "handlers::compute::pty")
      (tool mission_pty_signal     :handler "handlers::compute::pty")
      (tool mission_pty_confirm    :handler "handlers::compute::pty")
      (tool mission_task_submit    :handler "handlers::compute::task")
      (tool mission_task_query     :handler "handlers::compute::task")
      (tool mission_task_cancel    :handler "handlers::compute::task")
      (tool mission_task_delegate  :handler "handlers::compute::task_delegate")
      (tool mission_compute_slot   :handler "handlers::compute::compute_slot")
      (tool mission_slots          :handler "handlers::compute::slot")
      (tool mission_worker         :handler "handlers::compute::worker"
        ;; commit e18d0bf: handle_control 新增 target_type="project" → ControlManager.set_project(id, paused)
        ;; MCP schema enum: ["global","provider","domain","worker","slot_role","project"]
        )
      (tool mission_job_poll       :handler "handlers::compute::job")
      (tool mission_sonnet_process :handler "handlers::compute::process")
      (tool mission_minimax_process :handler "handlers::compute::minimax")
      (tool mission_agent          :handler "handlers::compute::cc_tasks"))

    (component knowledge-tools
      :target "crates/missiond-mcp/src/tools/knowledge/"
      (tool mission_kb_query       :handler "handlers::knowledge::kb")
      (tool mission_kb_mutate      :handler "handlers::knowledge::kb")
      (tool mission_kb_ops         :handler "handlers::knowledge::kb")
      (tool mission_kb_remember    :handler "handlers::knowledge::kb"
        :params-added "project: Option<String> — 所属项目 ID (commit 3c10d21)")
      ;; commit 3c10d21: 批量设置 KB 条目项目归属
      (tool mission_kb_batch_set_project :handler "handlers::knowledge::kb"
        :added "3c10d21"
        :params (assignments :"Vec<{key, project_id?}>")
        :returns "{updated, not_found, total}"
        :doc "一次性分配多条 KB 条目到指定项目; project_id 为空/null 表示清除归属(设为全局)")
      (tool mission_memory         :handler "handlers::knowledge::memory")
      (tool mission_board_query    :handler "handlers::knowledge::board")
      (tool mission_board_create   :handler "handlers::knowledge::board")
      (tool mission_board_update   :handler "handlers::knowledge::board")
      (tool mission_board_delete   :handler "handlers::knowledge::board")
      (tool mission_board_claim    :handler "handlers::knowledge::board")
      (tool mission_board_retry    :handler "handlers::knowledge::board")
      (tool mission_board_note_add :handler "handlers::knowledge::board")
      (tool mission_board_decompose :handler "handlers::knowledge::board")
      (tool mission_cc_query       :handler "handlers::knowledge::cascade")
      (tool mission_cc_swarm       :handler "handlers::knowledge::cascade")
      (tool mission_skill_query    :handler "handlers::knowledge::skill")
      (tool mission_skill_exec     :handler "handlers::knowledge::skill")
      (tool mission_skill_mutate   :handler "handlers::knowledge::skill")
      (tool mission_skill_context  :handler "handlers::knowledge::skill")
      (tool mission_insight        :handler "handlers::knowledge::insight")
      (tool mission_embedding_ops  :handler "handlers::knowledge::kb")
      (tool mission_code_search    :handler "handlers::knowledge::kb")
      (tool mission_universe_graph :handler "handlers::knowledge::cascade")
      (tool mission_cascade_plan   :handler "handlers::knowledge::cascade")
      (tool mission_cascade_trigger :handler "handlers::knowledge::cascade")
      (tool mission_cascade_lint   :handler "handlers::knowledge::cascade")
      ;; commit ec269d7: read project intent.lisp from MCP
      (tool mission_intent         :handler "handlers::knowledge::intent"
        :added "ec269d7"
        :actions (read section summary list)
        :doc "read / query .missiond/intent.lisp for a registered project; fuzzy project lookup (substring match, single match auto-resolves, multiple matches return 'ambiguous' error listing candidates)"
        (action read    :params (project :required section :optional)  :returns "raw lisp content")
        (action section :params (project :required section-name :required) :returns "matching s-expression block")
        (action summary :params (project :required)                    :returns "high-level summary from intent")
        (action list    :params ()                                      :returns "Vec<{project_id, intent_path, exists}>")
        :candidates (".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"))

      ;; P4+P5 (commit 76900d1): mission_project — 项目管理 MCP 工具
      ;; commit 84ac1a6: 新增 init action; list 响应增加 lispFiles/lispCount
      (tool mission_project        :handler "handlers::knowledge::project"
        :actions (list get set_active sync init)
        :doc "init: 一步注册新项目(path→git-remote→intent-scan→upsert+backfill+reload); list: 列出所有项目(附带lispFiles/lispCount); get: 按id获取; set_active: 设置活跃状态; sync: 扫描 ~/.claude/projects/ 自动发现注册"
        (action init
          :params (path :required id :optional slots :optional)
          :steps ("canonicalize path" "derive id from dir name (or param)" "git remote get-url origin → github_url" "scan .missiond/.jarvis/ intent.lisp" "upsert project" "backfill_project_id(path%) + backfill_project_id(claude-encoded-pattern%)" "reload SharedProjectRegistry")
          :returns (id path githubUrl intentPath backfilledConversations status="registered")
          :added "84ac1a6")))

    (component comm-tools
      :target "crates/missiond-mcp/src/tools/comm/"
      (tool mission_conversation_query     :handler "handlers::comm::conversation")
      (tool mission_conversation_analyze   :handler "handlers::comm::conversation")
      (tool mission_conversation_reconcile :handler "handlers::comm::conversation")
      (tool mission_question               :handler "handlers::comm::question")
      (tool mission_router_chat            :handler "handlers::comm::router_chat")
      (tool mission_router_chat_manage     :handler "handlers::comm::router_chat")
      (tool mission_timeline               :handler "handlers::comm::timeline")
      (tool mission_audit                  :handler "handlers::comm::audit")
      (tool mission_retrospective_manage   :handler "handlers::comm::retrospective")
      (tool mission_llm_trace              :handler "handlers::comm::audit")
      (tool mission_decision_stats         :handler "handlers::comm::conversation")
      (tool mission_slot_history           :handler "handlers::comm::timeline")
      (tool mission_beacon                 :handler "handlers::comm::audit")
      ;; commit ec269d7: new Codex CLI integration tool
      (tool mission_codex_ops             :handler "handlers::comm::codex_ops"
        :added "ec269d7"
        :actions (query history stats)
        :source "~/.codex/state_5.sqlite (via CodexIngestionWorker)"
        :doc "query Codex CLI operation history ingested into MissionD conversation timeline"))

    (component sysinfra-tools
      :target "crates/missiond-mcp/src/tools/sysinfra/"
      (tool mission_control          :handler "handlers::sysinfra::system")
      (tool mission_daemon_update    :handler "handlers::sysinfra::system")
      (tool mission_sys_config       :handler "handlers::sysinfra::system")
      (tool mission_sys_logs         :handler "handlers::sysinfra::system")
      (tool mission_infra_ops        :handler "handlers::sysinfra::infra")
      (tool mission_infra_query      :handler "handlers::sysinfra::infra")
      (tool mission_permission_query :handler "handlers::sysinfra::permission")
      (tool mission_permission_mutate :handler "handlers::sysinfra::permission")
      (tool mission_power_control    :handler "handlers::sysinfra::power")
      (tool mission_pause            :handler "handlers::sysinfra::misc")
      (tool mission_inbox            :handler "handlers::sysinfra::misc")
      (tool mission_incident         :handler "handlers::sysinfra::misc")
      (tool mission_submit_phase_result :handler "handlers::sysinfra::misc")
      (tool mission_gemini_auth      :handler "handlers::sysinfra::misc")))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 11: TRANSPORT & BOOTSTRAP
  ;; ══════════════════════════════════════════════════════
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
        )
      (module session-util      :target "crates/missiond-daemon/src/infra/session_util.rs")))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 12: STANDALONE SERVICES & FRONTENDS
  ;; ══════════════════════════════════════════════════════
  (pillar standalone
    (purpose "independently deployable services and client packages")

    (component skill-store
      :target "crates/skill-store/src/main.rs"
      (routes
        (auth          :target "crates/skill-store/src/routes/auth.rs")
        (skills        :target "crates/skill-store/src/routes/skills.rs")
        (invoke        :target "crates/skill-store/src/routes/invoke.rs")
        (creator       :target "crates/skill-store/src/routes/creator.rs")
        (subscriptions :target "crates/skill-store/src/routes/subscriptions.rs"))
      (services
        (billing   :target "crates/skill-store/src/services/billing.rs")
        (defense   :target "crates/skill-store/src/services/defense.rs")
        (executor  :target "crates/skill-store/src/services/executor.rs")
        (llm-proxy :target "crates/skill-store/src/services/llm_proxy.rs")))

    (component missiond-attach
      :target "crates/missiond-attach/src/main.rs")

    (component missiond-runner
      :target "crates/missiond-runner/src/runner.rs")

    (component semantic-terminal-napi
      :target "crates/semantic-terminal-napi/src/lib.rs")

    (component board-frontend
      :target "packages/board/src/App.tsx"
      (api-routes :target "packages/board/src/app/api/"
        (route "/api/slots"            :handler "slots/route.ts")
        (route "/api/tasks"            :handler "tasks/route.ts")
        (route "/api/conversations"    :handler "conversations/route.ts"
          :params (status limit conversationType source project)
          :note "project参数透传至mission_conversation_list; commit 5671c95")
        (route "/api/kb"               :handler "kb/route.ts"
          :methods (GET DELETE PATCH)
          :params (query category project limit)
          :note "project参数透传至mission_kb_query/mission_kb_list; commit 5671c95"
          (method PATCH
            :added "3c10d21"
            :body "{key, project_id}"
            :action "kb_update(key, project_id) — 空串清除归属"
            :returns "ToolResult from mission_kb_update"))
        (route "/api/projects"         :handler "projects/route.ts"
          :method GET :calls "mission_project{action:list}"
          :returns "Vec<ProjectInfo>(含lispFiles/lispCount/githubUrl)"
          :added "eae9bbd")
        (route "/api/questions"        :handler "questions/route.ts")
        (route "/api/timeline/events"  :handler "timeline/events/route.ts")
        (route "/api/architecture"     :handler "architecture/route.ts")
        (route "/api/pty/spawn"        :handler "pty/spawn/route.ts")
        (route "/api/pty/screen"       :handler "pty/screen/route.ts")
        (route "/api/pty/confirm"      :handler "pty/confirm/route.ts")
        (route "/api/system/health"    :handler "system/health/route.ts")
        (route "/api/system/llm-traces" :handler "system/llm-traces/route.ts")
        (route "/api/deploy/status"    :handler "deploy/status/route.ts"))

      (ui-components :target "packages/board/src/components/"
        (component Conversations
          :target "packages/board/src/components/Conversations.tsx"
          (state projectFilter :type string :default "all")
          (state projects      :type "Project[]" :fetched-from "/api/projects")
          (widget project-filter-select
            :trigger onMount+onChange :passes "project" to "/api/conversations"
            :shows active-projects-sorted-by-conversation_count
            :added "5671c95")
          (note "viewMode union type 新增 jarvis — fix pre-existing build error (commit ac96b3c)"))

        (component KnowledgeBase
          :target "packages/board/src/components/KnowledgeBase.tsx"
          (state activeProject :type "string|null" :default nil)
          (state projects      :type "Project[]" :fetched-from "/api/projects")
          (widget project-filter-pills
            :style pill-buttons :separator divider-line
            :passes "project" to "/api/kb"
            :added "5671c95"
            :note "commit 3c10d21: '__unclassified__' pill为客户端过滤(不传后端), filter: !e.projectId")
          (widget per-entry-project-selector
            :added "3c10d21"
            :location "KBEntryCard — 每张卡片底部 meta 行"
            :type "<select> 下拉"
            :options ("未分类(value='')" "各 ProjectConfig.id")
            :style "无项目: border-neutral-700/text-neutral-500; 有项目: border-cyan-500/30 text-cyan-400"
            :interaction "onChange → handleSetProject(key, projectId|null) → PATCH /api/kb → 乐观更新state; 失败时fetchEntries回滚"))

        (component SystemDashboard
          :target "packages/board/src/components/SystemDashboard.tsx"
          (sub-component ProjectsPanel
            :target "packages/board/src/components/SystemDashboard.tsx"
            :fetches "/api/projects"
            :layout "grid 1/2/3 columns responsive"
            :sort "active first → lispCount desc → alphabetical"
            :card-fields (id path-abbreviated active-badge githubUrl-link slots lisp-files-tags)
            :note "githubUrl: git@github.com:X/Y.git → https://github.com/X/Y 转换"
            :added "eae9bbd")
          :note "ProjectsPanel 置于 MemoryDashboard 之前(最顶部折叠区); collapsed 默认展开")))

    (component node-client
      :target "packages/node-client/src/index.ts"
      (modules
        (client :target "packages/node-client/src/client.ts")
        (daemon :target "packages/node-client/src/daemon.ts")
        (binary :target "packages/node-client/src/binary.ts")
        (pty    :target "packages/node-client/src/pty.ts")
        (types  :target "packages/node-client/src/types.ts"))))

  ;; ══════════════════════════════════════════════════════
  ;; FLOWS
  ;; ══════════════════════════════════════════════════════
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
        -> return{id path githubUrl intentPath backfilledConversations status}))))