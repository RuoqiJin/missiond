;; ══════════════════════════════════════════════════════════════
;; MissionD Domain Types — Enums + Core Structs
;; Forge: Lisp → DomainTypesDecl → gen_types.rs
;; ══════════════════════════════════════════════════════════════

(domain-types
  :target "crates/missiond-core/src/types/gen_types.rs"

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Enums with as_str() / from_str()                         │
  ;; └──────────────────────────────────────────────────────────┘

  (enum BoardTaskStatus :serde-rename "lowercase"
    :doc "Board task execution status"
    (variant Open "open")
    (variant Running "running")
    (variant Verifying "verifying")
    (variant Done "done")
    (variant Blocked "blocked")
    (variant Failed "failed")
    (variant Skipped "skipped"))

  (enum BoardNoteType :serde-rename "lowercase"
    :doc "Board task note type"
    (variant Progress "progress")
    (variant Summary "summary")
    (variant Note "note"))

  (enum EngineeringPhase :serde-rename "snake_case"
    :doc "Phase in the engineering task flow state machine"
    (variant Investigate "investigate")
    (variant ConsultGemini1 "consult_gemini1")
    (variant Plan "plan")
    (variant ConsultGemini2 "consult_gemini2")
    (variant Execute "execute")
    (variant Finalize "finalize")
    (variant Done "done"))

  (enum TaskStatus :serde-rename "lowercase"
    :doc "Background task queue status"
    (variant Queued "queued")
    (variant Running "running")
    (variant Completed "completed")
    (variant Failed "failed"))

  (enum EventType :serde-rename "snake_case"
    :doc "Task event type"
    (variant TaskCreated "task_created")
    (variant TaskStarted "task_started")
    (variant TaskCompleted "task_completed")
    (variant TaskFailed "task_failed")
    (variant MessageSent "message_sent")
    (variant MessageReceived "message_received"))

  (enum AsyncJobStatus :serde-rename "lowercase"
    :doc "Async job execution status"
    (variant Pending "pending")
    (variant Running "running")
    (variant Completed "completed")
    (variant Failed "failed")
    (variant Cancelled "cancelled"))

  (enum AgentQuestionStatus :serde-rename "lowercase"
    :doc "Agent question lifecycle status"
    (variant Pending "pending")
    (variant Answered "answered")
    (variant Dismissed "dismissed")
    (variant Expired "expired")
    (variant Harvested "harvested"))

  (enum IncidentSeverity :serde-rename "lowercase"
    :doc "Incident severity level"
    (variant Critical "critical")
    (variant High "high")
    (variant Medium "medium")
    (variant Low "low")
    (variant Info "info"))

  (enum IncidentSource :serde-rename "lowercase"
    :doc "Incident source system"
    (variant Monitoring "monitoring")
    (variant AlertManager "alert_manager")
    (variant Manual "manual")
    (variant System "system")
    (variant Webhook "webhook"))

  (enum DependencyStatus :copy false
    :doc "DAG dependency resolution result"
    :as-str false :from-str false)

  (enum CliEngine :serde-rename "lowercase"
    :doc "CLI engine for PTY slots"
    (variant ClaudeCode "claude_code")
    (variant ClaudeMd "claude_md"))

  (enum Lifecycle :serde-rename "lowercase"
    :doc "Slot lifecycle mode"
    (variant Persistent "persistent")
    (variant Transient "transient"))

  (enum SlotTrait :serde-rename "snake_case"
    :doc "Slot capability trait"
    (variant Memory "memory")
    (variant Sonnet "sonnet")
    (variant AutoPilot "auto_pilot")
    (variant Decision "decision")
    (variant Strategy "strategy"))

  ;; ┌──────────────────────────────────────────────────────────┐
  ;; │ Core Structs                                             │
  ;; └──────────────────────────────────────────────────────────┘

  (struct FlowContext :serde-rename "camelCase" :default "true"
    :doc "Context passed between engineering flow phases"
    (field investigation_report "Option<String>")
    (field gemini_advice_1 "Option<String>")
    (field execution_plan "Option<String>")
    (field gemini_advice_2 "Option<String>")
    (field execution_result "Option<String>")
    (field commit_hash "Option<String>"))

  (struct BoardTask :serde-rename "camelCase"
    :doc "Personal task board entry"
    (field id TaskId)
    (field title String)
    (field description String)
    (field status BoardTaskStatus)
    (field priority String)
    (field category String)
    (field project "Option<String>")
    (field server "Option<String>")
    (field due_date "Option<String>")
    (field parent_id "Option<TaskId>")
    (field assignee "Option<String>")
    (field auto_execute bool)
    (field prompt_template "Option<String>")
    (field hidden bool)
    (field retry_count i64)
    (field max_retries i64)
    (field order_idx i64)
    (field created_at String)
    (field updated_at String)
    (field claim_executor_id "Option<String>")
    (field claim_executor_type "Option<String>")
    (field claimed_at "Option<String>")
    (field flow_phase "Option<String>")
    (field flow_context "Option<String>")
    (field flow_template "Option<String>")
    (field depends_on "Vec<TaskId>")
    (field lease_expires_at "Option<String>")
    (field dedupe_key "Option<String>")
    (field timeout_secs "Option<i64>")
    (field context_intent "Option<String>")
    (field notes_count i64))

  (struct CompactBoardTask :serde-rename "camelCase"
    :doc "Compact board task for search results"
    (field id TaskId)
    (field title String)
    (field status BoardTaskStatus)
    (field priority String)
    (field parent_id "Option<TaskId>")
    (field project "Option<String>")
    (field category String))

  (struct BoardTaskNote :serde-rename "camelCase"
    :doc "Note attached to a board task"
    (field id String)
    (field task_id String)
    (field content String)
    (field note_type BoardNoteType)
    (field author "Option<String>")
    (field created_at String))

  (struct CreateBoardTaskInput :serde-rename "camelCase" :default "true"
    :doc "Input for creating a board task"
    (field title String)
    (field description "Option<String>")
    (field priority "Option<String>")
    (field category "Option<String>")
    (field project "Option<String>")
    (field server "Option<String>")
    (field due_date "Option<String>")
    (field parent_id "Option<String>")
    (field assignee "Option<String>")
    (field auto_execute "Option<bool>")
    (field prompt_template "Option<String>")
    (field hidden "Option<bool>")
    (field depends_on "Option<Vec<String>>")
    (field dedupe_key "Option<String>")
    (field timeout_secs "Option<i64>")
    (field context_intent "Option<String>"))

  (struct UpdateBoardTaskInput :serde-rename "camelCase" :default "true"
    :doc "Input for updating a board task"
    (field title "Option<String>")
    (field description "Option<String>")
    (field status "Option<String>")
    (field priority "Option<String>")
    (field category "Option<String>")
    (field project "Option<String>")
    (field server "Option<String>")
    (field due_date "Option<String>")
    (field parent_id "Option<String>")
    (field assignee "Option<String>")
    (field auto_execute "Option<bool>")
    (field prompt_template "Option<String>")
    (field hidden "Option<bool>")
    (field order_idx "Option<i64>")
    (field flow_phase "Option<String>")
    (field flow_context "Option<String>")
    (field flow_template "Option<String>")
    (field depends_on "Option<Vec<String>>")
    (field claim_executor_id "Option<String>")
    (field claim_executor_type "Option<String>"))

  (struct Conversation :serde-rename "camelCase"
    :doc "Claude Code conversation session"
    (field id String)
    (field project "Option<String>")
    (field slot_id "Option<String>")
    (field source String)
    (field model "Option<String>")
    (field git_branch "Option<String>")
    (field jsonl_path "Option<String>")
    (field parent_session_id "Option<String>")
    (field task_id "Option<String>")
    (field message_count i64)
    (field started_at String)
    (field ended_at "Option<String>")
    (field status String)
    (field analyzed_at "Option<String>")
    (field analysis_version i32)
    (field analysis_retries i32)
    (field deep_analyzed_message_id i64)
    (field chat_type "Option<String>")
    (field conversation_type String)
    (field updated_at "Option<String>")
    (field llm_summary "Option<String>")
    (field embedding_provider "Option<String>")
    (field session_timeline "Option<String>")
    (field timeline_built_at "Option<String>"))

  (struct ConversationMessage :serde-rename "camelCase"
    :doc "Single message in a conversation"
    (field id i64)
    (field session_id String)
    (field role String)
    (field content String)
    (field raw_content "Option<String>")
    (field message_uuid "Option<String>")
    (field parent_uuid "Option<String>")
    (field model "Option<String>")
    (field timestamp String)
    (field metadata "Option<String>")
    (field tool_name "Option<String>")
    (field raw_role "Option<String>")
    (field content_types "Option<String>")
    (field has_image bool)
    (field has_tool_use bool)
    (field has_tool_result bool)
    (field token_count "Option<i64>")
    (field seq "Option<i64>")
    (field role_display "Option<String>"))

  (struct KnowledgeEntry :serde-rename "camelCase"
    :doc "Knowledge base entry"
    (field id String)
    (field category String)
    (field key String)
    (field summary String)
    (field detail "Option<serde_json::Value>")
    (field source String)
    (field confidence f64)
    (field access_count i64)
    (field created_at String)
    (field updated_at String)
    (field last_accessed_at "Option<String>")
    (field linked_task_id "Option<String>")
    (field kb_type String :default "true")
    (field context_snippet "Option<String>")
    (field scope_task_id "Option<String>")
    (field utility_score f64 :default "true"))

  (struct KBRememberInput :serde-rename "camelCase"
    :doc "Input for remembering knowledge"
    (field category String)
    (field key String)
    (field summary String)
    (field detail "Option<serde_json::Value>" :default "true")
    (field source "Option<String>" :default "true")
    (field confidence "Option<f64>" :default "true"))

  (struct KBEdge :serde-rename "camelCase"
    :doc "Knowledge graph edge"
    (field source_id String)
    (field target_id String)
    (field relation_type String)
    (field weight f64)
    (field created_at String))

  (struct Task :serde-rename "camelCase"
    :doc "Background task in task queue"
    (field id String)
    (field role String)
    (field prompt String)
    (field status TaskStatus)
    (field slot_id "Option<String>")
    (field session_id "Option<String>")
    (field result "Option<String>")
    (field error "Option<String>")
    (field created_at i64)
    (field started_at "Option<i64>")
    (field finished_at "Option<i64>"))

  (struct InboxMessage :serde-rename "camelCase"
    :doc "Inter-agent inbox message"
    (field id String)
    (field task_id String)
    (field from_role String)
    (field content String)
    (field read bool)
    (field created_at i64))

  (struct TaskEvent :serde-rename "camelCase"
    :doc "Task lifecycle event"
    (field id i64)
    (field task_id String)
    (field event_type EventType)
    (field data "Option<serde_json::Value>")
    (field timestamp i64))

  (struct AgentQuestion :serde-rename "camelCase"
    :doc "Agent decision point question"
    (field id String)
    (field task_id "Option<String>")
    (field slot_id "Option<String>")
    (field session_id "Option<String>")
    (field question String)
    (field context String)
    (field status AgentQuestionStatus)
    (field answer "Option<String>")
    (field created_at String)
    (field updated_at String)
    (field target String)
    (field options "Option<String>")
    (field decision_type String)
    (field retry_count i32)
    (field routing_trace "Option<String>"))

  (struct IncidentRow :serde-rename "camelCase"
    :doc "Incident record"
    (field id String)
    (field severity String)
    (field source String)
    (field title String)
    (field description String)
    (field server_id "Option<String>")
    (field raw_payload "Option<String>")
    (field board_task_id "Option<String>")
    (field dedupe_key String)
    (field created_at String))

  (struct DynamicSlot :serde-rename "camelCase"
    :doc "Dynamically spawned compute slot"
    (field id String)
    (field parent_slot_id String)
    (field template String)
    (field objective "Option<String>")
    (field config String)
    (field status String)
    (field termination_reason "Option<String>")
    (field created_at String)
    (field terminated_at "Option<String>")
    (field ttl_seconds i32)
    (field expires_at String)
    (field extend_count i32))

  (struct SkillTopic :serde-rename "camelCase"
    :doc "Skill topic definition"
    (field topic String)
    (field description "Option<String>")
    (field aka "Option<String>")
    (field allowed_tools "Option<String>")
    (field file_path String)
    (field hit_count i32)
    (field last_hit_at "Option<String>")
    (field fragment_count i32)
    (field total_lines i32)
    (field checksum "Option<String>")
    (field created_at String)
    (field updated_at String)
    (field requires_json "Option<String>")
    (field actions_json "Option<String>")
    (field context_hooks_json "Option<String>"))

  (struct SkillBlock :serde-rename "camelCase"
    :doc "Skill content block"
    (field id String)
    (field topic String)
    (field block_type String)
    (field title "Option<String>")
    (field content String)
    (field sort_order i32)
    (field status String)
    (field created_at String)
    (field updated_at String))

  (struct ToolCallRecord :serde-rename "camelCase"
    :doc "Tool call audit record"
    (field id String)
    (field session_id String)
    (field message_id "Option<i64>")
    (field tool_name String)
    (field input_summary "Option<String>")
    (field raw_input "Option<String>")
    (field output_summary "Option<String>")
    (field raw_output "Option<String>")
    (field status String)
    (field duration_ms "Option<i32>")
    (field timestamp String)))
