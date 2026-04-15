;; MissionD — Pillar: db-agents
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

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

