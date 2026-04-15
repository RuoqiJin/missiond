;; MissionD — Pillar: db-pipeline
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

  (pillar db-pipeline
    (purpose "conversation turns, AST sync, beacons, backfill phases, watermarks, translations")

    ;; ── Conversation Turns (S3 Tagger & Chunker pipeline stage) ──────────────
    (component turn-tables
      :target "crates/missiond-core/src/db/pg/conversation.rs"
      :migration ("20260322100000_conversation_turns.sql"
                  "20260409000000_turn_embedding_digest.sql"
                  "20260409100000_turn_skeleton.sql")

      (table conversation_turns
        (col id              :type bigserial :pk)
        (col session_id      :type text :not-null :fk conversations.id :on-delete cascade)
        (col turn_idx        :type integer :not-null)
        (col start_message_id :type bigint :not-null :fk conversation_messages.id)
        (col end_message_id  :type bigint :not-null :fk conversation_messages.id)

        ;; Pre-extracted summary fields (pure rules, no LLM)
        (col user_content    :type text :comment "User message text, truncated 2000 chars")
        (col tool_names      :type text :comment "CSV: Bash,Read,Edit")
        (col tool_call_count :type integer :default 0)
        (col message_count   :type integer :default 0)
        (col has_code_change :type boolean :default false)
        (col has_mcp_call    :type boolean :default false)
        (col started_at      :type text)
        (col ended_at        :type text)

        ;; Downstream fields (S4 Embedder / Intent Analyst)
        (col topic           :type text)
        (col intent_group_id :type bigint)

        ;; Embedding digest fields (commit 21a3b63 — 20260409000000)
        (col files_read    :type text :nullable
          :comment "short file names extracted from Read tool calls via regex")
        (col files_changed :type text :nullable
          :comment "short file names extracted from Write/Edit tool calls via regex")
        (col outcome       :type text :nullable
          :comment "last assistant non-tool text — embedding signal for what was accomplished")

        ;; Turn skeleton (commit dc45dcb — 20260409100000)
        (col skeleton      :type text :nullable
          :comment "JSON index: {user_msg_id, tool_calls:[{tool,file,req_msg_id,res_msg_id}], outcome_msg_id}; enables on-demand selective message retrieval instead of loading all 68+ messages")

        (unique (session_id turn_idx))
        (index idx_ct_session      :cols (session_id))
        (index idx_ct_session_idx  :cols (session_id turn_idx))
        (index idx_ct_start_msg    :cols (start_message_id))
        (index idx_ct_end_msg      :cols (end_message_id))

        (op get-last-turn-end-msg (binds session_id) :returns "Option<i64>")
        (op get-max-turn-idx      (binds session_id) :returns "Option<i32>")
        (op insert-batch
          (binds session_id base_idx "Vec<RawTurn>")
          :note "INSERT ON CONFLICT (session_id, turn_idx) DO UPDATE — idempotent upsert")
        (op missing-embeddings
          :where "NOT EXISTS (SELECT 1 FROM conversation_turns ct WHERE ct.session_id = c.id)"
          :returns "Vec<session_id>"))

      ;; TurnBuilder — logic inside tagger_chunker.rs (commit 21a3b63 refactor)
      (struct RawTurn
        :target "crates/missiond-core/src/types/conversation.rs"
        (field turn_idx      :type i32)
        (field start_message_id :type i64)
        (field end_message_id   :type i64)
        (field user_content  :type "Option<String>")
        (field tool_names    :type "Vec<String>")
        (field tool_call_count :type i32)
        (field message_count   :type i32)
        (field has_code_change :type bool)
        (field has_mcp_call    :type bool)
        (field started_at    :type "Option<String>")
        (field ended_at      :type "Option<String>")
        (field files_read    :type "Option<String>" :added "21a3b63")
        (field files_changed :type "Option<String>" :added "21a3b63")
        (field outcome       :type "Option<String>" :added "21a3b63")
        (field skeleton      :type "Option<String>" :added "dc45dcb"))

      (note "build_turn_embedding_text (commit 21a3b63): rewrote from natural-language to structured keyword-dense format: 'project:X files:Y intent:Z outcome:W' — optimized for MCP vector search by Claude Code agents. Filters <task-notification> noise turns."))

    (component ast-tables
      :target "crates/missiond-core/src/db/ast.rs"

      (table ast_nodes
        (col id :type uuid :pk)
        (col file_path :type text :not-null)
        (col symbol_name :type text :not-null)
        (col kind :type text :not-null
          :enum ("function" "struct" "enum" "trait" "impl" "mod" "const" "type"))
        (col start_line :type integer :not-null)
        (col end_line :type integer :not-null)
        (col signature :type text)
        (col is_exported :type boolean :default false)
        (col parent_symbol :type text)
        (col embedding :type bytea)
        (col beacon_tags :type jsonb)
        (col commit_hash :type text)
        (col created_at :type timestamptz :not-null)

        (op sync-file (binds file_path "Vec<CodeNode>" commit_hash))
        (op delete-file (binds file_path))
        (op search (binds query kind is_exported limit) :fts true)
        (op search-ranked (binds query embedding limit) :hybrid true)
        (op get-file-nodes (binds file_path))
        (op find-related (binds symbol_name kind))
        (op get-file-commit (binds file_path))
        (op stats :aggregate true)
        (op set-embedding (binds id embedding))
        (op find-unembedded (limit batch_size))
        (op load-all-embeddings :returns "Vec<(id, embedding)>")
        (op get-search-hit (binds id))
        (op get-node (binds id))
        (op module-summaries :returns "Vec<(module, count)>")))

    (component beacon-tables
      :target "crates/missiond-core/src/db/beacon.rs"
      :gen-target "crates/missiond-core/src/db/gen/pipeline.rs"

      (table beacons
        (col id :type uuid :pk)
        (col tag :type text :unique :not-null)
        (col description :type text)
        (col harvest_count :type integer :default 0)
        (col created_at :type timestamptz :not-null)
        (op list)
        (op ensure (binds tag) :upsert-on tag)
        (op increment-harvest (binds tag))
        (op set-description (binds tag description))
        (op search (binds query) :fts true)
        (op cleanup-empty (where harvest_count=0)))

      (table beacon_nodes
        (col id :type uuid :pk)
        (col beacon_id :type uuid :fk beacons.id)
        (col file_path :type text :not-null)
        (col symbol_name :type text)
        (col line :type integer)
        (col annotation :type text)
        (op upsert (binds beacon_id file_path symbol_name line))
        (op map (binds beacon_id))
        (op delete-file (binds file_path))
        (op annotate (binds id annotation))))

    (component pipeline-tables
      :gen-target "crates/missiond-core/src/db/gen/pipeline.rs"

      (table backfill_phases
        (col phase_name :type text :pk)
        (col status :type text :default "pending" :enum ("pending" "running" "completed" "failed"))
        (col progress :type integer :default 0)
        (col total :type integer)
        (col started_at :type timestamptz)
        (col completed_at :type timestamptz)
        (col error :type text)
        (op get (binds phase_name))
        (op list)
        (op start (binds phase_name total))
        (op update-progress (binds phase_name progress))
        (op complete (binds phase_name))
        (op record-failure (binds phase_name error))
        (op retryable-failures)
        (op clear-failure (binds phase_name)))

      (table watermarks
        (col key :type text :pk)
        (col time_value :type timestamptz)
        (col msg_id_value :type bigint)
        (op advance-time (binds key time_value))
        (op advance-msg-id (binds key msg_id_value)))

      (table labels
        (col key :type text :pk)
        (col value :type text :not-null)
        (op set (binds key value)))

      (table translations
        (col id :type uuid :pk)
        (col message_id :type bigint)
        (col language :type text :not-null)
        (col content :type text :not-null)
        (col created_at :type timestamptz :not-null)
        (op insert (binds message_id language content))
        (op has (binds message_id language) :returns bool)))

    (component vision-tables
      :target "crates/missiond-core/src/db/vision.rs"

      (table image_descriptions
        (col id :type uuid :pk)
        (col image_hash :type text :unique :not-null)
        (col description :type text :not-null)
        (col created_at :type timestamptz :not-null)
        (op save (binds image_hash description) :upsert-on image_hash)
        (op count :aggregate true))))

