;; ══════════════════════════════════════════════════════════════
;; MissionD DB Layer — Compute Infrastructure (SQLite)
;; slot_sessions, slot_tasks, daemon_state, tasks, inbox, events
;; ══════════════════════════════════════════════════════════════

(pattern crud-gateway
  :gateway MissionDB
  :pool    "Mutex<Connection>"
  :backend sqlite
  :target  "crates/missiond-core/src/db/gen_compute.rs"

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: slot_sessions                                     │
  ;; └──────────────────────────────────────────────────────────┘
  (table slot_sessions
    (col slot_id    :type text   :pk)
    (col session_id :type text)
    (col updated_at :type bigint)

    (op get_slot_session
      :kind custom
      :where ((slot_id "&str"))
      :returns "Option<String>"
      :logic "SELECT session_id WHERE slot_id")

    (op set_slot_session
      :kind upsert
      :params ((slot_id text) (session_id text) (updated_at "i64"))
      :on-conflict "(slot_id) DO UPDATE SET session_id = ?2, updated_at = ?3"
      :returns "()")

    (op delete_slot_session
      :kind delete
      :where ((slot_id text))
      :returns "()")

    (op get_all_slot_sessions
      :kind custom
      :returns "Vec<(String, String)>"
      :logic "SELECT slot_id, session_id FROM slot_sessions")

    (op get_slot_for_session
      :kind custom
      :where ((session_id "&str"))
      :returns "Option<String>"
      :logic "SELECT slot_id WHERE session_id")

    (op cleanup_pty_placeholder
      :kind custom
      :where ((slot_id "&str"))
      :returns "()"
      :logic "UPDATE session_id WHERE starts_with pty-placeholder"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: slot_tasks                                        │
  ;; └──────────────────────────────────────────────────────────┘
  (table slot_tasks
    (col id               :type text    :pk)
    (col slot_id          :type text)
    (col task_type        :type text)
    (col status           :type text    :default "pending")
    (col prompt_summary   :type text    :nullable)
    (col source_sessions  :type text    :nullable)
    (col output_count     :type integer :default "0")
    (col created_at       :type text)
    (col started_at       :type text    :nullable)
    (col completed_at     :type text    :nullable)
    (col duration_ms      :type integer :nullable)
    (col error            :type text    :nullable)
    (col conversation_id  :type text    :nullable)

    (idx idx_slot_tasks_slot    :cols (slot_id))
    (idx idx_slot_tasks_type    :cols (task_type))
    (idx idx_slot_tasks_created :cols (created_at))

    (row-type SlotTaskRow
      :convert row_to_slot_task :to SlotTask)

    (op insert_slot_task
      :kind insert-struct
      :params ((task "&SlotTask"))
      :returns "()")

    (op slot_task_set_running
      :kind update
      :where ((id text))
      :set ((status :literal "'running'")
            (started_at :via "datetime('now')"))
      :returns "()")

    (op slot_task_set_completed
      :kind update
      :where ((id text))
      :set ((status :literal "'completed'")
            (completed_at :via "datetime('now')")
            (output_count "i64"))
      :returns "()")

    (op slot_task_set_failed
      :kind update
      :where ((id text))
      :set ((status :literal "'failed'")
            (completed_at :via "datetime('now')")
            (error "&str"))
      :returns "()")

    (op get_running_slot_task
      :kind custom
      :where ((slot_id "&str"))
      :returns "Option<String>"
      :logic "SELECT id WHERE slot_id AND status=running")

    (op last_completed_slot_task_at
      :kind custom
      :params ((task_type "&str"))
      :returns "Option<i64>"
      :logic "SELECT MAX epoch from completed_at WHERE task_type")

    (op list_slot_tasks
      :kind custom
      :params ((slot_id "Option<&str>") (status "Option<&str>") (task_type "Option<&str>") (limit "i64"))
      :returns "Vec<SlotTask>"
      :logic "dynamic WHERE with optional filters + ORDER BY created_at DESC")

    (op slot_task_stats
      :kind custom
      :params ((slot_id "Option<&str>"))
      :returns "serde_json::Value"
      :logic "multi-dimension aggregation: by_status, by_type, avg_duration")

    (op reap_stale_slot_tasks
      :kind custom
      :params ((max_age_secs "i64"))
      :returns "usize"
      :logic "UPDATE running → failed WHERE age > max_age_secs")

    (op find_stale_decision_tasks
      :kind custom
      :params ((max_age_secs "i64"))
      :returns "Vec<SlotTask>"
      :logic "SELECT pending+running decision tasks older than threshold")

    (op cleanup_orphan_slot_tasks
      :kind custom
      :returns "usize"
      :logic "UPDATE running → failed WHERE slot not in active slots"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: daemon_state (KV store)                           │
  ;; └──────────────────────────────────────────────────────────┘
  (table daemon_state
    (col key        :type text  :pk)
    (col value      :type text)
    (col updated_at :type text)

    (op daemon_state_get
      :kind custom
      :where ((key "&str"))
      :returns "Option<i64>"
      :logic "SELECT value, parse as i64")

    (op daemon_state_set
      :kind upsert
      :params ((key text) (value text) (updated_at text))
      :on-conflict "(key) DO UPDATE SET value = ?2, updated_at = ?3"
      :returns "()"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: tasks (task queue)                                │
  ;; └──────────────────────────────────────────────────────────┘
  (table tasks
    (col id          :type text    :pk)
    (col role        :type text)
    (col prompt      :type text)
    (col status      :type text    :default "queued")
    (col manual      :type boolean :default "0")
    (col slot_id     :type text    :nullable)
    (col session_id  :type text    :nullable)
    (col result      :type text    :nullable)
    (col error       :type text    :nullable)
    (col created_at  :type bigint)
    (col started_at  :type bigint  :nullable)
    (col finished_at :type bigint  :nullable)
    (col notified_at :type bigint  :nullable)

    (idx idx_tasks_status  :cols (status))
    (idx idx_tasks_role    :cols (role))
    (idx idx_tasks_created :cols (created_at))

    (row-type TaskRow
      :convert row_to_task :to Task
      :enum-map ((status task_status_from_str TaskStatus)))

    (op insert_task
      :kind insert-struct
      :params ((task "&Task"))
      :returns "()")

    (op get_task
      :kind select-one
      :where ((id text))
      :returns "Option<Task>")

    (op get_tasks_by_status
      :kind custom
      :params ((status "TaskStatus"))
      :returns "Vec<Task>"
      :logic "WHERE status = status.as_str()")

    (op get_queued_tasks_by_role
      :kind custom
      :where ((role "&str"))
      :returns "Vec<Task>"
      :logic "WHERE role AND status=queued ORDER BY created_at ASC")

    (op requeue_running_tasks_for_slot
      :kind custom
      :where ((slot_id "&str"))
      :returns "usize"
      :logic "UPDATE running → queued WHERE slot_id, clear session_id")

    (op get_all_tasks
      :kind custom
      :params ((limit "i64"))
      :returns "Vec<Task>"
      :logic "SELECT * ORDER BY created_at DESC LIMIT")

    (op ack_completed_tasks
      :kind custom
      :params ((since "Option<i64>"))
      :returns "Vec<Task>"
      :logic "SELECT+UPDATE: fetch completed/failed unnotified, set notified_at")

    (op update_task
      :kind custom
      :where ((id "&str"))
      :params ((update "&TaskUpdate"))
      :returns "()"
      :logic "dynamic SET with optional fields"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: inbox                                             │
  ;; └──────────────────────────────────────────────────────────┘
  (table inbox
    (col id         :type text    :pk)
    (col task_id    :type text)
    (col from_role  :type text)
    (col content    :type text)
    (col read       :type boolean :default "0")
    (col created_at :type bigint)

    (idx idx_inbox_read    :cols (read))
    (idx idx_inbox_created :cols (created_at))

    (row-type InboxMessageRow
      :convert row_to_inbox_message :to InboxMessage)

    (op insert_inbox_message
      :kind insert-struct
      :params ((msg "&InboxMessage"))
      :returns "()")

    (op get_inbox_messages
      :kind custom
      :params ((unread_only "bool") (limit "i64"))
      :returns "Vec<InboxMessage>"
      :logic "optional WHERE read=0, ORDER BY created_at DESC")

    (op mark_inbox_read
      :kind update
      :where ((id text))
      :set ((read :literal "1"))
      :returns "()"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Table: events                                            │
  ;; └──────────────────────────────────────────────────────────┘
  (table events
    (col id        :type bigint  :pk)
    (col task_id   :type text)
    (col type      :type text)
    (col data      :type jsonb   :nullable)
    (col timestamp :type bigint)

    (idx idx_events_task      :cols (task_id))
    (idx idx_events_timestamp :cols (timestamp))

    (row-type TaskEventRow
      :convert row_to_event :to TaskEvent
      :enum-map ((type event_type_from_str EventType)))

    (op insert_event
      :kind custom
      :params ((task_id "&str") (event_type "EventType") (data "Option<serde_json::Value>"))
      :returns "i64"
      :logic "INSERT with auto-increment ID + epoch timestamp")

    (op get_events_by_task
      :kind select-many
      :where ((task_id text))
      :order "timestamp ASC"
      :returns "Vec<TaskEvent>"))

  ;; ── Helpers ──
  (helpers
    (fn task_status_to_str :param (TaskStatus) :returns "&'static str"
      :map ((Queued "queued") (Running "running") (Completed "completed") (Failed "failed")))

    (fn task_status_from_str :param ("&str") :returns TaskStatus
      :map (("queued" Queued) ("running" Running) ("completed" Completed) ("failed" Failed))
      :default Queued)

    (fn event_type_to_str :param (EventType) :returns "&'static str"
      :map ((TaskCreated "task_created") (TaskStarted "task_started")
            (TaskCompleted "task_completed") (TaskFailed "task_failed")
            (MessageSent "message_sent") (MessageReceived "message_received")))

    (fn event_type_from_str :param ("&str") :returns EventType
      :map (("task_created" TaskCreated) ("task_started" TaskStarted)
            ("task_completed" TaskCompleted) ("task_failed" TaskFailed)
            ("message_sent" MessageSent) ("message_received" MessageReceived))
      :default TaskCreated)))
