;; MissionD — Pillar: db-observability
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

  (pillar db-observability
    (purpose "audit trail, tool calls, timeline, LLM traces, retrospectives")

    (component audit-tables
      :pattern crud-gateway
      :target "crates/missiond-core/src/db/audit.rs"
      :gen-target "crates/missiond-core/src/db/gen/audit.rs"

      (table tool_calls
        (col id :type uuid :pk)
        (col session_id :type text :not-null)
        (col conversation_id :type text)
        (col tool_name :type text :not-null)
        (col input :type jsonb)
        (col output :type jsonb)
        (col status :type text)
        (col error :type text)
        (col created_at :type timestamptz :not-null)
        (col completed_at :type timestamptz)

        (op insert (binds session_id conversation_id tool_name input))
        (op insert-batch (binds "Vec<tool_call>"))
        (op update-output (binds id output status error completed_at))
        (op select-one (binds id))
        (op select-by-session (binds session_id limit offset))
        (op stats (binds session_id) :aggregate true)
        (op count-pending (binds session_id))
        (op sessions-with-pending :returns "Vec<session_id>")
        (op for-detailed-analysis (binds session_id))
        (op with-status-timeline (binds session_id))
        (op name-sequence (binds session_id))
        (op error-samples (binds tool_name limit))
        (op retro-tool-stats (binds session_id))
        (op retro-meta (binds session_id))
        (op retro-repeat-patterns (binds session_id))
        (op retro-high-error-tools (binds session_id))
        (op first-user-message (binds session_id))
        (op for-backfill (binds session_id)))

      (table conversation_events
        (col id :type bigint :pk :autoincrement)
        (col conversation_id :type text :not-null)
        (col event_type :type text :not-null)
        (col data :type jsonb)
        (col created_at :type timestamptz :not-null)
        (op insert-batch (binds "Vec<event>"))
        (op select (binds conversation_id event_type since until limit))
        (op agent-trajectory (binds conversation_id))
        (op type-summary (binds conversation_id) :aggregate true)
        (op cleanup-old (binds threshold))
        (op sessions-with-events (binds since limit)))

      (table retrospective_results
        (col session_id :type text :pk)
        (col result :type jsonb :not-null)
        (col created_at :type timestamptz :not-null)
        (op save (binds session_id result))
        (op has (binds session_id) :returns bool)
        (op select-one (binds session_id))
        (op list (binds limit offset))
        (op needing-retro (where NOT EXISTS))
        (op for-backfill)))

    (component timeline-tables
      :target "crates/missiond-core/src/db/timeline.rs"

      (table system_timeline
        (col id :type bigint :pk :autoincrement)
        (col event_type :type text :not-null)
        (col slot_name :type text)
        (col session_id :type text)
        (col summary :type text)
        (col data :type jsonb)
        (col trace_id :type text)
        (col created_at :type timestamptz :not-null)
        (op insert (binds event_type slot_name session_id summary data trace_id))
        (op query (binds event_type slot_name since until limit offset))
        (op update-summary (binds id summary))
        (op stats (binds since) :aggregate true)
        (op traces (binds trace_id))))

    (component gemini-log-tables
      :target "crates/missiond-core/src/db/gemini_log.rs"

      (table gemini_requests
        (col id :type uuid :pk)
        (col model :type text :not-null)
        (col purpose :type text)
        (col prompt_preview :type text)
        (col response_preview :type text)
        (col input_tokens :type integer)
        (col output_tokens :type integer)
        (col status :type text)
        (col duration_ms :type integer)
        (col created_at :type timestamptz :not-null)
        (col completed_at :type timestamptz)
        (op insert-started (binds model purpose prompt_preview))
        (op update-completed (binds id response_preview input_tokens output_tokens status duration_ms))
        (op get-content (binds id))
        (op query (binds model purpose since until limit))
        (op stats (binds since) :aggregate true)
        (op cleanup (binds threshold)))

      (table gemini_file_cache
        (col file_hash :type text :pk)
        (col uri :type text :not-null)
        (col expires_at :type timestamptz :not-null)
        (op get (binds file_hash))
        (op put (binds file_hash uri expires_at))
        (op gc (where expires_at < now()))))

    (component incident-tables
      :target "crates/missiond-core/src/db/incident.rs"

      (table incidents
        (col id :type uuid :pk)
        (col incident_type :type text :not-null)
        (col severity :type text :not-null)
        (col message :type text :not-null)
        (col data :type jsonb)
        (col created_at :type timestamptz :not-null)
        (op insert (binds incident_type severity message data))
        (op has-recent (binds incident_type since) :returns bool)
        (op list (binds severity since limit)))

      (table token_usage
        (col id :type uuid :pk)
        (col session_id :type text)
        (col model :type text)
        (col input_tokens :type integer)
        (col output_tokens :type integer)
        (col cost_usd :type float8)
        (col created_at :type timestamptz :not-null)
        (op insert (binds session_id model input_tokens output_tokens))
        (op stats (binds since) :aggregate true)))

    (component narration-tables
      :target "crates/missiond-core/src/db/narration.rs"

      (table narration_cursors
        (col conversation_id :type text :pk)
        (col last_message_id :type bigint)
        (col processing :type boolean :default false)
        (col updated_at :type timestamptz)
        (op mark-processing (binds conversation_id)))

      (table message_narrations
        (col id :type bigint :pk :autoincrement)
        (col message_id :type bigint :fk conversation_messages.id)
        (col narration :type text :not-null)
        (col created_at :type timestamptz :not-null))))

