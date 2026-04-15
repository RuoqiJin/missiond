;; MissionD — Pillar: db-core
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

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
          :added "84ac1a6")
        ;; commit 8438a7d: Project Context Aggregator — 3 new aggregate queries
        (conversation_stats_by_project :binds project_id :returns "DbResult<serde_json::Value>"
          :doc "conversation count, status distribution, date range for a project" :added "8438a7d")
        (recent_conversations_by_project :binds (project_id limit) :returns "DbResult<Vec<serde_json::Value>>"
          :doc "most recent N conversations for a project" :added "8438a7d")
        (kb_stats_by_project :binds project_id :returns "DbResult<serde_json::Value>"
          :doc "KB entry count grouped by category for a project" :added "8438a7d"))

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
        (op compaction-fragments (binds id))
        ;; commit 43c80f4: reconciliation 查询
        (op sessions-recently-active-without-turns (binds since_minutes limit)
          :note "返回近 N 分钟内有 conversation_messages 但无 turns 记录的 session_id 列表; PG 实现; SQLite stub(返回空)"))

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
        (op insert-batch (binds "Vec<message>")
          ;; commit 43c80f4: insert_conversation_messages_batch trait method — CodexIngestionWorker 用于批量写入 text messages
          )
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

