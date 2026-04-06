;; ══════════════════════════════════════════════════════════════
;; MissionD DB Layer — Conversations + Messages (SQLite)
;; ══════════════════════════════════════════════════════════════

(pattern crud-gateway
  :gateway MissionDB
  :pool    "Mutex<Connection>"
  :backend sqlite
  :target  "crates/missiond-core/src/db/gen_conversation.rs"

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: conversations                                     │
  ;; └──────────────────────────────────────────────────────────┘
  (table conversations
    (col id                     :type text        :pk)
    (col project                :type text        :nullable)
    (col slot_id                :type text        :nullable)
    (col source                 :type text        :default "claude_cli")
    (col model                  :type text        :nullable)
    (col git_branch             :type text        :nullable)
    (col jsonl_path             :type text        :nullable)
    (col message_count          :type bigint      :default "0")
    (col started_at             :type text)
    (col ended_at               :type text        :nullable)
    (col status                 :type text        :default "active")
    (col analyzed_at            :type text        :nullable)
    (col memory_forwarded_at    :type text        :nullable)
    (col user_voice_forwarded_at :type text       :nullable)
    (col realtime_forwarded_at  :type text        :nullable)
    (col analysis_version       :type integer     :default "0")
    (col analysis_retries       :type integer     :default "0")
    (col parent_session_id      :type text        :nullable)
    (col task_id                :type text        :nullable)
    (col deep_analyzed_message_id :type bigint    :default "0")
    (col chat_type              :type text        :nullable :default "pty")
    (col conversation_type      :type text        :default "user")
    (col updated_at             :type text        :nullable)
    (col llm_summary            :type text        :nullable)
    (col embedding_provider     :type text        :nullable)
    (col embedding              :type text        :nullable)
    (col session_timeline       :type text        :nullable)
    (col timeline_built_at      :type text        :nullable)
    (col exit_code              :type integer     :nullable)
    (col habit_scanned_at       :type text        :nullable)
    (col rolling_summary        :type text        :nullable)
    (col last_summarized_msg_id :type bigint      :default "0")

    (idx idx_conv_status       :cols (status))
    (idx idx_conv_task_id      :cols (task_id))
    (idx idx_conv_parent       :cols (parent_session_id))
    (idx idx_conv_type         :cols (conversation_type))
    (idx idx_conv_slot         :cols (slot_id))

    (row-type ConversationRow
      :convert row_to_conversation :to Conversation)

    ;; ── Mechanical CRUD ──

    (op upsert_conversation
      :kind upsert
      :params ((id text) (project text) (slot_id text) (source text) (model text)
               (git_branch text) (jsonl_path text) (parent_session_id text)
               (task_id text) (message_count "i64") (started_at text) (ended_at text)
               (status text) (analyzed_at text) (conversation_type text))
      :on-conflict "(id) DO UPDATE SET slot_id = COALESCE(?3, slot_id), model = COALESCE(?5, model), git_branch = COALESCE(?6, git_branch), message_count = ?10, ended_at = ?12, status = ?13, conversation_type = COALESCE(?15, conversation_type)"
      :returns "()")

    (op get_conversation
      :kind select-one
      :where ((id text))
      :returns "Option<Conversation>")

    (op get_child_conversations
      :kind select-many
      :where ((parent_session_id text))
      :returns "Vec<Conversation>")

    (op mark_analysis_complete
      :kind update
      :where ((id text))
      :set ((analyzed_at :via "datetime('now')")
            (analysis_version "i32")
            (analysis_retries :literal "0")
            (deep_analyzed_message_id :literal "0"))
      :returns "()")

    (op update_deep_checkpoint
      :kind update
      :where ((id text))
      :set ((deep_analyzed_message_id "i64"))
      :returns "()")

    (op mark_analysis_failed
      :kind update
      :where ((id text))
      :set ((analysis_retries :literal "analysis_retries + 1"))
      :returns "()")

    (op mark_habit_scanned
      :kind update
      :where ((id text))
      :set ((habit_scanned_at :via "datetime('now')"))
      :returns "()")

    (op set_conversation_summary
      :kind update
      :where ((id text))
      :set ((llm_summary "&str"))
      :returns "()")

    (op clear_conversation_summary
      :kind update
      :where ((id text))
      :set ((llm_summary :literal "NULL")
            (embedding :literal "NULL")
            (embedding_provider :literal "NULL"))
      :returns "()")

    (op set_conversation_embedding
      :kind custom
      :where ((id "&str"))
      :params ((embedding "&[f32]") (provider "&str"))
      :returns "()"
      :logic "UPDATE SET embedding = blob, embedding_provider WHERE id")

    ;; ── Analysis pipeline queries ──

    (op get_pending_deep_analysis
      :kind custom
      :params ((current_version "i32") (max_retries "i32"))
      :returns "Vec<Conversation>"
      :logic "3-status UNION: completed + compacted + active with version/retry checks")

    (op has_pending_deep_analysis
      :kind custom
      :params ((current_version "i32") (max_retries "i32"))
      :returns "bool"
      :logic "EXISTS check for pending deep analysis")

    (op count_pending_deep_analysis
      :kind custom
      :params ((current_version "i32") (max_retries "i32"))
      :returns "i64"
      :logic "COUNT of pending deep analysis conversations")

    (op pending_deep_detail
      :kind custom
      :params ((current_version "i32") (max_retries "i32"))
      :returns "Vec<(String, String, i32)>"
      :logic "per-conversation pending summary (id, ended_at, retries)")

    (op count_pending_realtime
      :kind custom
      :returns "i64"
      :logic "COUNT DISTINCT sessions with pending realtime messages")

    (op pending_realtime_detail
      :kind custom
      :returns "Vec<(String, i64, String)>"
      :logic "per-session realtime summary (session_id, msg_count, oldest_ts)")

    ;; ── Habit scan ──

    (op get_unscanned_conversations
      :kind custom
      :params ((limit "usize"))
      :returns "Vec<Conversation>"
      :logic "WHERE habit_scanned_at IS NULL AND type=user, oldest first")

    (op count_unscanned_conversations
      :kind custom
      :returns "i64"
      :logic "COUNT unscanned user conversations")

    (op count_scannable_conversations
      :kind custom
      :returns "i64"
      :logic "COUNT total user conversations eligible for scan")

    ;; ── Complex queries ──

    (op list_conversations
      :kind custom
      :params ((status "Option<&str>") (limit "i64") (conv_type "Option<&str>")
               (task_id "Option<&str>") (since "Option<&str>") (until "Option<&str>")
               (source "Option<&str>"))
      :returns "Vec<Conversation>"
      :logic "dynamic SQL with 7 optional filters, 2-phase fetch (active vs non-active)")

    ;; ── Embedding/Summary/Topic queries ──

    (op load_conversation_embeddings
      :kind custom
      :params ((provider "&str"))
      :returns "Vec<(String, Vec<f32>)>"
      :logic "SELECT id, embedding WHERE provider matches, deserialize blobs")

    (op conversations_missing_summary
      :kind custom
      :params ((limit "i64"))
      :returns "Vec<String>"
      :logic "IDs missing LLM summary, user/worker types only")

    (op conversations_stale_embedding
      :kind custom
      :params ((current_provider "&str") (limit "i64"))
      :returns "Vec<String>"
      :logic "IDs with embeddings from different provider")

    (op set_conversation_topic_vectors
      :kind custom
      :where ((session_id "&str"))
      :params ((topics "&[(String, Vec<f32>)]") (provider "&str"))
      :returns "()"
      :logic "DELETE old + INSERT new topic vectors for session")

    (op load_conversation_topic_vectors
      :kind custom
      :params ((provider "&str"))
      :returns "Vec<(String, Vec<Vec<f32>>)>"
      :logic "GROUP BY session_id, reconstruct vectors from blobs")

    (op get_conversation_topics
      :kind custom
      :where ((session_id "&str"))
      :returns "Vec<String>"
      :logic "SELECT topic summaries for session")

    (op conversations_needing_topic_vectors
      :kind custom
      :params ((provider "&str") (limit "i64"))
      :returns "Vec<String>"
      :logic "sessions with old single-vector but no topic vectors")

    ;; ── Session Timeline ──

    (op conversations_needing_timeline
      :kind custom
      :params ((limit "i64"))
      :returns "Vec<String>"
      :logic "completed parents with compaction children but no timeline")

    (op get_compaction_fragments
      :kind custom
      :where ((parent_id "&str"))
      :returns "Vec<(String, String, i64)>"
      :logic "ordered compaction fragments (id, started_at, msg_count)")

    (op get_last_assistant_content
      :kind custom
      :where ((session_id "&str"))
      :returns "Option<String>"
      :logic "last assistant message content for compaction summary")

    (op set_session_timeline
      :kind custom
      :where ((parent_id "&str"))
      :params ((timeline_json "&str"))
      :returns "bool"
      :logic "CAS write: only if timeline_built_at IS NULL"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: conversation_messages                             │
  ;; └──────────────────────────────────────────────────────────┘
  (table conversation_messages
    (col id               :type bigint    :pk)
    (col session_id       :type text      :fk conversations :on-delete cascade)
    (col role             :type text)
    (col content          :type text)
    (col raw_content      :type text      :nullable)
    (col message_uuid     :type text      :nullable)
    (col parent_uuid      :type text      :nullable)
    (col model            :type text      :nullable)
    (col timestamp        :type text)
    (col metadata         :type text      :nullable)
    (col tool_name        :type text      :nullable)
    (col raw_role         :type text      :nullable)
    (col content_types    :type text      :nullable)
    (col has_image        :type boolean   :default "0")
    (col has_tool_use     :type boolean   :default "0")
    (col has_tool_result  :type boolean   :default "0")
    (col token_count      :type integer   :nullable)

    (unique (message_uuid))
    (idx idx_conv_msg_session   :cols (session_id))
    (idx idx_conv_msg_session_id :cols (session_id id))
    (idx idx_conv_msg_timestamp :cols (timestamp))
    (idx idx_conv_msg_uuid      :cols (message_uuid))
    (idx idx_conv_msg_tool      :cols (tool_name))

    (row-type ConversationMessageRow
      :convert row_to_conversation_message :to ConversationMessage)

    ;; ── Mechanical CRUD ──

    (op insert_conversation_message
      :kind insert-struct
      :params ((msg "&ConversationMessage"))
      :returns "i64")

    (op get_conversation_message_by_id
      :kind select-one
      :where ((id bigint))
      :returns "Option<ConversationMessage>")

    (op get_conversation_messages
      :kind custom
      :where ((session_id "&str"))
      :params ((since_id "Option<i64>") (limit "i64"))
      :returns "Vec<ConversationMessage>"
      :logic "messages since ID or last N, with label override LEFT JOIN")

    (op insert_conversation_messages_batch
      :kind custom
      :params ((messages "&[ConversationMessage]"))
      :returns "Vec<i64>"
      :logic "transaction batch INSERT OR IGNORE, auto-update message_count")

    ;; ── Search (all complex/custom) ──

    (op get_messages_around
      :kind custom
      :params ((message_id "i64") (before "i64") (after "i64"))
      :returns "Option<(String, Vec<ConversationMessage>)>"
      :logic "CTEs for before/after/anchor with label override")

    (op search_conversation_messages
      :kind custom
      :params ((query "&str") (limit "i64"))
      :returns "Vec<ConversationMessage>"
      :logic "3-phase: AND-first FTS5 → OR fallback → LIKE fallback")

    (op search_messages_filtered
      :kind custom
      :params ((query "&str") (session_id "Option<&str>") (role "Option<&str>")
               (tool_name "Option<&str>") (time_after "Option<&str>") (limit "i64"))
      :returns "Vec<ConversationMessage>"
      :logic "dynamic WHERE with SQL-pushed filters + 3-phase FTS")

    (op search_conversation_sessions_fts
      :kind custom
      :params ((query "&str") (limit "i64"))
      :returns "Vec<(String, f64)>"
      :logic "grouped BM25 scoring with AND→OR→LIKE fallback")

    (op get_session_fts_snippets
      :kind custom
      :where ((session_id "&str"))
      :params ((query "&str") (limit "usize"))
      :returns "Vec<(String, String)>"
      :logic "FTS5 snippet() extraction with ranking")

    (op search_conversation_sessions_fts_filtered
      :kind custom
      :params ((query "&str") (limit "i64") (time_after "Option<&str>") (project "Option<&str>"))
      :returns "Vec<(String, f64)>"
      :logic "FTS5 session search with time/project filters")))
