;; ══════════════════════════════════════════════════════════════
;; MissionD DB Layer — Remaining Modules (SQLite)
;; ast, timeline, router_chat, question, gemini_log,
;; narration, dynamic_slot, incident, vision
;; ══════════════════════════════════════════════════════════════

(pattern crud-gateway
  :gateway MissionDB
  :pool    "Mutex<Connection>"
  :backend sqlite
  :target  "crates/missiond-core/src/db/gen_misc.rs"

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: ast_nodes                                         │
  ;; └──────────────────────────────────────────────────────────┘
  (table ast_nodes
    (col id               :type text    :pk)
    (col repo             :type text)
    (col file_path        :type text)
    (col node_type        :type text)
    (col name             :type text)
    (col signature        :type text)
    (col start_line       :type integer)
    (col end_line         :type integer)
    (col is_exported      :type boolean :default "0")
    (col docstring        :type text    :nullable)
    (col stub_content     :type text)
    (col calls            :type text    :nullable)
    (col embedding        :type text    :nullable)
    (col embedding_provider :type text  :nullable)

    (op ast_sync_file     :kind custom :params ((repo "&str") (file_path "&str") (commit "&str") (nodes "&[AstNodeRow]")) :returns "()" :logic "DELETE old + INSERT new + FTS sync + update ast_file_meta")
    (op ast_delete_file   :kind custom :params ((repo "&str") (file_path "&str")) :returns "usize" :logic "DELETE nodes + FTS cleanup + delete ast_file_meta")
    (op ast_search        :kind custom :params ((query "&str") (limit "usize")) :returns "Vec<AstSearchHit>" :logic "FTS5 search with snippet on name+signature+stub")
    (op ast_get_file_nodes :kind custom :params ((repo "&str") (file_path "&str")) :returns "Vec<AstNodeRow>" :logic "SELECT * WHERE repo AND file_path ORDER BY start_line")
    (op ast_find_related  :kind custom :params ((name "&str") (limit "usize")) :returns "Vec<AstSearchHit>" :logic "WHERE name LIKE OR calls LIKE OR stub LIKE")
    (op ast_get_file_commit :kind custom :params ((repo "&str") (file_path "&str")) :returns "Option<String>" :logic "SELECT commit_hash FROM ast_file_meta")
    (op ast_stats         :kind custom :returns "AstStats" :logic "COUNT by repo, node_type, file_count aggregation")
    (op ast_set_embedding :kind custom :params ((id "&str") (embedding "&[f32]") (provider "&str")) :returns "()" :logic "UPDATE embedding blob + provider")
    (op ast_find_unembedded :kind custom :params ((limit "usize")) :returns "Vec<(String, String, String)>" :logic "id, name, signature WHERE embedding IS NULL")
    (op ast_load_all_embeddings :kind custom :returns "Vec<(String, Vec<f32>)>" :logic "deserialize all ast embedding blobs")
    (op ast_search_ranked :kind custom :params ((query "&str") (limit "usize")) :returns "Vec<(String, usize)>" :logic "FTS5 ranked search returning (id, rank)")
    (op ast_get_search_hit :kind custom :params ((node_id "&str")) :returns "Option<AstSearchHit>" :logic "SELECT * with struct mapping")
    (op ast_get_node      :kind custom :params ((node_id "&str")) :returns "Option<AstNodeRow>" :logic "SELECT * WHERE id")
    (op ast_module_summaries :kind custom :params ((repo "&str")) :returns "Vec<ModuleAstSummary>" :logic "GROUP BY file_path: name list, function/struct counts"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: ast_file_meta                                     │
  ;; └──────────────────────────────────────────────────────────┘
  (table ast_file_meta
    (col repo        :type text)
    (col file_path   :type text)
    (col commit_hash :type text)
    (col node_count  :type integer :default "0")
    (col updated_at  :type timestamptz :default-now)
    (pk (repo file_path)))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: system_timeline                                   │
  ;; └──────────────────────────────────────────────────────────┘
  (table system_timeline
    (col seq            :type bigint  :pk)
    (col trace_id       :type text    :nullable)
    (col span_id        :type text    :nullable)
    (col parent_span_id :type text    :nullable)
    (col event_type     :type text)
    (col summary        :type text    :nullable)
    (col payload        :type text)
    (col created_at     :type timestamptz :default-now)

    (idx idx_tl_created :cols (created_at))
    (idx idx_tl_type    :cols (event_type))
    (idx idx_tl_trace   :cols (trace_id))
    (idx idx_tl_parent  :cols (parent_span_id))

    (op insert_timeline_batch :kind custom :params ((events "&[(Option<String>, Option<String>, Option<String>, String, Option<String>, String)]")) :returns "()" :logic "transaction batch INSERT")
    (op query_timeline_since  :kind custom :params ((since_seq "i64") (limit "usize")) :returns "Vec<TimelineRow>" :logic "WHERE seq > since ORDER BY seq ASC LIMIT")
    (op timeline_latest_seq   :kind custom :returns "i64" :logic "SELECT MAX(seq)")
    (op cleanup_timeline_ttl  :kind custom :params ((days "i64")) :returns "usize" :logic "DELETE WHERE created_at < N days ago")
    (op find_timeline_needing_briefing :kind custom :params ((min_content_chars "usize") (limit "usize")) :returns "Vec<(i64, String, String, Option<String>)>" :logic "WHERE summary IS NULL AND payload length >= min")
    (op update_timeline_summary :kind update :where ((seq "i64")) :set ((summary "&str")) :returns "bool")
    (op query_timeline_filtered :kind custom :params ((event_type "Option<&str>") (trace_id "Option<&str>") (since "Option<&str>") (until "Option<&str>") (query "Option<&str>") (limit "usize")) :returns "Vec<TimelineRow>" :logic "dynamic WHERE with 5 optional filters")
    (op query_timeline_stratified :kind custom :params ((since "Option<&str>") (until "Option<&str>") (limit "usize")) :returns "Vec<TimelineRow>" :logic "proportional sampling across event types")
    (op query_timeline_by_trace :kind custom :where ((trace_id "&str")) :returns "Vec<TimelineRow>" :logic "WHERE trace_id ORDER BY seq ASC")
    (op query_timeline_stats :kind custom :params ((since "Option<&str>") (until "Option<&str>")) :returns "TimelineStats" :logic "multi-dimension aggregation: by_type, total, time range")
    (op query_timeline_search :kind custom :params ((query "&str") (event_type "Option<&str>") (since "Option<&str>") (until "Option<&str>") (limit "usize")) :returns "Vec<TimelineRow>" :logic "FTS5 + LIKE fallback with optional type/time filters")
    (op get_briefing_summaries_for_session :kind custom :where ((session_id "&str")) :returns "HashMap<i64, String>" :logic "SELECT seq, summary WHERE trace_id matches session pattern"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: agent_questions                                   │
  ;; └──────────────────────────────────────────────────────────┘
  (table agent_questions
    (col id              :type text    :pk)
    (col task_id         :type text    :nullable)
    (col slot_id         :type text    :nullable)
    (col session_id      :type text    :nullable)
    (col question        :type text)
    (col context         :type text    :default "")
    (col status          :type text    :default "pending")
    (col answer          :type text    :nullable)
    (col created_at      :type text)
    (col updated_at      :type text)
    (col target          :type text    :default "user")
    (col options         :type text    :nullable)
    (col decision_type   :type text    :default "implementation")
    (col retry_count     :type integer :default "0")
    (col routing_trace   :type text    :nullable)

    (idx idx_agent_questions_status :cols (status))
    (idx idx_agent_questions_target :cols (target status))

    (row-type AgentQuestionRow :convert row_to_agent_question :to AgentQuestion)

    (op create_agent_question :kind custom :params ((task_id "Option<&str>") (slot_id "Option<&str>") (session_id "Option<&str>") (question "&str") (context "&str") (target "&str") (options "Option<&str>") (decision_type "&str")) :returns "AgentQuestion" :logic "INSERT with auto-id + timestamps")
    (op get_agent_question :kind select-one :where ((id text)) :returns "Option<AgentQuestion>")
    (op list_agent_questions :kind custom :params ((status "Option<&str>") (target "Option<&str>") (limit "i64")) :returns "Vec<AgentQuestion>" :logic "dynamic WHERE with optional filters")
    (op list_questions_for_task :kind select-many :where ((task_id text)) :order "created_at ASC" :returns "Vec<AgentQuestion>")
    (op answer_agent_question :kind custom :where ((id "&str")) :params ((answer "&str")) :returns "Option<AgentQuestion>" :logic "UPDATE status=answered, set answer + updated_at, unclaim board task if linked")
    (op dismiss_agent_question :kind custom :where ((id "&str")) :returns "Option<AgentQuestion>" :logic "UPDATE status=dismissed + updated_at")
    (op increment_question_retry :kind update :where ((id text)) :set ((retry_count :literal "retry_count + 1") (updated_at :via "datetime('now')")) :returns "()")
    (op set_question_routing_trace :kind update :where ((id text)) :set ((routing_trace "&str") (updated_at :via "datetime('now')")) :returns "()")
    (op downgrade_question_to_user :kind update :where ((id text)) :set ((target :literal "'user'") (updated_at :via "datetime('now')")) :returns "()")
    (op find_stale_master_questions :kind custom :params ((max_age_secs "i64")) :returns "Vec<AgentQuestion>" :logic "WHERE target=master AND status=pending AND age > threshold")
    (op find_tasks_with_unharvested_decisions :kind custom :params ((min_count "usize")) :returns "Vec<(String, String, usize)>" :logic "GROUP BY task_id: having answered_count >= min_count AND no kb harvested")
    (op decision_stats :kind custom :params ((hours "i64")) :returns "serde_json::Value" :logic "multi-dimension stats: by_status, by_target, by_decision_type, avg_response_time"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: gemini_requests                                   │
  ;; └──────────────────────────────────────────────────────────┘
  (table gemini_requests
    (col id             :type text    :pk)
    (col caller         :type text)
    (col session_id     :type text    :nullable)
    (col api_mode       :type text)
    (col model          :type text)
    (col prompt_chars   :type integer :nullable)
    (col response_chars :type integer :nullable)
    (col queue_wait_ms  :type integer :nullable)
    (col duration_ms    :type integer :nullable)
    (col retry_count    :type integer :default "0")
    (col status         :type text)
    (col error_msg      :type text    :nullable)
    (col created_at     :type timestamptz :default-now)
    (col prompt_text    :type text    :nullable)
    (col response_text  :type text    :nullable)

    (idx idx_gemini_req_created :cols (created_at))
    (idx idx_gemini_req_caller  :cols (caller))
    (idx idx_gemini_req_session :cols (session_id))

    (op gemini_log_insert_started :kind custom :params ((id "&str") (caller "&str") (session_id "Option<&str>") (api_mode "&str") (model "&str") (prompt_chars "Option<i32>") (prompt_text "Option<&str>")) :returns "()" :logic "INSERT with status=started")
    (op gemini_log_update_completed :kind custom :params ((id "&str") (response_chars "Option<i32>") (queue_wait_ms "Option<i32>") (duration_ms "Option<i32>") (retry_count "i32") (status "&str") (error_msg "Option<&str>") (response_text "Option<&str>")) :returns "()" :logic "UPDATE all response fields WHERE id")
    (op gemini_log_get_content :kind custom :where ((request_id "&str")) :returns "Option<serde_json::Value>" :logic "SELECT prompt_text, response_text as JSON object")
    (op gemini_log_query :kind custom :params ((caller "Option<&str>") (session_id "Option<&str>") (status "Option<&str>") (since "Option<&str>") (limit "i64")) :returns "Vec<serde_json::Value>" :logic "dynamic WHERE with 4 optional filters, JSON row mapping")
    (op gemini_log_stats :kind custom :returns "serde_json::Value" :logic "multi-dimension stats: by_caller, by_model, by_status, avg_duration, token_totals")
    (op gemini_log_cleanup :kind custom :params ((retention_days "i64")) :returns "i64" :logic "DELETE WHERE created_at < N days ago"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: gemini_file_uploads                               │
  ;; └──────────────────────────────────────────────────────────┘
  (table gemini_file_uploads
    (col file_hash  :type text  :pk)
    (col file_uri   :type text)
    (col mime_type  :type text)
    (col expires_at :type bigint)

    (op gemini_file_cache_get :kind custom :where ((file_hash "&str")) :returns "Option<String>" :logic "SELECT file_uri WHERE not expired")
    (op gemini_file_cache_put :kind upsert :params ((file_hash text) (file_uri text) (mime_type text) (expires_at "i64")) :on-conflict "(file_hash) DO UPDATE SET file_uri = ?2, mime_type = ?3, expires_at = ?4" :returns "()")
    (op gemini_file_cache_gc :kind custom :returns "i64" :logic "DELETE WHERE expires_at < now epoch"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: message_narrations                                │
  ;; └──────────────────────────────────────────────────────────┘
  (table message_narrations
    (col message_id   :type bigint  :pk)
    (col session_id   :type text)
    (col step_title   :type text)
    (col step_intent  :type text)
    (col step_result  :type text)
    (col created_at   :type timestamptz :default-now)

    (idx idx_narrations_session :cols (session_id))

    (op insert_narrations :kind custom :params ((narrations "&[(i64, &str, &str, &str, &str)]")) :returns "usize" :logic "transaction batch INSERT OR IGNORE")
    (op get_narrations_for_session :kind custom :where ((session_id "&str")) :returns "Vec<(i64, String, String, String)>" :logic "SELECT message_id, step_title, step_intent, step_result ORDER BY message_id")
    (op get_last_narration :kind custom :where ((session_id "&str")) :returns "Option<(i64, String, String, String)>" :logic "SELECT * ORDER BY message_id DESC LIMIT 1")
    (op fetch_narration_batch :kind custom :params ((session_id "&str") (after_id "i64") (batch_size "i64")) :returns "Vec<ConversationMessage>" :logic "SELECT messages WHERE session AND id > after_id LIMIT batch_size"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: narration_cursors                                 │
  ;; └──────────────────────────────────────────────────────────┘
  (table narration_cursors
    (col session_id        :type text    :pk)
    (col last_processed_id :type bigint  :default "0")
    (col batch_index       :type integer :default "0")
    (col status            :type text    :default "idle")
    (col retry_count       :type integer :default "0")
    (col total_messages    :type integer :default "0")
    (col updated_at        :type timestamptz :default-now)

    (op get_or_create_narration_cursor :kind custom :where ((session_id "&str")) :returns "(i64, i64, String, i64, i64)" :logic "INSERT OR IGNORE + SELECT (last_processed_id, batch_index, status, total_messages, retry_count)")
    (op get_sessions_needing_narration :kind custom :params ((min_unnarrated "i64")) :returns "Vec<(String, i64)>" :logic "sessions with msg_count - last_processed_id >= min_unnarrated")
    (op commit_narration_batch :kind custom :params ((session_id "&str") (last_id "i64") (batch_idx "i32") (total "i32")) :returns "()" :logic "UPDATE cursor: last_processed_id, batch_index, total_messages, status=idle")
    (op mark_narration_cursor_processing :kind update :where ((session_id text)) :set ((status :literal "'processing'") (updated_at :via "datetime('now')")) :returns "()")
    (op mark_narration_cursor_failed :kind custom :where ((session_id "&str")) :params ((max_retries "i64")) :returns "bool" :logic "UPDATE retry_count+1, status=failed if exceeded max"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: dynamic_slots                                     │
  ;; └──────────────────────────────────────────────────────────┘
  (table dynamic_slots
    (col id                  :type text    :pk)
    (col parent_slot_id      :type text)
    (col template            :type text)
    (col objective           :type text    :nullable)
    (col config              :type text)
    (col status              :type text    :default "active")
    (col termination_reason  :type text    :nullable)
    (col created_at          :type text)
    (col terminated_at       :type text    :nullable)
    (col ttl_seconds         :type integer :default "14400")
    (col expires_at          :type text)
    (col extend_count        :type integer :default "0")

    (idx idx_dynamic_slots_active :cols (status expires_at))

    (row-type DynamicSlotRow :convert row_to_dynamic_slot :to DynamicSlot)

    (op create_dynamic_slot :kind insert-struct :params ((slot "&DynamicSlot")) :returns "()")
    (op get_dynamic_slot :kind select-one :where ((id text)) :returns "Option<DynamicSlot>")
    (op list_dynamic_slots :kind custom :params ((status "Option<&str>")) :returns "Vec<DynamicSlot>" :logic "optional WHERE status, ORDER BY created_at DESC")
    (op count_active_dynamic_slots :kind count :where-fixed ("status = 'active'") :returns "i64")
    (op terminate_dynamic_slot :kind custom :where ((id "&str")) :params ((reason "&str")) :returns "bool" :logic "UPDATE status=terminated, set reason + terminated_at WHERE active")
    (op extend_dynamic_slot :kind custom :where ((id "&str")) :params ((additional_seconds "i64")) :returns "Option<String>" :logic "UPDATE expires_at + extend_count WHERE active, return new expires_at")
    (op find_expired_dynamic_slots :kind custom :returns "Vec<DynamicSlot>" :logic "WHERE status=active AND expires_at < now")
    (op find_expiring_dynamic_slots :kind custom :params ((within_seconds "i64")) :returns "Vec<DynamicSlot>" :logic "WHERE status=active AND expires_at < now + within"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: incidents                                         │
  ;; └──────────────────────────────────────────────────────────┘
  (table incidents
    (col id            :type text  :pk)
    (col severity      :type text)
    (col source        :type text)
    (col title         :type text)
    (col description   :type text  :default "")
    (col server_id     :type text  :nullable)
    (col raw_payload   :type text  :nullable)
    (col board_task_id :type text  :nullable)
    (col dedupe_key    :type text)
    (col created_at    :type text)

    (idx idx_incidents_created :cols (created_at))
    (idx idx_incidents_dedupe  :cols (dedupe_key created_at))

    (op insert_incident :kind insert-struct :params ((incident "&IncidentRow")) :returns "()")
    (op has_recent_incident :kind custom :params ((dedupe_key "&str") (window_secs "i64")) :returns "bool" :logic "EXISTS WHERE dedupe_key AND created_at within window")
    (op list_incidents :kind custom :params ((limit "i64")) :returns "Vec<IncidentRow>" :logic "SELECT * ORDER BY created_at DESC LIMIT"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: token_usage_ledger                                │
  ;; └──────────────────────────────────────────────────────────┘
  (table token_usage_ledger
    (col id                    :type bigint  :pk)
    (col created_at            :type timestamptz :default-now)
    (col slot_id               :type text    :nullable)
    (col slot_task_id          :type text    :nullable)
    (col conversation_id       :type text)
    (col model                 :type text    :nullable)
    (col input_tokens          :type integer :default "0")
    (col cache_creation_tokens :type integer :default "0")
    (col cache_read_tokens     :type integer :default "0")
    (col output_tokens         :type integer :default "0")
    (col message_id            :type bigint  :nullable)

    (idx idx_token_ledger_conv    :cols (conversation_id))
    (idx idx_token_ledger_slot    :cols (slot_id))
    (idx idx_token_ledger_created :cols (created_at))

    (op insert_token_usage :kind custom :params ((slot_id "Option<&str>") (slot_task_id "Option<&str>") (conversation_id "&str") (model "Option<&str>") (input_tokens "i32") (cache_creation_tokens "i32") (cache_read_tokens "i32") (output_tokens "i32") (message_id "Option<i64>")) :returns "()" :logic "INSERT with dedup on message_id")
    (op token_stats :kind custom :returns "serde_json::Value" :logic "multi-dimension aggregation: by_model, by_slot, totals, time-series"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: image_descriptions                                │
  ;; └──────────────────────────────────────────────────────────┘
  (table image_descriptions
    (col image_hash  :type text    :pk)
    (col media_type  :type text)
    (col description :type text)
    (col char_count  :type integer :default "0")
    (col created_at  :type timestamptz :default-now)

    (op get_image_description :kind custom :where ((image_hash "&str")) :returns "Option<String>" :logic "SELECT description WHERE image_hash")
    (op save_image_description :kind upsert :params ((image_hash text) (media_type text) (description text) (char_count "i32") (created_at text)) :on-conflict "(image_hash) DO UPDATE SET description = ?3, char_count = ?4" :returns "()")
    (op image_description_count :kind count :returns "i64")
    (op find_unprocessed_image_messages :kind custom :params ((limit "usize")) :returns "Vec<(i64, String)>" :logic "SELECT msg id, content WHERE has_image AND no description yet")
    (op mark_vision_permanently_failed :kind custom :params ((message_id "i64")) :returns "bool" :logic "INSERT placeholder description to prevent retry")
    (op update_message_content :kind custom :params ((message_id "i64") (new_content "&str")) :returns "()" :logic "UPDATE conversation_messages SET content WHERE id")
    (op get_message_raw_content :kind custom :params ((message_id "i64")) :returns "Option<String>" :logic "SELECT raw_content FROM conversation_messages WHERE id"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: router_chat_archive                               │
  ;; └──────────────────────────────────────────────────────────┘
  (table router_chat_archive
    (col id             :type bigint  :pk)
    (col original_id    :type bigint)
    (col session_id     :type text)
    (col role           :type text)
    (col content        :type text)
    (col timestamp      :type text)
    (col archived_at    :type timestamptz :default-now)
    (col archive_reason :type text  :default "clear")

    (idx idx_rca_session  :cols (session_id))
    (idx idx_rca_archived :cols (archived_at))

    ;; Router chat ops (cross conversations + conversation_messages + archive)
    (op router_chat_get_or_create :kind custom :params ((task_id "&str") (model "&str")) :returns "String" :logic "find existing conv by task_id OR create new, return conv id")
    (op router_chat_load_history :kind custom :where ((conv_id "&str")) :returns "Vec<serde_json::Value>" :logic "SELECT messages as JSON array")
    (op router_chat_get_summary :kind custom :where ((conv_id "&str")) :returns "(Option<String>, i64)" :logic "SELECT rolling_summary, last_summarized_msg_id")
    (op router_chat_load_active_history :kind custom :params ((conv_id "&str") (after_id "i64")) :returns "Vec<serde_json::Value>" :logic "SELECT messages WHERE id > after_id")
    (op router_chat_unsummarized_count :kind custom :params ((conv_id "&str") (after_id "i64")) :returns "i64" :logic "COUNT messages WHERE id > after_id")
    (op router_chat_load_compressible :kind custom :params ((conv_id "&str") (before_id "i64") (limit "i64")) :returns "Vec<serde_json::Value>" :logic "SELECT oldest unsummarized messages")
    (op router_chat_update_summary :kind custom :params ((conv_id "&str") (summary "&str") (up_to_id "i64")) :returns "()" :logic "UPDATE rolling_summary + last_summarized_msg_id")
    (op router_chat_append_messages :kind custom :params ((conv_id "&str") (messages "&[serde_json::Value]")) :returns "Vec<i64>" :logic "batch INSERT messages, return IDs")
    (op router_chat_list :kind custom :params ((limit "i64")) :returns "Vec<serde_json::Value>" :logic "SELECT conversations with msg_count, last_message preview")
    (op router_chat_stats :kind custom :returns "serde_json::Value" :logic "multi-dimension stats: total convs, total msgs, by_model, active sessions")
    (op router_chat_clear :kind custom :params ((conversation_id "&str") (count "Option<i64>")) :returns "(i64, i64)" :logic "archive messages → delete, return (archived, remaining)")
    (op router_chat_clear_by_task :kind custom :params ((task_id "&str") (count "Option<i64>")) :returns "i64" :logic "find conv by task → clear")
    (op router_chat_delete_message :kind custom :params ((message_id "i64")) :returns "Option<String>" :logic "archive + delete single message, return session_id")
    (op router_chat_delete :kind custom :params ((conversation_id "&str")) :returns "(i64, i64)" :logic "archive all → delete conv + messages")
    (op router_chat_delete_by_task :kind custom :params ((task_id "&str")) :returns "(i64, i64)" :logic "find conv by task → delete")
    (op router_chat_restore :kind custom :params ((conversation_id "&str")) :returns "i64" :logic "INSERT messages FROM archive back to conv_messages")
    (op jarvis_get_or_create :kind custom :params ((conversation_id "Option<&str>")) :returns "String" :logic "find/create jarvis conversation")
    (op jarvis_save_exchange :kind custom :params ((conv_id "&str") (user_msg "&str") (assistant_msg "&str")) :returns "()" :logic "INSERT user + assistant messages")
    (op jarvis_update_title :kind update :where ((conv_id text)) :set ((title "&str") (updated_at :via "datetime('now')")) :returns "()")
    (op find_latest_jarvis_conversation :kind custom :returns "Option<String>" :logic "SELECT id WHERE type=jarvis ORDER BY started_at DESC LIMIT 1"))

  ;; Board task helper ops (in question.rs but touching board_tasks)
  (table board_tasks
    (col id :type text :pk)
    (op increment_board_task_retry :kind update :where ((id text)) :set ((retry_count "i64") (updated_at :via "datetime('now')")) :returns "()")
    (op unclaim_board_task :kind custom :where ((task_id "&str")) :returns "()" :logic "UPDATE SET claim_executor=NULL, status=open WHERE id")))
