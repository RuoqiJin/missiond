;; ══════════════════════════════════════════════════════════════
;; MissionD DB Layer — CRUD Gateway (SQLite backend)
;; Forge: Lisp → IR → rusqlite codegen
;; ══════════════════════════════════════════════════════════════

(pattern crud-gateway
  :gateway MissionDB
  :pool    "Mutex<Connection>"
  :backend sqlite
  :target  "crates/missiond-core/src/db/gen_board.rs"

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: board_tasks                                       │
  ;; └──────────────────────────────────────────────────────────┘
  (table board_tasks
    (col id               :type text        :pk)
    (col title            :type text)
    (col description      :type text        :default "")
    (col status           :type text        :default "open")
    (col priority         :type text        :default "medium")
    (col category         :type text        :default "other")
    (col project          :type text        :nullable)
    (col server           :type text        :nullable)
    (col due_date         :type text        :nullable)
    (col parent_id        :type text        :nullable)
    (col assignee         :type text        :nullable)
    (col auto_execute     :type boolean     :default "0")
    (col prompt_template  :type text        :nullable)
    (col hidden           :type boolean     :default "0")
    (col retry_count      :type integer     :default "0")
    (col max_retries      :type integer     :default "2")
    (col order_idx        :type bigint      :default "0")
    (col created_at       :type timestamptz :default-now)
    (col updated_at       :type timestamptz :default-now)
    (col claim_executor_id   :type text     :nullable)
    (col claim_executor_type :type text     :nullable)
    (col claimed_at       :type text        :nullable)
    (col flow_phase       :type text        :nullable)
    (col flow_context     :type text        :nullable)
    (col flow_template    :type text        :nullable)
    (col depends_on       :type jsonb       :default "[]")
    (col lease_expires_at :type text        :nullable)
    (col dedupe_key       :type text        :nullable)
    (col timeout_secs     :type integer     :nullable)
    (col context_intent   :type text        :nullable)
    (col notes_count      :type integer     :default "0")

    (idx idx_board_tasks_status     :cols (status))
    (idx idx_board_tasks_parent     :cols (parent_id))
    (idx idx_board_tasks_category   :cols (category))
    (idx idx_board_tasks_dedupe     :cols (dedupe_key))
    (idx idx_board_tasks_order      :cols (order_idx))

    (row-type BoardTaskRow
      :convert row_to_board_task :to BoardTask
      :enum-map ((status board_task_status_from_str BoardTaskStatus)))

    ;; ── Mechanical CRUD ops ──

    (op insert_board_task
      :kind insert-struct
      :params ((task "&BoardTask"))
      :returns "()")

    (op get_board_task_by_id
      :kind select-one
      :where ((id text))
      :returns "Option<BoardTask>")

    (op list_board_tasks_by_status
      :kind select-many
      :where ((status text))
      :order "order_idx ASC"
      :returns "Vec<BoardTask>")

    (op find_open_task_by_dedupe_key
      :kind select-one
      :where ((dedupe_key text))
      :where-fixed ("status NOT IN ('done', 'failed', 'skipped')")
      :returns "Option<BoardTask>")

    (op set_board_task_lease
      :kind update
      :where ((id text))
      :set ((lease_expires_at "&str")
            (updated_at :via "datetime('now')"))
      :returns "()")

    (op release_board_claims_by_executor
      :kind update
      :where ((claim_executor_id text))
      :set ((claim_executor_id :literal "NULL")
            (claim_executor_type :literal "NULL")
            (claimed_at :literal "NULL")
            (status :literal "'open'")
            (updated_at :via "datetime('now')"))
      :returns "()")

    (op list_autopilot_tasks
      :kind select-many
      :where-fixed ("auto_execute = 1" "status = 'open'")
      :order "CASE WHEN assignee IS NOT NULL THEN 0 ELSE 1 END, order_idx ASC"
      :returns "Vec<BoardTask>")

    (op list_running_autopilot_tasks
      :kind select-many
      :where-fixed ("auto_execute = 1" "status = 'running'" "claim_executor_id IS NOT NULL")
      :order "claimed_at ASC"
      :returns "Vec<BoardTask>")

    (op clear_done_board_tasks
      :kind delete
      :where-fixed ("status = 'done'")
      :returns "i64")

    (op count_tasks_by_category
      :kind count
      :where ((category text))
      :where-fixed ("status IN ('open','running')" "hidden = 0")
      :returns "i64")

    ;; ── Complex ops → Generation Gap (Custom trait) ──

    (op create_board_task
      :kind custom
      :params ((input "&CreateBoardTaskInput"))
      :returns "BoardTask"
      :logic "resolve parent_id, compute max order_idx, construct struct, insert")

    (op update_board_task
      :kind custom
      :where ((id "&str"))
      :params ((update "&UpdateBoardTaskInput"))
      :returns "Option<BoardTask>"
      :logic "dynamic SET with push_field macro, resolve parent_id/depends_on, auto-clear claims")

    (op resolve_board_task_id
      :kind custom
      :where ((id "&str"))
      :returns "Option<String>"
      :logic "exact match, then prefix match for short IDs >= 6 chars")

    (op get_board_task
      :kind custom
      :where ((id "&str"))
      :returns "Option<BoardTask>"
      :logic "resolve_board_task_id → get_board_task_by_id")

    (op claim_board_task
      :kind custom
      :where ((id "&str"))
      :params ((executor_id "&str") (executor_type "&str"))
      :returns "Option<BoardTask>"
      :logic "CAS: UPDATE WHERE status='open' AND claim_executor_id IS NULL")

    (op delete_board_task
      :kind custom
      :where ((id "&str"))
      :returns "i64"
      :logic "recursive descendant collection + cascade delete notes")

    (op toggle_board_task
      :kind custom
      :where ((id "&str"))
      :returns "Option<BoardTask>"
      :logic "read-modify-write: open↔done toggle")

    (op close_task_by_dedupe_key
      :kind custom
      :params ((dedupe_key "&str"))
      :returns "Option<BoardTask>"
      :logic "find open by dedupe_key → update to done")

    (op recover_stale_running_tasks
      :kind custom
      :params ((fallback_stale_minutes "i64"))
      :returns "usize"
      :logic "dual-phase: lease-expired + fallback time-based recovery")

    (op check_dependencies
      :kind custom
      :params ((depends_on "&[TaskId]"))
      :returns "DependencyStatus"
      :logic "DAG traversal: check each dep status (done=ok, failed/skipped=blocked, else=pending)")

    (op find_downstream_tasks
      :kind custom
      :where ((task_id "&str"))
      :returns "Vec<String>"
      :logic "BFS graph traversal over depends_on JSON arrays")

    (op retry_board_task
      :kind custom
      :where ((task_id "&str"))
      :params ((reset_downstream "bool"))
      :returns "Vec<String>"
      :logic "reset target + optionally cascade-reset all downstream")

    (op board_summary
      :kind custom
      :params ((since "Option<&str>"))
      :returns "serde_json::Value"
      :logic "multi-table aggregation: status counts, pending questions, new KB entries")

    (op search_board_tasks
      :kind custom
      :params ((input "&BoardSearchInput"))
      :returns "BoardSearchResult"
      :logic "dynamic SQL builder with query/project/category/status/parent_id filters")

    (op get_board_tasks_with_context
      :kind custom
      :params ((ids "&[String]") (include_children "bool"))
      :returns "Vec<BoardTaskWithContext>"
      :logic "3-query batch: main tasks → children → notes, assembled into tree")

    (op count_open_tasks_by_priority
      :kind custom
      :params ((priorities "&[&str]"))
      :returns "i64"
      :logic "dynamic IN clause from priority list")

    (op query_board_tasks_in_range
      :kind custom
      :params ((time_col "&str") (since "&str") (until "&str"))
      :returns "Vec<serde_json::Value>"
      :logic "whitelist column name, dynamic SQL, JSON row serialization")

    (op query_board_tasks_in_range_with_status
      :kind custom
      :params ((status "&str") (since "&str") (until "&str"))
      :returns "Vec<serde_json::Value>"
      :logic "fixed SQL with 3 params, JSON row serialization"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: board_task_notes                                  │
  ;; └──────────────────────────────────────────────────────────┘
  (table board_task_notes
    (col id         :type text  :pk)
    (col task_id    :type text  :fk board_tasks :on-delete cascade)
    (col content    :type text)
    (col note_type  :type text  :default "note")
    (col author     :type text  :nullable)
    (col created_at :type timestamptz :default-now)

    (idx idx_board_task_notes_task :cols (task_id))

    (row-type BoardTaskNoteRow
      :convert row_to_board_task_note :to BoardTaskNote
      :enum-map ((note_type board_note_type_from_str BoardNoteType)))

    (op insert_board_task_note
      :kind insert
      :auto (id :via "uuid::Uuid::new_v4().to_string()")
      :auto (created_at :via "chrono::Utc::now().to_rfc3339()")
      :params ((task_id text) (content text) (note_type text) (author text))
      :returns "BoardTaskNote")

    (op get_board_task_notes
      :kind select-many
      :where ((task_id text))
      :order "created_at ASC"
      :returns "Vec<BoardTaskNote>")

    (op get_board_task_with_notes
      :kind custom
      :where ((id "&str"))
      :returns "Option<BoardTaskWithNotes>"
      :logic "get_board_task → get_board_task_notes → compose"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Helpers: enum ↔ string converters                        │
  ;; └──────────────────────────────────────────────────────────┘
  (helpers
    (fn board_task_status_to_str :param (BoardTaskStatus) :returns "&'static str"
      :map ((Open "open") (Running "running") (Verifying "verifying")
            (Done "done") (Blocked "blocked") (Failed "failed") (Skipped "skipped")))

    (fn board_task_status_from_str :param ("&str") :returns BoardTaskStatus
      :map (("open" Open) ("running" Running) ("verifying" Verifying)
            ("done" Done) ("blocked" Blocked) ("failed" Failed) ("skipped" Skipped))
      :default Open)

    (fn board_note_type_to_str :param (BoardNoteType) :returns "&'static str"
      :map ((Progress "progress") (Summary "summary") (Note "note")))

    (fn board_note_type_from_str :param ("&str") :returns BoardNoteType
      :map (("progress" Progress) ("summary" Summary) ("note" Note))
      :default Note)))
