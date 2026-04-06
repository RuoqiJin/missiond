;; ══════════════════════════════════════════════════════════════
;; MissionD DB Layer — Audit & Retrospective (SQLite)
;; conversation_tool_calls, conversation_events, retrospective_results
;; ══════════════════════════════════════════════════════════════

(pattern crud-gateway
  :gateway MissionDB
  :pool    "Mutex<Connection>"
  :backend sqlite
  :target  "crates/missiond-core/src/db/gen_audit.rs"

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: conversation_tool_calls                           │
  ;; └──────────────────────────────────────────────────────────┘
  (table conversation_tool_calls
    (col id             :type text    :pk)
    (col session_id     :type text    :fk conversations :on-delete cascade)
    (col message_id     :type bigint  :nullable)
    (col tool_name      :type text)
    (col input_summary  :type text    :nullable)
    (col raw_input      :type text    :nullable)
    (col output_summary :type text    :nullable)
    (col raw_output     :type text    :nullable)
    (col status         :type text    :default "pending")
    (col duration_ms    :type integer :nullable)
    (col timestamp      :type text)

    (idx idx_tc_session :cols (session_id))
    (idx idx_tc_name    :cols (tool_name))
    (idx idx_tc_status  :cols (status))

    (row-type ToolCallRow
      :convert row_to_tool_call :to ToolCallRecord)

    (op insert_tool_call
      :kind insert-struct
      :params ((tc "&ToolCallRecord"))
      :returns "()")

    (op insert_tool_calls_batch
      :kind custom
      :params ((calls "&[ToolCallRecord]"))
      :returns "usize"
      :logic "transaction loop INSERT OR IGNORE")

    (op update_tool_call_output
      :kind update
      :where ((id text))
      :set ((output_summary "&str") (raw_output "&str") (status "&str"))
      :returns "bool")

    (op get_tool_call_by_id
      :kind select-one
      :where ((id text))
      :returns "Option<ToolCallRecord>")

    (op get_tool_calls_by_session
      :kind custom
      :where ((session_id "&str"))
      :params ((tool_filter "Option<&[String]>") (limit "i64"))
      :returns "Vec<ToolCallRecord>"
      :logic "optional tool_name IN filter, ORDER BY timestamp ASC")

    (op get_tool_call_stats
      :kind custom
      :where ((session_id "&str"))
      :returns "Vec<(String, i64, i64, i64)>"
      :logic "GROUP BY tool_name: total, completed, avg_duration")

    (op count_pending_tool_calls
      :kind count
      :where-fixed ("status = 'pending'")
      :returns "i64")

    (op get_sessions_with_pending_tool_calls
      :kind custom
      :returns "Vec<String>"
      :logic "SELECT DISTINCT session_id WHERE status=pending")

    (op get_sessions_with_tool_calls
      :kind custom
      :returns "std::collections::HashSet<String>"
      :logic "SELECT DISTINCT session_id")

    (op get_messages_for_tool_call_backfill
      :kind custom
      :where ((session_id "&str"))
      :returns "Vec<(String, String, String)>"
      :logic "SELECT message_uuid, role, content for tool_use/tool_result messages")

    (op get_tool_calls_for_detailed_analysis
      :kind custom
      :where ((session_id "&str"))
      :returns "Vec<(String, String, String, String)>"
      :logic "tool_name, status, input_summary, output_summary")

    (op get_tool_calls_with_status_timeline
      :kind custom
      :where ((session_id "&str"))
      :returns "Vec<(String, String, String, String)>"
      :logic "tool_name, status, timestamp, duration_ms timeline")

    (op get_tool_name_sequence
      :kind custom
      :where ((session_id "&str"))
      :returns "Vec<String>"
      :logic "SELECT tool_name ORDER BY timestamp ASC")

    (op get_tool_error_samples
      :kind custom
      :params ((session_id "&str") (tool_name "&str"))
      :returns "Vec<(String, String, String)>"
      :logic "failed tool calls: input, output, raw_output samples"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: conversation_events                               │
  ;; └──────────────────────────────────────────────────────────┘
  (table conversation_events
    (col id         :type bigint  :pk)
    (col session_id :type text    :fk conversations :on-delete cascade)
    (col event_type :type text)
    (col content    :type text    :nullable)
    (col raw_data   :type text    :nullable)
    (col timestamp  :type text)
    (col event_uuid :type text    :nullable)

    (idx idx_conv_event_session :cols (session_id))
    (idx idx_conv_event_type    :cols (event_type))

    (op insert_conversation_events_batch
      :kind custom
      :params ((events "&[ConversationEvent]"))
      :returns "usize"
      :logic "transaction loop INSERT OR IGNORE with dedup on (session_id, event_uuid)")

    (op get_conversation_events
      :kind custom
      :params ((session_id "&str") (event_type "Option<&str>") (limit "i64") (before "Option<&str>") (after "Option<&str>"))
      :returns "Vec<ConversationEvent>"
      :logic "dynamic WHERE with optional type/time filters")

    (op is_compact_boundary_event
      :kind custom
      :params ((session_id "&str") (event_uuid "&str"))
      :returns "bool"
      :logic "EXISTS check for compact_boundary event")

    (op get_agent_trajectory
      :kind custom
      :where ((session_id "&str"))
      :returns "Vec<serde_json::Value>"
      :logic "SELECT events + tool_calls interleaved, ORDER BY timestamp")

    (op get_event_type_summary
      :kind custom
      :where ((session_id "&str"))
      :returns "Vec<(String, i64, String, String)>"
      :logic "GROUP BY event_type: count, first, last")

    (op cleanup_old_events
      :kind custom
      :params ((cutoff "&str"))
      :returns "usize"
      :logic "DELETE WHERE created_at < cutoff for completed sessions")

    (op get_sessions_with_events
      :kind custom
      :returns "std::collections::HashSet<String>"
      :logic "SELECT DISTINCT session_id"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Conversation lifecycle ops (cross-table on conversations)│
  ;; └──────────────────────────────────────────────────────────┘
  (table conversations
    (col id :type text :pk)

    (op get_conversations_with_jsonl
      :kind custom
      :returns "Vec<(String, String)>"
      :logic "SELECT id, jsonl_path WHERE jsonl_path IS NOT NULL")

    (op mark_conversation_analyzed
      :kind update
      :where ((id text))
      :set ((analyzed_at :via "datetime('now')"))
      :returns "()")

    (op get_unanalyzed_conversations
      :kind custom
      :returns "Vec<Conversation>"
      :logic "WHERE analyzed_at IS NULL AND status=completed")

    (op complete_conversation
      :kind update
      :where ((id text))
      :set ((status :literal "'completed'")
            (ended_at :via "datetime('now')")
            (updated_at :via "datetime('now')"))
      :returns "()")

    (op save_conversation_exit_code
      :kind update
      :where ((id text))
      :set ((exit_code "i32")
            (updated_at :via "datetime('now')"))
      :returns "()")

    (op complete_stale_conversations
      :kind custom
      :params ((cutoff "&str"))
      :returns "usize"
      :logic "UPDATE active → completed WHERE updated_at < cutoff AND slot active")

    (op mark_conversation_compacted
      :kind update
      :where ((id text))
      :set ((status :literal "'compacted'")
            (updated_at :via "datetime('now')"))
      :returns "()")

    (op set_conversation_task_id
      :kind update
      :where ((id text))
      :set ((task_id "&str"))
      :returns "()")

    (op get_conversations_by_task_id
      :kind select-many
      :where ((task_id text))
      :returns "Vec<Conversation>")

    (op reactivate_conversation
      :kind custom
      :where ((id "&str"))
      :returns "usize"
      :logic "UPDATE status=active, clear ended_at WHERE completed/compacted")

    ;; ── Memory/UserVoice/Realtime forwarding ──

    (op get_pending_memory_messages
      :kind custom
      :params ((today "&str"))
      :returns "Vec<(String, String, Vec<ConversationMessage>)>"
      :logic "sessions with unforwarded messages, today only, grouped by session")

    (op update_memory_forwarded_at
      :kind update
      :where ((session_id text))
      :set ((memory_forwarded_at "&str"))
      :returns "()")

    (op get_pending_user_voice_messages
      :kind custom
      :returns "Vec<(String, String, Vec<ConversationMessage>)>"
      :logic "sessions with user_voice_forwarded_at IS NULL, grouped by session")

    (op update_user_voice_forwarded_at
      :kind update
      :where ((session_id text))
      :set ((user_voice_forwarded_at "&str"))
      :returns "()")

    (op get_pending_realtime_messages
      :kind custom
      :returns "Vec<(String, String, Vec<ConversationMessage>)>"
      :logic "sessions with unforwarded realtime messages")

    (op get_pending_realtime_messages_with_limit
      :kind custom
      :params ((limit "usize"))
      :returns "Vec<(String, String, Vec<ConversationMessage>)>"
      :logic "same as above with LIMIT on sessions")

    (op update_realtime_forwarded_at
      :kind update
      :where ((session_id text))
      :set ((realtime_forwarded_at "&str"))
      :returns "()"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: retrospective_results                             │
  ;; └──────────────────────────────────────────────────────────┘
  (table retrospective_results
    (col session_id      :type text  :pk)
    (col trigger_reason  :type text)
    (col quick_stats     :type text)
    (col full_analysis   :type text  :nullable)
    (col created_at      :type timestamptz :default-now)

    (idx idx_retro_created :cols (created_at))

    (op save_retrospective_result
      :kind upsert
      :params ((session_id text) (trigger_reason text) (quick_stats text) (full_analysis text) (created_at text))
      :on-conflict "(session_id) DO UPDATE SET trigger_reason = ?2, quick_stats = ?3, full_analysis = ?4"
      :returns "()")

    (op has_retrospective_result
      :kind count
      :where ((session_id text))
      :returns "bool")

    (op get_retrospective_result
      :kind custom
      :where ((session_id "&str"))
      :returns "Option<(String, String, Option<String>, String)>"
      :logic "SELECT trigger_reason, quick_stats, full_analysis, created_at")

    (op list_retrospective_results
      :kind custom
      :params ((limit "i64"))
      :returns "Vec<(String, String, String, Option<String>, String)>"
      :logic "SELECT session_id, trigger_reason, quick_stats, full_analysis, created_at")

    (op get_sessions_needing_retrospective
      :kind custom
      :returns "Vec<(String, i64, i64, f64)>"
      :logic "completed sessions without retro, with tool call stats")

    (op get_sessions_for_retro_backfill
      :kind custom
      :params ((since "&str") (force "bool"))
      :returns "Vec<(String, i64, i64, f64)>"
      :logic "same as above but with since filter and optional force")

    ;; Retrospective analysis helpers (cross-table queries)
    (op get_retrospective_tool_stats
      :kind custom
      :params ((session_id "&str") (limit "i64"))
      :returns "Vec<(String, i64, i64, i64, f64)>"
      :logic "tool_name, total, completed, failed, avg_duration_ms from tool_calls")

    (op get_retrospective_meta
      :kind custom
      :where ((session_id "&str"))
      :returns "(i64, i64, i64, i64)"
      :logic "message_count, tool_count, user_msgs, assistant_msgs")

    (op get_retrospective_repeat_patterns
      :kind custom
      :params ((session_id "&str") (min_streak "i64"))
      :returns "Vec<(String, i64, String, String)>"
      :logic "detect consecutive same-tool streaks in tool_calls")

    (op get_retrospective_high_error_tools
      :kind custom
      :params ((session_id "&str") (min_error_rate "f64"))
      :returns "Vec<(String, f64, i64)>"
      :logic "tool_name, error_rate, total_calls above threshold")

    (op get_first_user_message
      :kind custom
      :where ((session_id "&str"))
      :returns "Option<String>"
      :logic "SELECT content FROM messages WHERE role=user ORDER BY id ASC LIMIT 1")))
