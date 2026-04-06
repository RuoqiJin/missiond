;; ══════════════════════════════════════════════════════════════
;; MissionD DB Layer — Data Pipeline Infrastructure (SQLite)
;; backfill, beacons, watermarks, labels, translations
;; ══════════════════════════════════════════════════════════════

(pattern crud-gateway
  :gateway MissionDB
  :pool    "Mutex<Connection>"
  :backend sqlite
  :target  "crates/missiond-core/src/db/gen_pipeline.rs"

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: backfill_progress                                 │
  ;; └──────────────────────────────────────────────────────────┘
  (table backfill_progress
    (col phase           :type text    :pk)
    (col status          :type text    :default "pending")
    (col last_cursor     :type bigint  :default "0")
    (col total_estimated :type bigint  :default "0")
    (col processed       :type bigint  :default "0")
    (col failed          :type bigint  :default "0")
    (col started_at      :type text    :nullable)
    (col completed_at    :type text    :nullable)
    (col updated_at      :type text    :nullable)

    (op backfill_get_phase
      :kind custom
      :where ((phase "&str"))
      :returns "Option<BackfillPhaseStatus>"
      :logic "SELECT * WHERE phase, map to struct")

    (op backfill_list_phases
      :kind custom
      :returns "Vec<BackfillPhaseStatus>"
      :logic "SELECT * ORDER BY started_at")

    (op backfill_start_phase
      :kind upsert
      :params ((phase text) (total_estimated "i64") (started_at text) (status text) (updated_at text))
      :on-conflict "(phase) DO UPDATE SET status = ?4, total_estimated = ?2, started_at = COALESCE(?3, started_at), updated_at = ?5"
      :returns "()")

    (op backfill_update_progress
      :kind update
      :where ((phase text))
      :set ((last_cursor "i64")
            (processed "i64")
            (failed "i64")
            (updated_at :via "datetime('now')"))
      :returns "()")

    (op backfill_complete_phase
      :kind update
      :where ((phase text))
      :set ((status :literal "'completed'")
            (completed_at :via "datetime('now')")
            (updated_at :via "datetime('now')"))
      :returns "()"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: backfill_failures                                 │
  ;; └──────────────────────────────────────────────────────────┘
  (table backfill_failures
    (col session_id   :type text)
    (col phase        :type text)
    (col retry_count  :type integer  :default "1")
    (col last_error   :type text     :nullable)
    (col updated_at   :type text     :nullable)
    (pk (session_id phase))

    (op backfill_record_failure
      :kind upsert
      :params ((session_id text) (phase text) (last_error text) (updated_at text) (retry_count "i32"))
      :on-conflict "(session_id, phase) DO UPDATE SET retry_count = retry_count + 1, last_error = ?3, updated_at = ?4"
      :returns "()")

    (op backfill_clear_failure
      :kind delete
      :where ((session_id text) (phase text))
      :returns "()")

    (op backfill_retryable_failures
      :kind custom
      :params ((phase "&str") (max_retries "i64") (limit "i64"))
      :returns "Vec<String>"
      :logic "WHERE retry_count < max AND cooldown elapsed, return session_ids")

    (op backfill_retryable_failures_no_cooldown
      :kind custom
      :params ((phase "&str") (max_retries "i64"))
      :returns "i64"
      :logic "COUNT WHERE retry_count < max (no cooldown check)")

    ;; Cursor-based backfill queries (cross-table)
    (op conversations_missing_summary_cursor
      :kind custom
      :params ((cursor "i64") (limit "i64"))
      :returns "Vec<(i64, String)>"
      :logic "SELECT rowid, id FROM conversations WHERE no summary, rowid > cursor")

    (op conversations_needing_topic_vectors_cursor
      :kind custom
      :params ((provider "&str") (cursor "i64") (limit "i64"))
      :returns "Vec<(i64, String)>"
      :logic "SELECT rowid, id WHERE has embedding but no topic vectors, rowid > cursor")

    (op conversations_missing_summary_count
      :kind custom
      :returns "i64"
      :logic "COUNT conversations without summary (user/worker types)")

    (op conversations_needing_topic_vectors_count
      :kind custom
      :params ((provider "&str"))
      :returns "i64"
      :logic "COUNT conversations with embedding but no topic vectors"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: beacons                                           │
  ;; └──────────────────────────────────────────────────────────┘
  (table beacons
    (col id            :type text  :pk)
    (col name          :type text)
    (col description   :type text  :nullable)
    (col created_at    :type timestamptz :default-now)
    (col updated_at    :type timestamptz :default-now)
    (col harvest_count :type integer :default "0")

    (unique (name))

    (op beacon_list
      :kind custom
      :returns "Vec<BeaconInfo>"
      :logic "SELECT with LEFT JOIN beacon_nodes for node_count")

    (op beacon_ensure
      :kind custom
      :params ((name "&str"))
      :returns "String"
      :logic "INSERT OR IGNORE + return id (existing or new)")

    (op beacon_increment_harvest
      :kind custom
      :params ((name "&str"))
      :returns "i64"
      :logic "UPDATE harvest_count + 1, return new count")

    (op beacon_set_description
      :kind update
      :where ((name text))
      :set ((description "&str")
            (updated_at :via "datetime('now')"))
      :returns "bool")

    (op beacon_search
      :kind custom
      :params ((query "&str"))
      :returns "Vec<BeaconInfo>"
      :logic "LIKE search on name + description + symbol_name")

    (op beacon_cleanup_empty
      :kind custom
      :returns "usize"
      :logic "DELETE beacons with no beacon_nodes"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: beacon_nodes                                      │
  ;; └──────────────────────────────────────────────────────────┘
  (table beacon_nodes
    (col beacon_id    :type text  :fk beacons :on-delete cascade)
    (col repo         :type text)
    (col file_path    :type text)
    (col symbol_name  :type text)
    (col annotation   :type text  :nullable)
    (pk (beacon_id repo file_path symbol_name))

    (idx idx_bn_symbol :cols (symbol_name))
    (idx idx_bn_beacon :cols (beacon_id))

    (op beacon_node_upsert
      :kind upsert
      :params ((beacon_id text) (repo text) (file_path text) (symbol_name text) (annotation text))
      :on-conflict "(beacon_id, repo, file_path, symbol_name) DO UPDATE SET annotation = COALESCE(?5, annotation)"
      :returns "()")

    (op beacon_nodes_delete_file
      :kind custom
      :params ((repo "&str") (file_path "&str"))
      :returns "usize"
      :logic "DELETE WHERE repo AND file_path")

    (op beacon_map
      :kind custom
      :params ((beacon_name "&str"))
      :returns "Vec<BeaconNode>"
      :logic "JOIN beacons ON name → SELECT nodes + optional ast_nodes annotation")

    (op beacon_node_annotate
      :kind custom
      :params ((beacon_name "&str") (symbol_name "&str") (annotation "&str"))
      :returns "bool"
      :logic "UPDATE annotation via beacon name + symbol JOIN"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: consumer_watermarks                               │
  ;; └──────────────────────────────────────────────────────────┘
  (table consumer_watermarks
    (col consumer_name         :type text)
    (col session_id            :type text)
    (col last_processed_msg_id :type bigint  :nullable)
    (col last_processed_time   :type text    :nullable)
    (col extra                 :type text    :nullable)
    (pk (consumer_name session_id))

    (idx idx_cw_consumer :cols (consumer_name))

    (op watermark_get
      :kind custom
      :params ((consumer "&str") (session_id "&str"))
      :returns "Option<(Option<i64>, Option<String>)>"
      :logic "SELECT msg_id, time WHERE consumer AND session")

    (op watermark_advance_time
      :kind upsert
      :params ((consumer_name text) (session_id text) (last_processed_time text))
      :on-conflict "(consumer_name, session_id) DO UPDATE SET last_processed_time = ?3"
      :returns "()")

    (op watermark_advance_msg_id
      :kind upsert
      :params ((consumer_name text) (session_id text) (last_processed_msg_id "i64"))
      :on-conflict "(consumer_name, session_id) DO UPDATE SET last_processed_msg_id = ?3"
      :returns "()")

    (op watermark_advance_full
      :kind custom
      :params ((consumer "&str") (session_id "&str") (msg_id "Option<i64>") (timestamp "Option<&str>") (extra "Option<&str>"))
      :returns "()"
      :logic "UPSERT with optional field updates via COALESCE")

    (op watermark_list
      :kind custom
      :params ((consumer "&str"))
      :returns "Vec<(String, Option<i64>, Option<String>)>"
      :logic "SELECT session_id, msg_id, time WHERE consumer"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: message_labels                                    │
  ;; └──────────────────────────────────────────────────────────┘
  (table message_labels
    (col message_id  :type bigint)
    (col label       :type text)
    (col value       :type text   :nullable)
    (col source      :type text   :default "rule")
    (col created_at  :type timestamptz :default-now)
    (pk (message_id label))

    (idx idx_msg_label     :cols (label value))
    (idx idx_msg_label_mid :cols (message_id))

    (op label_set
      :kind upsert
      :params ((message_id "i64") (label text) (value text) (source text) (created_at text))
      :on-conflict "(message_id, label) DO UPDATE SET value = ?3, source = ?4"
      :returns "()")

    (op label_set_batch
      :kind custom
      :params ((labels "&[(i64, &str, &str, &str)]"))
      :returns "usize"
      :logic "transaction loop: INSERT OR REPLACE for each label")

    (op label_get
      :kind custom
      :params ((message_id "i64"))
      :returns "Vec<(String, String, String)>"
      :logic "SELECT label, value, source WHERE message_id")

    (op label_get_batch
      :kind custom
      :params ((message_ids "&[i64]"))
      :returns "HashMap<i64, Vec<(String, String)>>"
      :logic "batch SELECT with IN clause, group by message_id")

    (op label_find_messages
      :kind custom
      :params ((label "&str") (value "Option<&str>") (limit "i64"))
      :returns "Vec<i64>"
      :logic "SELECT message_id WHERE label AND optional value"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: message_translations                              │
  ;; └──────────────────────────────────────────────────────────┘
  (table message_translations
    (col message_id   :type bigint  :pk)
    (col translation  :type text)
    (col source_lang  :type text    :default "en")
    (col target_lang  :type text    :default "zh")
    (col model        :type text    :nullable)
    (col duration_ms  :type integer :nullable)
    (col created_at   :type timestamptz :default-now)

    (op insert_translation
      :kind upsert
      :params ((message_id "i64") (translation text) (source_lang text)
               (target_lang text) (model text) (duration_ms "i32") (created_at text))
      :on-conflict "(message_id) DO UPDATE SET translation = ?2, model = ?5, duration_ms = ?6"
      :returns "()")

    (op get_translation
      :kind custom
      :params ((message_id "i64"))
      :returns "Option<(String, String)>"
      :logic "SELECT translation, target_lang WHERE message_id")

    (op has_translation
      :kind count
      :where ((message_id "i64"))
      :returns "bool")))
