;; ══════════════════════════════════════════════════════════════
;; MissionD DB Layer — Skills & AST (SQLite)
;; skill_topics, skill_blocks, skill_versions, skill_executions,
;; ast_nodes, ast_file_meta
;; ══════════════════════════════════════════════════════════════

(pattern crud-gateway
  :gateway MissionDB
  :pool    "Mutex<Connection>"
  :backend sqlite
  :target  "crates/missiond-core/src/db/gen_skill.rs"

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: skill_topics                                      │
  ;; └──────────────────────────────────────────────────────────┘
  (table skill_topics
    (col topic              :type text  :pk)
    (col description        :type text  :nullable)
    (col aka                :type text  :nullable)
    (col allowed_tools      :type text  :nullable)
    (col file_path          :type text)
    (col hit_count          :type integer :default "0")
    (col last_hit_at        :type text  :nullable)
    (col fragment_count     :type integer :default "0")
    (col total_lines        :type integer :default "0")
    (col checksum           :type text  :nullable)
    (col created_at         :type text)
    (col updated_at         :type text)
    (col requires_json      :type text  :nullable)
    (col actions_json       :type text  :nullable)
    (col context_hooks_json :type text  :nullable)
    (col embedding          :type text  :nullable)
    (col embedding_provider :type text  :nullable)

    (op skill_topic_upsert
      :kind custom
      :params ((topic "&str") (description "Option<&str>") (aka "Option<&str>")
               (allowed_tools "Option<&str>") (file_path "&str"))
      :returns "()"
      :logic "INSERT OR REPLACE with timestamp management")

    (op skill_topic_upsert_full
      :kind custom
      :params ((topic "&str") (description "Option<&str>") (aka "Option<&str>")
               (allowed_tools "Option<&str>") (file_path "&str")
               (requires_json "Option<&str>") (actions_json "Option<&str>")
               (context_hooks_json "Option<&str>"))
      :returns "()"
      :logic "full INSERT OR REPLACE with all extended fields")

    (op skill_topic_get
      :kind custom
      :where ((topic "&str"))
      :returns "Option<SkillTopic>"
      :logic "SELECT * with struct mapping")

    (op skill_topic_list
      :kind custom
      :returns "Vec<SkillTopic>"
      :logic "SELECT * ORDER BY topic with struct mapping")

    (op skill_topic_hit
      :kind update
      :where ((topic text))
      :set ((hit_count :literal "hit_count + 1")
            (last_hit_at :via "datetime('now')"))
      :returns "()")

    (op skill_topic_update_stats
      :kind update
      :where ((topic text))
      :set ((total_lines "i32") (checksum "&str") (updated_at :via "datetime('now')"))
      :returns "()")

    (op skill_set_topic_embedding
      :kind custom
      :params ((topic "&str") (embedding "&[f32]") (provider "&str"))
      :returns "()"
      :logic "UPDATE embedding blob + provider WHERE topic")

    (op skill_load_topic_embeddings
      :kind custom
      :returns "Vec<(String, Vec<f32>)>"
      :logic "deserialize all topic embedding blobs")

    (op skill_topics_missing_embedding
      :kind custom
      :params ((limit "i64"))
      :returns "Vec<(String, String)>"
      :logic "topic, description WHERE embedding IS NULL")

    (op skill_topics_stale_embedding
      :kind custom
      :params ((current_provider "&str") (limit "i64"))
      :returns "Vec<(String, String)>"
      :logic "topic, description WHERE provider != current")

    (op skill_search_topics
      :kind custom
      :params ((query "&str"))
      :returns "Vec<SkillTopic>"
      :logic "LIKE search on topic + description + aka"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: skill_blocks                                      │
  ;; └──────────────────────────────────────────────────────────┘
  (table skill_blocks
    (col id         :type text  :pk)
    (col topic      :type text)
    (col block_type :type text)
    (col title      :type text  :nullable)
    (col content    :type text)
    (col sort_order :type integer :default "0")
    (col status     :type text  :default "active")
    (col created_at :type text)
    (col updated_at :type text)

    (idx idx_skill_blocks_topic  :cols (topic))
    (idx idx_skill_blocks_status :cols (status))

    (op skill_block_insert
      :kind custom
      :params ((id "&str") (topic "&str") (block_type "&str") (title "Option<&str>")
               (content "&str") (sort_order "i32"))
      :returns "()"
      :logic "INSERT with FTS sync")

    (op skill_block_update
      :kind custom
      :where ((id "&str"))
      :params ((content "&str"))
      :returns "bool"
      :logic "UPDATE content + FTS sync")

    (op skill_block_set_status
      :kind update
      :where ((id text))
      :set ((status "&str") (updated_at :via "datetime('now')"))
      :returns "bool")

    (op skill_blocks_for_topic
      :kind custom
      :where ((topic "&str"))
      :returns "Vec<SkillBlock>"
      :logic "SELECT * WHERE topic AND status=active ORDER BY sort_order")

    (op skill_blocks_delete_topic
      :kind custom
      :params ((topic "&str"))
      :returns "usize"
      :logic "DELETE blocks + FTS cleanup WHERE topic")

    (op skill_search_fts
      :kind custom
      :params ((query "&str"))
      :returns "Vec<SkillSearchResult>"
      :logic "FTS5 search with snippet + topic JOIN")

    (op skill_rebuild_fts
      :kind custom
      :returns "()"
      :logic "DROP + recreate FTS table from skill_blocks"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: skill_versions                                    │
  ;; └──────────────────────────────────────────────────────────┘
  (table skill_versions
    (col id        :type bigint  :pk)
    (col topic     :type text)
    (col content   :type text)
    (col checksum  :type text)
    (col created_at :type text)

    (idx idx_skill_ver_topic :cols (topic))

    (op skill_version_save
      :kind custom
      :params ((topic "&str") (content "&str") (checksum "&str"))
      :returns "()"
      :logic "INSERT with auto-increment ID + timestamp")

    (op skill_version_list
      :kind custom
      :params ((topic "&str") (limit "i64"))
      :returns "Vec<SkillVersion>"
      :logic "SELECT * WHERE topic ORDER BY id DESC LIMIT")

    (op skill_version_get
      :kind custom
      :params ((id "i64"))
      :returns "Option<SkillVersion>"
      :logic "SELECT * WHERE id"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: skill_executions                                  │
  ;; └──────────────────────────────────────────────────────────┘
  (table skill_executions
    (col id              :type text  :pk)
    (col skill_topic     :type text)
    (col action_id       :type text)
    (col status          :type text  :default "running")
    (col steps_total     :type integer)
    (col steps_completed :type integer :default "0")
    (col context_json    :type text  :nullable)
    (col error           :type text  :nullable)
    (col triggered_by    :type text  :default "manual")
    (col created_at      :type text)
    (col completed_at    :type text  :nullable)
    (col duration_ms     :type integer :nullable)

    (op skill_execution_insert
      :kind custom
      :params ((id "&str") (skill_topic "&str") (action_id "&str") (steps_total "i32")
               (triggered_by "&str") (context_json "Option<&str>"))
      :returns "()"
      :logic "INSERT with auto-timestamp")

    (op skill_execution_update
      :kind custom
      :params ((id "&str") (status "&str") (steps_completed "i32") (error "Option<&str>"))
      :returns "()"
      :logic "UPDATE status + steps + optional error + completion timestamp")

    (op skill_execution_update_with_duration
      :kind custom
      :params ((id "&str") (status "&str") (steps_completed "i32") (error "Option<&str>") (duration_ms "i32"))
      :returns "()"
      :logic "same as above + explicit duration_ms")

    (op skill_execution_stats
      :kind custom
      :params ((topic "Option<&str>"))
      :returns "Vec<SkillExecutionStat>"
      :logic "GROUP BY skill_topic: total, completed, failed, avg_duration")

    (op skill_execution_is_running
      :kind custom
      :params ((skill_topic "&str") (action_id "&str"))
      :returns "bool"
      :logic "EXISTS WHERE status=running AND topic AND action_id")))
