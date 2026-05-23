  (capability-governance-policy
    :desc "Lisp-owned capability audit policy; runtime review paths and protected lists are projections, not Rust-only constants."
    :review-sidecar ".missiond/v3/runtime/capability-usage-review.json"
    :protected-tool-patterns ["mission_execution"
                              "mission_intent"
                              "mission_forge_"
                              "mission_sys_"
                              "mission_daemon_update"
                              "mission_health"
                              "mission_power_control"
                              "mission_kb_ops"
                              "mission_audit"
                              "mission_pty_signal"
                              "mission_pty_confirm"
                              "mission_incident"]
    :protected-flow-patterns ["engineering"
                              "F-execution-log-governance"
                              "F-incident-reaction"
                              "F-capability-usage-monitoring"]
    :invariants
      ["mission_capability_usage snapshot/report/candidates/mark/ack MUST project review sidecar location and protected source/target policy from capability-governance-policy."
       "Protected pattern semantics stay explicit: tool patterns ending '_' are prefixes; other tool patterns are exact; flow patterns match exact or prefix."
       "A real MissionD project with .missiond but no capability-governance-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (mcp-tool-governance-policy
    :desc "Primary MCP tool families for agents; old public tools remain callable compatibility leaves, but agents should select by family first."
    :schema "missiond.mcp-tool-governance.v1"
    :primary-families [mission_board mission_workflow mission_workstation mission_context mission_memory mission_universe mission_ops mission_router mission_tool_directory]
    :directory-tool mission_tool_directory
    :max-primary-families 12
    :xjp-cli-mcp-parity
      ((authority tools/xjp-mcp)
       (operator-shell xjp-cli)
       (audit-command "xjp mcp parity --json")
       (rule "XJP MCP is the latest ClaudeCode/MissionD tool authority; xjp-cli is an operator shell and must expose parity gaps rather than implying it contains every deploy/router/storage/cloudflare tool."))
    :metadata-required [tool_family primary_action tier danger_level intent_examples preferred_surface compatibility_tools]
    :agent-rule "When unsure, call mission_tool_directory(action=\"recommend\", intent=...) before selecting a lower-level MCP tool. Tool families are a selection/readability layer; compatibility tools remain stable for existing workers."
    :invariants
      ["mission_tool_directory MUST expose list/recommend/lookup/explain/deprecated/guide actions over the primary tool-family catalog and remain read-only."
       "mission_agent_navigation owns catalog/review/feedback/suggest_entries; feedback may append only .missiond/v3/runtime/agent-navigation-review.json and must not mutate Board, KB, project registries, SSOT shards, or sibling repositories."
       "Public tools MAY remain numerous, but every high-frequency tool must map to a primary family and preferred surface."
       "Deprecated/raw tools MUST return a preferredFamily/preferredSurface hint instead of relying on operator memory."
       "MCP tool-family guide semantics must be read-only; the only allowed navigation write is mission_agent_navigation feedback appending its review sidecar."])

  (memory-provider-contract
    :schema "missiond.memory-provider.v1"
    :purpose "Make memory pluggable so MissionD Core can be open-sourced, multi-tenant, and multi-universe without carrying private conversation, KB, skill-evidence, embedding, or review-overlay data."
    :core-boundary "MissionD Core owns provider registry, scope resolution, query/write facades, context injection policy, and MCP compatibility; providers own memory data and retrieval internals."
    :scope-fields [tenant_id universe_id project_id user_id source_type source_id authority privacy_class review_state]
    :default-provider null-memory
    :providers
      ((provider null-memory
         :kind disabled
         :use-case open-source-default
         :capabilities []
         :rule "Open-source/default MissionD can run without private memory data; queries return explicit MEMORY_PROVIDER_DISABLED diagnostics.")
       (provider local-postgres-memory
         :kind local-postgres
         :use-case single-user-dev-compatible
         :capabilities [query remember review-overlay conversation-ingest skill-evidence export purge]
         :data-owner "local MissionD database compatibility tables"
         :rule "Current MissionD KB/conversation tables are a compatibility provider implementation, not the permanent MissionD Core memory model.")
       (provider xjp-memory
         :kind remote-service
         :use-case private-multi-universe
         :capabilities [query remember review-overlay conversation-ingest skill-evidence fts embedding rerank context-pack export purge]
         :runtime-env [MISSIOND_MEMORY_PROVIDER_URL MISSIOND_MEMORY_PROVIDER_TOKEN]
         :embedding-provider xjp-router
         :rerank-provider xjp-router
         :rule "Private deployments use xjp-memory for tenant/universe/project/user scoped memory, conversation history, skill evidence, embedding, rerank, and review overlay. Secrets and provider tokens stay in secret-store/env, never in Lisp."))
    :functions
      ((function memory-provider-registry
         :entry [V3-compiled-runtime env-config mission_memory.provider_status]
         :core ((step s1 :logic "load provider declarations and active provider selection from MISSIOND_MEMORY_PROVIDER_URL / MISSIOND_MEMORY_PROVIDER_MODE")
                (step s2 :logic "validate provider capabilities against requested operation")
                (step s3 :logic "call /v1/memory/provider_status for xjp-memory providers, or return explicit null/local compatibility diagnostics"))
         :egress [MemoryProviderConfig provider-status])
       (function memory-scope-resolution
         :entry [BoardTask project-registry user-request active-universe]
         :core ((step s1 :logic "resolve tenant/universe/project/user scope before every memory query or write")
                (step s2 :logic "reject unscoped global memory reads unless workflow explicitly asks for cross-universe audit")
                (step s3 :logic "attach scope fields to provider requests and task-result artifacts"))
         :egress [memory-scope provider-namespace])
       (function memory-query-contract
         :entry [mission_memory.query mission_kb_query context-pack-builder]
         :core ((step s1 :logic "apply memory-scope-resolution")
                (step s2 :logic "apply review overlay and default active-only retrieval")
                (step s3 :logic "call provider query with explicit capability and privacy class")
                (step s4 :logic "return lane-labeled evidence without injecting broad KB into prompts by default"))
         :egress [memory-query-result context-evidence-lane])
       (function memory-write-contract
         :entry [mission_memory.remember mission_kb_remember intent-memory-capture memory-review-batch-runner]
         :core ((step s1 :logic "require explicit scope and write reason")
                (step s2 :logic "route high-confidence stable intent to provider remember")
                (step s3 :logic "route uncertain or conflicting items to review overlay/candidate artifacts")
                (step s4 :logic "preserve source_refs and supersession metadata"))
         :egress [memory-record review-candidate])
       (function memory-review-overlay-contract
         :entry [mission_memory.review mission_kb_review memory-review-batch-runner]
         :core ((step s1 :logic "write non-destructive review overlay")
                (step s2 :logic "exclude superseded/historical/duplicate/stale/delete-candidate/needs-human from default retrieval")
                (step s3 :logic "keep original evidence available with include_archived=true or state_filter"))
         :egress [review-overlay-state])
       (function memory-context-injection-policy
         :entry [resident-master context-pack-builder worker-brief]
         :core ((step s1 :logic "default to no KB prefetch")
                (step s2 :logic "inject memory only when workflow declares memory scope and evidence purpose")
                (step s3 :logic "include provider/source/scope labels so agents can distinguish long-term memory from SSOT and runtime evidence"))
         :egress [context-pack-memory-lane]))
    :invariants
      ["MissionD Core MUST NOT require private memory data to boot, run Board/workstation workflows, or pass open-source checks."
       "Every memory query/write MUST resolve tenant/universe/project/user scope before calling a provider."
       "mission_kb_query and mission_kb_remember are compatibility leaves; preferred agents use mission_memory query/remember/review/provider_status."
       "Provider implementations own FTS, embedding, rerank, conversation archive, skill evidence index, active memory, archive state, export, and purge."
       "Default context-pack generation MUST NOT preload KB/history/provider logs; memory is opt-in by workflow and scope."]
    :checker "node scripts/check-v3-service-extraction-isomorphism.mjs")

  (memory-kb-policy
    :desc "Lisp-owned memory extraction budget for the memory-kb surface."
    :pending-message-limit 60
    :tool-result-preview-chars 1000
    :assistant-preview-chars 500
    :active-memory-target-ratio 0.10
    :sensitive-query-suppression [architecture:module]
    :review-states [active superseded-by-lisp superseded-by-code historical-evidence duplicate wrong-or-stale delete-candidate needs-human]
    :default-query-policy "exclude current review states superseded-by-lisp/superseded-by-code/historical-evidence/duplicate/wrong-or-stale/delete-candidate/needs-human unless include_archived or state_filter is explicit"
    :invariants
      ["mission_memory_pending MUST project batch size and preview truncation lengths from memory-kb-policy."
       "mission_memory_pending MUST cache the served realtime extraction batch for the active extraction cycle and allow bounded replay after context compaction; if replay cache is missing or exhausted it MUST return structured MEMORY_PENDING_ALREADY_SERVED rather than a successful empty result."
       "mission_memory_pending MUST classify deployment-monitor, runtime-report, worker-instruction, and provider-preamble text noise into input skip diagnostics before active memory extraction; deployment-monitor covers deploy/build/smoke/rollback/agent-update/provenance diagnostics plus deployment-event-response, xjp_build_wait, xjp_deploy_watch, and xjp_deploy_status monitor text; user utterances MUST never be filtered by these text classifiers."
       "mission_kb_query MUST suppress architecture:module details for sensitive credential/secret/SSH/token queries unless the caller explicitly scopes category/project to that architecture surface."
       "mission_kb_query MUST support excludeCategory / exclude_category for explicit category suppression, including subcategory matches such as memory excluding memory:*."
       "mission_kb_mutate(action=batch_remember) MUST accept a bounded entries array so memory review and distillation workflows do not need to spam one MCP call per KB row."
       "mission_kb_remember MUST pass through one shared dedupe gate in KbStore::kb_remember before any realtime/deep-analysis/manual pipeline can create a new active key; same source-session duplicates use a stricter low threshold and merge evidence_refs/source_sessions/superseded_by instead of overwriting them."
       "mission_kb_review MUST write a non-destructive knowledge_review_state overlay; it MUST NOT mutate or delete the original knowledge row."
       "Low-confidence semantic duplicate candidates MUST create a needs-human knowledge_review_state artifact and leave the raw row as evidence rather than deleting or silently activating it."
       "Large KB cleanup MUST calibrate with at least five manual batches before batch overlay application; target active memory is about 10%, with needs-human hidden from default retrieval."
       "mission_kb_query default retrieval MUST honor the review overlay while include_archived=true and state_filter preserve audit access to historical evidence."
       "A real MissionD project with .missiond but no memory-kb-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (learning-engine-policy
    :desc "Lisp-owned autonomous learning engine cadence, pty budget, and low-utility reflection policy."
    :realtime-extraction-timeout-secs 300
    :realtime-empty-backoff-base-secs 30
    :realtime-empty-backoff-max-secs 900
    :deep-analysis-zero-output-fuse-threshold 3
    :deep-analysis-zero-output-fuse-secs 3600
    :decision-tier3-timeout-secs 300
    :habit-scan-timeout-secs 600
    :token-spend-guard-window-secs 3600
    :token-spend-guard-soft-limit 250000
    :timeline-analysis-interval-secs 43200
    :timeline-analysis-window-hours 12
    :timeline-error-limit 20
    :timeline-llm-sample-limit 50
    :timeline-slow-event-limit 20
    :timeline-slow-threshold-ms 60000
    :idle-explore-interval-secs 7200
    :habit-scan-interval-secs 14400
    :habit-scan-batch-size 5
    :kb-auto-gc-interval-secs 3600
    :kb-consolidation-interval-secs 86400
    :kb-reflection-interval-secs 604800
    :kb-reflection-utility-threshold 0.3
    :kb-reflection-min-access 3
    :kb-reflection-max-entries 20
    :kb-reflection-max-tokens 2000
    :decision-harvest-interval-secs 86400
    :cooccurrence-refresh-interval-secs 21600
    :invariants
      ["LearningEngineRuntimeConfig MUST load learning-engine-policy from .missiond/v3/missiond-blueprint.lisp and fail with V3_BLUEPRINT_CONFIG_ERROR for real MissionD projects whose V3 blueprint or policy block is missing."
       "Realtime extraction, Tier3 decision escalation, and historical habit scan pty.send budgets MUST project from learning-engine-policy."
       "Realtime extraction MUST apply exponential empty-queue backoff from learning-engine-policy after consecutive no-user-work probes, and reset the backoff as soon as a real batch is dispatched."
       "Deep analysis MUST apply a Lisp-projected zero-output saturation fuse after consecutive completed deep-analysis jobs produce no KB mutations; while fused it MUST skip dispatch and expose diagnostics."
       "Memory/learning workers MUST consult token_usage_ledger through a Lisp-projected sliding-window token-spend soft guard before dispatch; if the window crosses token-spend-guard-soft-limit, MissionD MUST pause the memory domain through ControlTree and emit diagnostics instead of spending into a provider quota cliff."
       "Realtime extraction MUST claim the extraction lane before running pending-message DB probes; pending realtime SQL MUST use EXISTS/LATERAL LIMIT or bounded materialized-candidate shapes instead of global COUNT(DISTINCT)/ROW_NUMBER scans; deep-analysis active-conversation probes MUST use bounded EXISTS/OFFSET checks instead of full message COUNT scans so repeated ticks or status refreshes cannot exhaust the Postgres pool."
       "Memory extraction pending selectors MUST filter MissionD self-referential worker slots, including slot-memory*, slot-diagnosis*, and agent-* sessions, even when historical role attribution mistakenly labeled them as user conversations."
       "Learning maintenance cadences (timeline analysis, idle exploration, habit scan, KB auto-GC, KB consolidation, KB reflection, decision harvest, co-occurrence refresh) MUST project from learning-engine-policy."
       "Timeline analysis read windows, event limits, and slow-request threshold MUST project from learning-engine-policy."
       "KB reflection low-utility threshold, minimum access count, max entries, and max_tokens MUST project from learning-engine-policy."
       "Timeline projection SQL MUST cast string-bound since/until parameters as ::timestamptz when comparing against event_log.ts so PG never raises 'operator does not exist: timestamp with time zone >= text' from Timeline Analyst, mission_timeline, or stratified queries."
       "Timeline Analyst MUST check the Gemini provider gate before collecting timeline evidence or calling Gemini; when the gate is closed it MUST advance the cadence marker and skip without warning spam or repeated LLM attempts."])

  (conversation-ingestion-policy
    :desc "Lisp-owned read-model window and limit defaults for conversation, event, and timeline query surfaces."
    :conversation-get-tail-default 50
    :conversation-search-default-limit 10
    :message-search-default-limit 20
    :analysis-context-max-turns 50
    :label-calibration-sample-limit 200
    :jarvis-stream-envelope-schema "missiond.jarvis-stream-envelope.v1"
    :context-before-default 3
    :context-after-default 5
    :conversation-events-default-limit 100
    :agent-trajectory-default-limit 200
    :timeline-query-default-limit 50
    :timeline-query-max-limit 200
    :timeline-search-default-limit 20
    :timeline-search-max-limit 100
    :intent-router-model "claude-opus-4.6"
    :intent-router-timeout-ms 10000
    :vision-codex-binary "codex"
    :vision-codex-model "gpt-5.4"
    :vision-codex-idle-timeout-secs 120
    :vision-codex-absolute-timeout-secs 300
    :invariants
      ["mission_conversation_get/search/message_search/context_around MUST project default limits from conversation-ingestion-policy."
       "mission_conversation_events and mission_agent_trajectory MUST project default limits from conversation-ingestion-policy."
       "mission_timeline query/search MUST project default and max limits from conversation-ingestion-policy."
       "mission_timeline(action=wait) MUST expose bounded EventBus waits for board/slot/task/system predicates; timeout/lag returns diagnostic JSON so polling remains only an explicit fallback."
       "Explicit opt-in UserPromptSubmit context prefetch intent router model and timeout MUST project from conversation-ingestion-policy instead of local claude-opus/10000ms literals; default workstation hook sync removes UserPromptSubmit prefetch until a memory-audit workflow enables it."
       "Codex vision worker binary/model/idle timeout and CodexCli absolute timeout MUST project from conversation-ingestion-policy instead of local gpt-5.4/120s/300s literals."
       "Historical conversation event/tool-call backfills MUST NOT run unconditionally on daemon startup; they are opt-in maintenance/workflow operations gated by llm.yaml backfill_enabled or MISSIOND_CONVERSATION_BACKFILL_ON_STARTUP=1 so daemon restarts do not replay large provider histories as foreground CPU load."
       "llm_summary/topic embedding generation MUST default to human/Jarvis/direct CLI chat read models only; worker/meta/memory-slot conversations project their canonical result through task-result-artifact so skill-injection prompts, quota diagnostics, and worker instructions do not pollute user-facing conversation summaries."
       "Conversation analysis_context MUST be a bounded read model: it samples at most analysis-context-max-turns from calibrated turns and never pulls raw worker/provider chatter into user-intent inference."
       "Conversation label calibration MUST remain overlay-first: message_labels stores speaker/origin/canonical_state evidence, rawRole is preserved, and calibration reports are reviewed before any destructive rewrite."
       "Jarvis SSE and OpenAI-compatible chat surfaces MUST emit jarvis-stream-envelope-schema frames with conversation_id/task_id correlation, process affinity, and semantic event kind; PTY status is diagnostic and cannot replace the envelope."
       "Jarvis mobile/public clients MUST call /api/readiness after /health so daemon liveness, default slot busy state, and slot-unavailable startup failures are distinct operator-visible states; /health alone MUST NOT be presented as end-to-end readiness."
       "Jarvis mobile/public clients and operators MUST have /api/monitor/jarvis as the chain monitor for proxy reachability, daemon release, default slot readiness, MCP config, PTY live-screen/log evidence, and compiled runtime config; readiness is UX state, monitor is debug evidence."
       "Jarvis chat surfaces MUST write provider usage into token_usage_ledger with slot/task/message linkage so billing and quota views read one source of truth."
       "A real MissionD project with .missiond but no conversation-ingestion-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (evidence-governance-policy
    :desc "Unified evidence model for Memory/KB, Logs, Timeline, Conversation, and worker outputs."
    :authority-order [task_result_artifacts provider_durable_conversation event_log knowledge_review_state board_projection]
    :roles
      ((task-result-artifact :role canonical-worker-output :rule "Worker and workflow finals land here first; Board notes are projections.")
       (conversation :role provider-user-turn-read-model :rule "Conversation rows/messages preserve provider/user turns for audit and retrieval, not completion authority.")
       (timeline :role event-causality-view :rule "event_log / EventBus projections explain when and why something happened.")
       (kb-memory :role reviewed-long-term-knowledge :rule "KB is curated active memory after review overlay; raw historical logs are not active knowledge.")
       (board :role coordination-projection :rule "BoardTask state coordinates work and operator decisions; it is not the canonical worker result body."))
    :runtime-projection [mission_shared_memory.evidence_view task_result_artifacts conversations event_log knowledge_review_state board_tasks]
    :invariants
      ["mission_shared_memory(action=evidence_view) MUST return the unified evidence governance view for a task/project, grouping task_result_artifacts, conversations, event_log/shared_events, KB review overlay, and Board projection into named evidence lanes."
       "Memory/KB, Logs, Timeline, and Conversation MUST NOT each invent their own final-result authority; worker outputs use task-result-artifact, conversations are read models, timeline is causality, KB is reviewed long-term knowledge, and Board is coordination projection."
       "Default agent context may cite the evidence view lanes, but must not treat raw PTY, raw provider transcript, or unreviewed KB as higher authority than task-result-artifacts and durable events."])

  (cli-conversation-ingestion
    :desc "Canonical CLI conversation-log ingestion contract for ClaudeCode, Gemini CLI, and Codex CLI."
    :legacy-aliases ["claude_cli" "pty_jsonl"]
    (source claude-code
      :canonical "claude_code"
      :paths ["~/.claude/projects/**/*.jsonl" "~/.claude/history.jsonl"]
      :watcher "crates/missiond-core/src/cc_tasks/watcher.rs"
      :route "crates/missiond-daemon/src/infra/ingestion_router.rs"
      :history-import "scripts/import-claude-history-jsonl.mjs"
      :normalizer "scripts/normalize-claudecode-conversations.mjs"
      :audit "scripts/audit-claudecode-conversations.mjs")
    (source gemini-cli
      :canonical "gemini_cli"
      :paths ["~/.gemini/tmp/*/chats/*.json" "~/.gemini/tmp/*/chats/*.jsonl"]
      :watcher "crates/missiond-core/src/gemini_cli/watcher.rs"
      :route "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
      :audit "scripts/audit-gemini-conversations.mjs")
    (source codex-cli
      :canonical "codex_cli"
      :paths ["~/.codex/state_5.sqlite" "~/.codex/sessions/**/*.jsonl" "~/.codex/archived_sessions/*.jsonl" "~/.codex/session_index.jsonl" "~/.codex/history.jsonl"]
      :worker "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs")
    :invariants
      ["Conversation sources MUST be canonicalized before DB write: claude_code, gemini_cli, or codex_cli."
       "Legacy claude_cli and PTY transport pty_jsonl remain read aliases only; new non-transport source fields MUST name the canonical CLI."
       "mission_pty_status and mission_slots observability MUST be joinable with the latest conversation row by slot/session id and source."
       "mission_slots MUST reject or flag slot_sessions whose conversation source disagrees with the slot engine; stale provider drift must never masquerade as current state."
       "Codex CLI slot_sessions may contain a PTY placeholder id; mission_slots MUST fall back to the latest real codex_cli conversation for the slot project instead of surfacing a messageCount=0 placeholder as the latest durable conversation."
       "Codex CLI ingestion MUST scan the full state_5.sqlite thread set, including archived threads, and mark archived threads as historical status instead of dropping them from MissionD history."
       "Codex CLI ingestion MUST also discover raw rollout JSONL under ~/.codex/sessions/**/*.jsonl and ~/.codex/archived_sessions/*.jsonl even when state_5.sqlite has no thread row; session_meta.payload.id is the canonical conversation id, and raw-only imported rows MUST be recorded in conversation_source_state as sqlite-missing instead of being silently ignored."
       "Codex CLI conversation_source_state MUST distinguish current, sqlite-missing, missing-stale, path-mismatch, archived, and pty-placeholder evidence so audits can explain whether MissionD is missing provider history, whether Codex sqlite lost a rollout row, or whether a visible PTY is only a diagnostic placeholder."
       "Codex CLI runtime status MUST NOT treat archived=false in state_5.sqlite as proof that a conversation is actively running; provider source archive state, slot binding, durable final, and PTY state are separate evidence lanes."
       "Gemini request-log persistence MUST only consume Gemini provider LlmEvent variants; Codex CLI durable history belongs to codex_cli conversations/source-state, and non-Gemini LlmEvent replay MUST NOT pollute gemini_requests or generate duplicate insert warnings."
       "Codex CLI message ingestion MUST generate deterministic non-null message_uuid values from thread id, JSONL line number, role, and source event hash so reconcile/backfill cannot repeatedly insert duplicate NULL-uuid rows."
       "Codex CLI background ingestion MUST persist rollout size/mtime/line/complete watermarks and parse large rollout files in bounded pages after the last durable cursor; a 50k safety limit is per poll page, never a permanent history truncation."
       "When deterministic UUID ingestion meets an older NULL-uuid row with the same session, role, timestamp, and content, the DB layer MUST adopt that existing row by setting message_uuid instead of inserting a new duplicate row."
       "mission_conversation_get MUST defensively coalesce duplicate rows by message_uuid or role/timestamp/content fallback so frontend logs stay readable until historical cleanup is reviewed."
       "mission_conversation_get MUST retrieve tail messages with the indexed (session_id,id) path and assign display seq after duplicate coalescing; it MUST NOT use a ROW_NUMBER window over an entire large Codex/Gemini session."
       "Historical duplicate cleanup is dry-run/report-first; destructive DB cleanup must keep the earliest row in each duplicate group and require an explicit reviewed apply path."
       "Gemini background reconcile MUST use size/mtime companion watermarks to skip already-reconciled old chat files without reparsing full historical transcripts; manual reconcile may force a full scan."
       "Gemini manual/full reconcile MUST ignore count watermarks and replay raw ~/.gemini/tmp/*/chats/session-* files from message index 0 through deterministic message_uuid upserts, so historical sessions anchored before MissionD watcher startup can still be imported without duplicates."
       "Gemini manual/full reconcile MUST be reachable through mission_conversation_query(action=gemini_reconcile) / mission_conversation_gemini_reconcile, and that action MUST call gemini_reconcile_worker::run_gemini_reconciliation_now instead of relying on daemon restart or ad hoc SQL repair."
       "Gemini CLI tool lifecycle MUST close conversation_tool_calls with tool_result messages: parser emits tool_use/tool_result blocks, realtime ingestion and gemini_reconcile both persist has_tool_use/has_tool_result/content_types, and role=tool_result updates output_summary/raw_output/status rather than leaving tool calls pending."
       "Gemini CLI raw-vs-DB coverage MUST be auditable through scripts/audit-gemini-conversations.mjs: raw sessions missing in DB, DB conversations missing raw file, pending tool calls, and raw-vs-DB tool counts are reported before memory distillation trusts Gemini history."
       "Cursor/watermark advancement MUST happen after durable DB write acknowledgement, never before."
       "ClaudeCode ~/.claude/history.jsonl is a prompt-only historical source: import it as conversation_type=history_prompt, chat_type=history_jsonl, source=claude_code, speaker=human_user, authority=claude_history_prompt, and deterministic message_uuid=claude-history:<sha>; it MUST NOT be mistaken for assistant/tool transcript coverage."
       "ClaudeCode historical import MUST refresh conversations.message_count from actual inserted conversation_messages after import because database triggers/upserts can otherwise leave placeholder counts that make Logs and exports report double messages."
       "ClaudeCode conversation normalization MUST maintain a non-destructive overlay: conversation_source_state records current/missing-stale/path-mismatch/raw-only-local-command/raw-only-provider-prompt/raw-only-uningested source evidence, message_labels canonical_state marks exact role/timestamp/content duplicates as equivalent-duplicate, raw_role_state distinguishes native/reconstructed/provider-derived/ambiguous, and no provider JSONL row is physically deleted by normalization."
       "True-user utterance export MUST include ClaudeCode history_prompt rows and exclude equivalent-duplicate, worker/subagent/compaction, task-bound sessions, MissionD runtime prompts, provider context, terminal artifacts, and local-command artifacts; verification must fail if BoardTask/Swarm prompt signatures leak into the export."
       "ClaudeCode provider role normalization MUST be shared by realtime watcher, per-session reconcile, and daily reconcile paths: top-level raw_role=user inside automated slot sessions normalizes to worker_user, interactive Jarvis/user conversations remain user, sidechain progress remains agent_user/agent_assistant, and raw_role is preserved for audit."
       "Historical ClaudeCode role repair is dry-run/report-first through scripts/report-claude-role-attribution.mjs; first pass reports suspected system/user/agent_user drift and never mutates DB."
       "Provider-aware conversation_type classification MUST live behind crates/missiond-core/src/db/conversation_query.rs::classify_conversation_type so ClaudeCode, Codex CLI, and Gemini CLI workers share one rule set: slot-bound sessions (any provider) classify as worker with durable slotId/taskId linkage; background-ingested Codex threads classify as codex_chat (parallel to gemini_chat), never as the human user fallthrough; real human Jarvis user sessions remain user."
       "ClaudeCode slot session capture and reconcile MUST bind conversations.task_id from the currently running BoardTask claimed by that slot after session UUID discovery and after lazy JSONL conversation creation, so mission_conversation_query(taskId=...) works while workers are still running rather than only after final evidence."
       "Codex CLI background ingestion MUST call classify_conversation_type AND preserve the provider role into raw_role so the conversation row carries enough metadata for audit_classification and the role-attribution report; the legacy hardcoded conversation_type=\"user\" + raw_role=None pattern is forbidden."
       "Codex CLI background ingestion MUST refresh conversation message_count from actual inserted rows after each import so the conversation list, Logs, and memory distillation samples do not regress to the initial upsert placeholder count."
       "Historical row classification repair is dry-run/report-first through db::conversation_query::audit_historical_classification: it returns HistoricalClassificationFinding values for codex_user_without_slot, codex_slot_not_worker, worker_loses_slot_linkage, codex_raw_role_missing, and claude_worker_prompt_signature; mission_conversation_query(action=audit_classification) reports candidates without mutation, mission_conversation_query(action=backfill_classification, apply=true) may apply only high-confidence repairs through set_conversation_type, backfill_missing_raw_roles_for_session for old Codex rows, and then rebuild conversation_turns via rebuild_session_turns."
       "Historical ClaudeCode message-role repair is also dry-run/report-first: mission_conversation_query(action=audit_message_roles) reports worker-session rows where source=claude_code, conversation_type=worker, role=user, raw_role=user, and content matches local-command or worker-prompt signatures; mission_conversation_query(action=backfill_message_roles, apply=true) may rewrite only those reviewed rows to worker_user and then rebuild conversation_turns via rebuild_session_turns. It MUST NOT delete provider messages or bulk-relabel real human Jarvis/user conversations."
       "Conversation turn repair is explicit and bounded: mission_conversation_query(action=turn_backfill, sessionId=...) clears only that session's conversation_turns and re-runs tagger_chunker on its canonical message stream; it does not rewrite raw provider logs."]
    :checker "node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs")

  (upstream-pty-signatures
    :desc "Provider-aware PTY recognition signatures derived from upstream TUI source instead of screenshot-only heuristics."
    (provider codex-cli
      :canonical "codex_cli"
      :upstream "https://github.com/openai/codex"
      :ref "ff27d01676a93be7467b3893e82f41a7af7e1418"
      :source-paths ["codex-rs/tui/src/status_indicator_widget.rs"
                     "codex-rs/tui/src/chatwidget.rs"
                     "codex-rs/tui/src/bottom_pane/approval_overlay.rs"
                     "codex-rs/tui/src/bottom_pane/chat_composer.rs"]
      :signals [working-status esc-to-interrupt status-details approval-overlay composer-idle])
    (provider gemini-cli
      :canonical "gemini_cli"
      :upstream "https://github.com/google-gemini/gemini-cli"
      :ref "d9f273e44095b742e9ab74241e240c587ae27e64"
      :source-paths ["packages/cli/src/ui/types.ts"
                     "packages/cli/src/ui/components/LoadingIndicator.tsx"
                     "packages/cli/src/ui/components/InputPrompt.tsx"
                     "packages/cli/src/ui/components/messages/DenseToolMessage.tsx"]
      :signals [StreamingState.Idle StreamingState.Responding StreamingState.WaitingForConfirmation Thinking esc-to-cancel CoreToolCallStatus])
    (provider claude-code
      :canonical "claude_code"
      :upstream "/Users/jinchen/Downloads/claudecode/claudecode"
      :source-paths ["src/constants/spinnerVerbs.ts"
                     "src/constants/turnCompletionVerbs.ts"
                     "src/remote/sdkMessageAdapter.ts"
                     "src/cli/print.ts"]
      :signals [spinner-verbs turn-completion-verbs tool-progress auto-mode prompt-footer])
    :output PtyRecognitionSnapshot
    :states [running idle blocked complete unknown]
    :invariants
      ["Codex CLI and Gemini CLI MUST use provider-specific StateParser implementations and MUST NOT fall back to the ClaudeCode parser."
       "mission_pty_status MUST include PtyRecognitionSnapshot with provider, state, confidence, reason, phase/tool/blocked details when available."
       "Autopilot watchdogs MUST treat low-confidence unknown as diagnostic state rather than automatic BoardTask closure evidence."
       "If an upstream TUI signal changes, checker failure is preferred over silent downgrade to generic prompt heuristics."
       "recognize_screen MUST fuse SessionState with screen heuristics: an active processing SessionState (Thinking, Responding, ToolRunning, Confirming) MUST NOT be demoted to Blocked from screen_fallback confirmation or model-picker text; the fused snapshot is sourced from screen_fused active evidence or session_state, and explicit Confirming SessionState always preserves Blocked."
       "Confirming is an active turn state, not a TextOutput::Complete boundary: Codex/ClaudeCode/Gemini approval menus may block the provider turn, but completion can only fire after confirmation resolves and the real final answer returns to Idle/Exited."
       "Exited/terminal SessionState overrides stale running screen evidence; mission_pty_status and mission_slots MUST NOT expose recognition.state=running when the durable PTY session state is exited or error."
       "Codex MCP approval menus (`Allow the ... MCP server to run tool`, `Allow for this session`, `enter to submit | esc to cancel`) are explicit blocked TUI source signatures and MUST NOT be demoted to Running just because the SessionState is Thinking."
       "mission_pty_confirm MUST confirm option menus by human-like keyboard navigation (Down/Up then Enter), never by sending numeric shortcut keys; this applies to ClaudeCode, Codex CLI, and Gemini CLI."
       "recognize_claude_code Blocked MUST require explicit confirmation/model-picker UI (Enter to confirm, Do you want to proceed/make this edit/allow/use this api key, Select model, approval request); the bare words `approval` or `permission(s)` -- including the `bypass permissions on` composer-mode footer toggle and historical task-brief prose -- MUST NOT trigger Blocked on Idle or completed screens."
       "ClaudeCode worker MCP reconnect MUST follow `/mcp` -> Enter -> ArrowDown until missiond -> Enter -> Enter using arrow-key keystrokes only; numeric shortcut selection is forbidden because Claude Code's MCP picker numeric shortcuts have shifted between releases. The keystroke sequence is the SSOT and missiond-pty Session::mcp_reconnect_sequence MUST project from it."
       "When a ClaudeCode worker advertises supports_mcp=true but its mounted tool list does not include any mission_* tool after slot ready, master_control MUST file a durable claude_code_mcp_missing incident so the resident master is woken; if the /mcp arrow-key reconnect ritual does not surface mission_* tools within the policy budget, a follow-up claude_code_mcp_reconnect_failed incident is required, never a silent retry loop."]
    :checker "node scripts/check-v3-pty-recognition-isomorphism.mjs")
