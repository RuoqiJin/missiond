;; ══════════════════════════════════════════════════════════════
;; MissionD DB Layer — Knowledge Base (SQLite)
;; 6 tables: knowledge, knowledge_edges, kb_access_log,
;;           kb_operation_queue, kb_ast_links, prompt_snapshots
;; ══════════════════════════════════════════════════════════════

(pattern crud-gateway
  :gateway MissionDB
  :pool    "Mutex<Connection>"
  :backend sqlite
  :target  "crates/missiond-core/src/db/gen_knowledge.rs"

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: knowledge                                         │
  ;; └──────────────────────────────────────────────────────────┘
  (table knowledge
    (col id               :type text      :pk)
    (col category         :type text)
    (col key              :type text)
    (col summary          :type text)
    (col detail           :type text      :nullable)
    (col source           :type text      :default "conversation")
    (col confidence       :type float8    :default "1.0")
    (col access_count     :type integer   :default "0")
    (col created_at       :type text)
    (col updated_at       :type text)
    (col last_accessed_at :type text      :nullable)
    (col linked_task_id   :type text      :nullable)
    (col embedding        :type text      :nullable)
    (col embedding_provider :type text    :nullable)
    (col kb_type          :type text      :default "fact")
    (col scope_task_id    :type text      :nullable)
    (col utility_score    :type float8    :default "0.5")
    (col needs_re_extraction :type integer :default "0")

    (unique (category key))
    (idx idx_kb_category   :cols (category))
    (idx idx_kb_scope      :cols (scope_task_id))
    (idx idx_kb_utility    :cols (utility_score))
    (idx idx_kb_type       :cols (kb_type))

    (row-type KnowledgeEntryRow
      :convert row_to_knowledge_entry :to KnowledgeEntry)

    ;; ── Mechanical CRUD ──

    (op kb_get_by_id
      :kind select-one
      :where ((id text))
      :returns "Option<KnowledgeEntry>")

    (op kb_get_id_by_key
      :kind select-one
      :where ((key text))
      :select-cols (id)
      :returns "Option<String>")

    (op kb_forget
      :kind delete
      :where ((key text))
      :returns "bool")

    (op kb_set_linked_task_id
      :kind update
      :where ((key text))
      :set ((linked_task_id "Option<&str>"))
      :returns "bool")

    (op kb_clear_scope
      :kind update
      :where ((id text))
      :set ((scope_task_id :literal "NULL")
            (updated_at :via "datetime('now')"))
      :returns "()")

    (op kb_list_by_scope
      :kind select-many
      :where ((scope_task_id text))
      :returns "Vec<KnowledgeEntry>")

    (op kb_summary
      :kind custom
      :returns "Vec<(String, i64)>"
      :logic "SELECT category, COUNT(*) GROUP BY category ORDER BY cnt DESC")

    (op kb_hot_keys
      :kind custom
      :params ((limit "i64"))
      :returns "Vec<String>"
      :logic "SELECT key ORDER BY access_count DESC, updated_at DESC LIMIT")

    (op kb_find_stale
      :kind custom
      :params ((days "i64"))
      :returns "Vec<KnowledgeEntry>"
      :logic "access_count=0 AND julianday age > N days")

    (op kb_list_low_confidence
      :kind custom
      :params ((threshold "f64") (limit "usize"))
      :returns "Vec<KnowledgeEntry>"
      :logic "WHERE confidence < threshold ORDER BY confidence ASC")

    (op kb_list_low_utility
      :kind custom
      :params ((threshold "f64") (min_access "i64") (limit "usize"))
      :returns "Vec<KnowledgeEntry>"
      :logic "WHERE utility < threshold AND access >= min")

    (op kb_list_stale_state_entries
      :kind custom
      :params ((stale_days "i64") (limit "usize"))
      :returns "Vec<KnowledgeEntry>"
      :logic "kb_type=state AND stale last_accessed_at")

    ;; ── Complex / multi-step ops ──

    (op kb_remember
      :kind custom
      :params ((input "&KBRememberInput"))
      :returns "KBRememberResult"
      :logic "4-phase upsert: exact category+key → key re-cat → fuzzy merge (Jaccard≥0.5) → insert new")

    (op kb_update
      :kind custom
      :where ((key "&str"))
      :params ((new_category "Option<&str>") (new_summary "Option<&str>") (new_detail "Option<&serde_json::Value>")
               (new_confidence "Option<f64>") (new_linked_task_id "Option<&str>"))
      :returns "Option<(KnowledgeEntry, bool)>"
      :logic "dynamic SET clauses, returns (entry, content_changed)")

    (op kb_get
      :kind custom
      :where ((key "&str"))
      :returns "Option<KnowledgeEntry>"
      :logic "SELECT + side-effect: bump access_count + utility_score")

    (op kb_list
      :kind custom
      :params ((category "Option<&str>"))
      :returns "Vec<KnowledgeEntry>"
      :logic "optional category filter with LIKE for sub-categories")

    (op kb_list_paginated
      :kind custom
      :params ((category "Option<&str>") (limit "u32") (offset "u32"))
      :returns "Vec<KnowledgeEntry>"
      :logic "kb_list + LIMIT OFFSET")

    (op kb_search
      :kind custom
      :params ((query "&str") (category "Option<&str>"))
      :returns "Vec<KnowledgeEntry>"
      :logic "hybrid FTS5 + LIKE fallback search")

    (op kb_search_ranked
      :kind custom
      :params ((query "&str") (category "Option<&str>") (limit "usize"))
      :returns "Vec<(KnowledgeEntry, usize)>"
      :logic "kb_search with rank position")

    (op kb_search_fts_ranked
      :kind custom
      :params ((query "&str") (category "Option<&str>"))
      :returns "Vec<(String, usize, Option<String>)>"
      :logic "FTS5 MATCH with snippet() and BM25")

    (op kb_search_like_ranked
      :kind custom
      :params ((query "&str") (category "Option<&str>"))
      :returns "Vec<(String, usize)>"
      :logic "dynamic per-keyword LIKE with multi-column search")

    (op kb_set_embedding
      :kind custom
      :where ((id "&str"))
      :params ((embedding "&[f32]") (provider "&str"))
      :returns "()"
      :logic "UPDATE embedding blob + provider")

    (op kb_entries_missing_embedding
      :kind custom
      :params ((category "Option<&str>"))
      :returns "Vec<(String, String, String)>"
      :logic "id, summary, detail WHERE embedding IS NULL")

    (op kb_entries_stale_embedding
      :kind custom
      :params ((current_provider "&str") (limit "i64"))
      :returns "Vec<(String, String, String)>"
      :logic "id, summary, detail WHERE provider != current")

    (op kb_load_embeddings
      :kind custom
      :params ((category "&str"))
      :returns "Vec<(String, Vec<f32>)>"
      :logic "deserialize embedding blobs by category")

    (op kb_load_all_embeddings
      :kind custom
      :returns "Vec<(String, Vec<f32>)>"
      :logic "deserialize all embedding blobs")

    (op kb_batch_forget
      :kind custom
      :params ((keys "&[String]"))
      :returns "usize"
      :logic "loop DELETE + FTS rebuild")

    (op kb_update_access_stats
      :kind custom
      :params ((entries "&[KnowledgeEntry]"))
      :returns "()"
      :logic "batch UPDATE access_count + utility_score with anti-spam (1hr cooldown)")

    (op kb_adjust_confidence
      :kind custom
      :where ((id "&str"))
      :params ((delta "f64"))
      :returns "Option<f64>"
      :logic "UPDATE confidence = MAX(0.1, MIN(1.0, confidence + delta))")

    (op kb_batch_apply_utility_feedback
      :kind custom
      :params ((kb_ids "&[String]") (success "bool"))
      :returns "usize"
      :logic "transacted batch utility boost/decay")

    (op kb_mark_needs_re_extraction
      :kind custom
      :params ((ids "&[String]"))
      :returns "usize"
      :logic "loop UPDATE needs_re_extraction = 1")

    (op kb_find_duplicates
      :kind custom
      :returns "Vec<(KnowledgeEntry, KnowledgeEntry, f64)>"
      :logic "in-memory Jaccard similarity across categories (threshold 0.6)")

    (op kb_auto_gc
      :kind custom
      :returns "usize"
      :logic "3-phase: delete infra → Darwinian decay gc → cleanup edges + FTS rebuild")

    (op kb_stats
      :kind custom
      :returns "serde_json::Value"
      :logic "complex aggregation: total, by_category, by_source, by_type, utility distribution")

    (op embedding_stats
      :kind custom
      :returns "serde_json::Value"
      :logic "coverage stats for KB/Skill/Conversation embeddings across providers"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: knowledge_edges                                   │
  ;; └──────────────────────────────────────────────────────────┘
  (table knowledge_edges
    (col source_id      :type text)
    (col target_id      :type text)
    (col relation_type  :type text)
    (col weight         :type float8    :default "1.0")
    (col created_at     :type timestamptz :default-now)
    (pk (source_id target_id relation_type))

    (idx idx_kb_edge_source :cols (source_id))
    (idx idx_kb_edge_target :cols (target_id))

    (op kb_add_edge
      :kind upsert
      :params ((source_id text) (target_id text) (relation_type text) (weight "f64") (created_at text))
      :on-conflict "(source_id, target_id, relation_type) DO UPDATE SET weight = ?4"
      :returns "()")

    (op kb_get_edges
      :kind custom
      :where ((id "&str"))
      :returns "Vec<KBEdge>"
      :logic "WHERE source_id = ?1 OR target_id = ?1")

    (op kb_delete_edges_for
      :kind custom
      :where ((id "&str"))
      :returns "()"
      :logic "DELETE WHERE source_id OR target_id")

    (op kb_expand_related
      :kind custom
      :params ((ids "&[String]") (max_extra "usize"))
      :returns "Vec<String>"
      :logic "per-id neighbor query, deduplicate, sort by weight, cap at max_extra"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: kb_access_log                                     │
  ;; └──────────────────────────────────────────────────────────┘
  (table kb_access_log
    (col id              :type bigint    :pk)
    (col kb_id           :type text)
    (col co_accessed_ids :type text)
    (col context_source  :type text      :default "prefetch")
    (col created_at      :type timestamptz :default-now)
    (col session_id      :type text      :nullable)

    (idx idx_kb_access_log_kb      :cols (kb_id))
    (idx idx_kb_access_log_created :cols (created_at))
    (idx idx_kb_access_log_session :cols (session_id))

    (op kb_log_co_access
      :kind custom
      :params ((kb_ids "&[String]") (source "&str") (session_id "Option<&str>"))
      :returns "()"
      :logic "for each kb_id: INSERT INTO kb_access_log")

    (op get_session_cited_kb_ids
      :kind custom
      :where ((session_id "&str"))
      :returns "Vec<String>"
      :logic "SELECT DISTINCT kb_id WHERE session_id")

    (op kb_compute_cooccurrence
      :kind custom
      :params ((since_hours "i64") (top_n "usize"))
      :returns "HashMap<String, Vec<String>>"
      :logic "clean old logs + fetch recent + build frequency map"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: kb_operation_queue                                │
  ;; └──────────────────────────────────────────────────────────┘
  (table kb_operation_queue
    (col id          :type text    :pk)
    (col plan_id     :type text)
    (col task_id     :type text    :nullable)
    (col operation   :type text)
    (col target_keys :type text)
    (col rationale   :type text    :nullable)
    (col status      :type text    :default "pending")
    (col priority    :type integer :default "0")
    (col result      :type text    :nullable)
    (col created_at  :type text)
    (col executed_at :type text    :nullable)
    (col error       :type text    :nullable)

    (idx idx_kb_op_status :cols (status))
    (idx idx_kb_op_plan   :cols (plan_id))

    (op kb_ops_save_plan
      :kind custom
      :params ((plan_id "&str") (task_id "Option<&str>") (operations "&[KBOperation]"))
      :returns "usize"
      :logic "loop INSERT OR REPLACE for each operation")

    (op kb_ops_list
      :kind custom
      :params ((plan_id "Option<&str>") (status "Option<&str>"))
      :returns "Vec<KBOperationRow>"
      :logic "dynamic WHERE with optional filters")

    (op kb_ops_update_status
      :kind custom
      :params ((op_id "&str") (status "&str") (result "Option<&str>") (error "Option<&str>"))
      :returns "bool"
      :logic "UPDATE SET status, result, error, executed_at")

    (op kb_ops_complete_by_task_id
      :kind custom
      :params ((task_id "&str") (new_status "&str") (result "Option<&str>"))
      :returns "bool"
      :logic "UPDATE WHERE status=dispatched AND result LIKE task pattern")

    (op kb_ops_expire_stale
      :kind custom
      :params ((ttl_secs "i64"))
      :returns "usize"
      :logic "UPDATE pending → expired WHERE created_at < TTL")

    (op kb_ops_plan_summary
      :kind custom
      :where ((plan_id "&str"))
      :returns "serde_json::Value"
      :logic "SELECT status, COUNT(*) GROUP BY status"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: kb_ast_links                                      │
  ;; └──────────────────────────────────────────────────────────┘
  (table kb_ast_links
    (col id           :type bigint    :pk)
    (col kb_id        :type text)
    (col ast_node_id  :type text      :nullable)
    (col symbol_name  :type text)
    (col file_path    :type text      :nullable)
    (col relation     :type text      :default "related_to")
    (col confidence   :type float8    :default "0.8")
    (col created_at   :type timestamptz :default-now)

    (idx idx_kb_ast_kb     :cols (kb_id))
    (idx idx_kb_ast_symbol :cols (symbol_name))
    (idx idx_kb_ast_file   :cols (file_path))
    (idx idx_kb_ast_node   :cols (ast_node_id))

    (op kb_add_ast_link
      :kind custom
      :params ((kb_id "&str") (symbol_name "&str") (file_path "Option<&str>")
               (ast_node_id "Option<&str>") (relation "&str") (confidence "f64"))
      :returns "()"
      :logic "INSERT ON CONFLICT DO NOTHING")

    (op kb_get_memories_for_symbol
      :kind custom
      :where ((symbol_name "&str"))
      :returns "Vec<KnowledgeEntry>"
      :logic "JOIN knowledge ON kb_id ORDER BY utility DESC LIMIT 10")

    (op kb_get_memories_for_file
      :kind custom
      :where ((file_path "&str"))
      :returns "Vec<(String, KnowledgeEntry)>"
      :logic "JOIN knowledge ON kb_id ORDER BY symbol_name, utility LIMIT 30")

    (op kb_delete_ast_links_for
      :kind custom
      :where ((kb_id "&str"))
      :returns "()"
      :logic "DELETE WHERE kb_id")

    (op kb_ast_lazy_resolve
      :kind custom
      :params ((link_id "i64") (symbol_name "&str"))
      :returns "Option<String>"
      :logic "lookup ast_nodes by name → UPDATE kb_ast_links.ast_node_id"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: prompt_snapshots                                  │
  ;; └──────────────────────────────────────────────────────────┘
  (table prompt_snapshots
    (col task_id     :type text    :pk)
    (col prompt      :type text)
    (col cited_kb_ids :type text   :default "[]")
    (col category    :type text    :default "other")
    (col task_outcome :type text   :nullable)
    (col created_at  :type text)

    (idx idx_prompt_snap_cat :cols (category))

    (op save_prompt_snapshot
      :kind upsert
      :params ((task_id text) (prompt text) (cited_kb_ids text) (category text) (created_at text))
      :on-conflict "(task_id) DO UPDATE SET prompt = ?2, cited_kb_ids = ?3, category = ?4, created_at = ?5"
      :returns "()")

    (op update_prompt_snapshot_outcome
      :kind update
      :where ((task_id text))
      :set ((task_outcome "&str"))
      :returns "()")

    (op list_prompt_snapshots
      :kind custom
      :params ((category "&str") (limit "usize"))
      :returns "Vec<(String, String, String)>"
      :logic "WHERE category AND task_outcome IS NOT NULL ORDER BY created_at DESC")

    (op list_modified_snapshots
      :kind custom
      :params ((limit "usize"))
      :returns "Vec<(String, String, String, String)>"
      :logic "multi-table JOIN: snapshots WHERE success AND cited KB modified after snapshot")))
