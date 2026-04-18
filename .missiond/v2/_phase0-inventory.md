# Phase 0 · Event Bus Refactor — Inventory

> Read-only survey of the current (v1) event-bus surface. Produced 2026-04-19 on branch `refactor/event-bus-v2`.
> All file paths absolute. Consumers reading this should cross-reference `intent-event-bus.lisp` frozen design before
> touching code in Phase 1-9.

Scope: all files under `/Users/jinchen/Projects/missiond/crates/`. Worktree copies under `.claude/worktrees/*` intentionally ignored.

---

## 1. Event enum — `DaemonEvent` full variant map

Source: `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/event_bus.rs:17-430`.
`wire_type()` map at `event_bus.rs:446-503`; `is_ephemeral()` at `event_bus.rs:509-522`; `to_frontend_payload()` at `event_bus.rs:525-954`.

52 variants total (6 are LEGACY-deprecated engine-specific; all 52 still match-armed in `wire_type` / `to_frontend_payload`).

| # | Variant | Fields (abbrev.) | event_bus.rs line | is_ephemeral? | → v2 domain |
|---|---|---|---|---|---|
| 1 | `SlotBecameIdle` | slot_id | 20 | no | SlotEvent::BecameIdle |
| 2 | `TaskCreated` | task_id | 25 | no | TaskEvent::Created |
| 3 | `TaskCompleted` | task_id | 27 | no | TaskEvent::Completed |
| 4 | `QuestionCreated` | question_id | 30 | no | QuestionEvent::Created |
| 5 | `GeminiRequestStarted` *LEGACY* | request_id, caller, session_id, model, prompt_chars, prompt_text | 34 | no | LlmEvent::RequestStarted (Provider=Gemini) — deprecated, may drop |
| 6 | `GeminiRequestCompleted` *LEGACY* | request_id+api_mode+retry_count+duration_ms etc. | 44 | no | LlmEvent::RequestCompleted — deprecated |
| 7 | `GeminiToolActivity` *LEGACY* | request_id, tool_seq, activity, tool_name… | 62 | no | LlmEvent::ToolActivity — deprecated |
| 8 | `DecisionResolved` | question_id, tier, duration_ms | 80 | no | QuestionEvent::DecisionResolved |
| 9 | `QuestionResolved` | question_id, resolution | 86 | no | QuestionEvent::Resolved |
| 10 | `MemoryPhaseChanged` | slot_id, phase, active_type | 91 | no | MemoryEvent::PhaseChanged |
| 11 | `BoardTaskCreated` | task_id, title, category | 98 | no | BoardEvent::TaskCreated |
| 12 | `BoardTaskStatusChanged` | task_id, old_status, new_status | 104 | no | BoardEvent::StatusChanged |
| 13 | `BoardTaskNoteAdded` | task_id, note_id, content_preview | 110 | no | BoardEvent::NoteAdded |
| 14 | `BoardTaskClaimed` | task_id, slot_id | 116 | no | BoardEvent::Claimed |
| 15 | `BoardTaskDeleted` | task_id, title | 118 | no | BoardEvent::Deleted |
| 16 | `BoardTaskUpdated` | task_id, status, category | 120 | no | BoardEvent::Updated |
| 17 | `SlotStateChanged` | slot_id, new_state, prev_state | 126 | no | SlotEvent::StateChanged |
| 18 | `InsightGenerated` | category, priority, title | 134 | no | SystemEvent::InsightGenerated |
| 19 | `SlotTaskDispatched` | slot_id, task_id, purpose, prompt_chars, preview, cited_kb_ids | 142 | no | SlotEvent::TaskDispatched |
| 20 | `ConversationMessageLogged` | message_id, session_id, parent_session_id, slot_id, role, content_chars, preview | 158 | no | MessageEvent::Logged |
| 21 | `CodexRequestStarted` *LEGACY* | request_id, caller, model, prompt_chars, has_image, image_hash, prompt_text | 172 | no | LlmEvent::RequestStarted (Provider=Codex) — deprecated |
| 22 | `CodexRequestCompleted` *LEGACY* | …+input_tokens, output_tokens | 182 | no | LlmEvent::RequestCompleted — deprecated |
| 23 | `ImageMessageInserted` | message_id, session_id | 199 | **yes** | MessageEvent::ImageInserted |
| 24 | `BriefingBatchStarted` | pending_count | 203 | **yes** | WorkerEvent::BriefingBatchStarted |
| 25 | `BriefingSummaryGenerated` | target_seq, summary, method | 206 | **yes** | WorkerEvent::BriefingSummaryGenerated |
| 26 | `TranslationStarted` | message_id, slot_id, content_chars | 217 | **yes** | WorkerEvent::TranslationStarted |
| 27 | `TranslationCompleted` | message_id, slot_id, preview, duration_ms | 224 | no | WorkerEvent::TranslationCompleted |
| 28 | `TranslationFailed` | message_id, slot_id, error | 232 | no | WorkerEvent::TranslationFailed |
| 29 | `NarrationSessionStarted` | session_id, total_messages, already_narrated | 240 | no | WorkerEvent::NarrationSessionStarted |
| 30 | `NarrationBatchCompleted` | session_id, batch_index, processed_count, total_messages, duration_ms | 246 | **yes** | WorkerEvent::NarrationBatchCompleted |
| 31 | `NarrationSessionCompleted` | session_id, total_narrated | 254 | no | WorkerEvent::NarrationSessionCompleted |
| 32 | `NarrationFailed` | session_id, batch_index, error, will_retry | 259 | no | WorkerEvent::NarrationFailed |
| 33 | `WorkerLlmCall` | caller, task_id, status, prompt_chars, response_chars, duration_ms, queue_wait_ms | 268 | **yes** | WorkerEvent::LlmCall |
| 34 | `ToolCompleted` | session_id, slot_id, tool_name, status, is_error, input_summary, output_summary | 281 | no | SystemEvent::ToolCompleted |
| 35 | `ConfigFileChanged` | path, kind | 293 | no | SystemEvent::ConfigChanged |
| 36 | `CliRequestStarted` | engine, request_id, caller, session_id, model, prompt_chars, prompt_text, extra | 302 | no | LlmEvent::RequestStarted |
| 37 | `CliRequestCompleted` | engine, …, duration_ms, status, error_msg, response_text, extra | 315 | no | LlmEvent::RequestCompleted |
| 38 | `JarvisTaskCompleted` | conversation_id, task_id | 334 | no | SessionEvent::JarvisTaskCompleted |
| 39 | `CliToolActivity` | engine, request_id, tool_seq, activity, tool_name, input_preview, result_preview, is_error | 341 | no | LlmEvent::ToolActivity |
| 40 | `SessionCompleted` | session_id, slot_id, message_count, duration_secs, status (`SessionEndStatus`) | 355 | no | SessionEvent::Completed |
| 41 | `DeepAnalysisCompleted` | session_id, kb_entries_created | 364 | no | SessionEvent-adjacent — likely `SessionEvent::DeepAnalysisCompleted` (new variant) |
| 42 | `KBBatchMutated` | count, categories, action | 370 | no | MemoryEvent::KBBatchMutated (new variant, domain=Memory) |
| 43 | `SessionOrganized` | session_id | 380 | **yes** | SessionEvent::Organized (new variant) |
| 44 | `TurnExtracted` | session_id, turn_count | 383 | **yes** | SessionEvent::TurnExtracted (new variant) |
| 45 | `IntentAnalyzed` | session_id, intent_type | 389 | **yes** | SessionEvent::IntentAnalyzed (new variant) |
| 46 | `JarvisProactivePush` | conversation_id, trigger_reason, summary | 397 | no | MessageEvent::JarvisProactivePush (new variant) |
| 47 | `ContextualCommitDetected` | commit_hash, branch, summary, conversation_id, message_id, session_id, slot_id | 406 | no | SystemEvent::ContextualCommitDetected (new variant) |
| 48 | `CascadeTriggered` | service, changed | 418 | no | SystemEvent::CascadeTriggered (new variant) |
| 49 | `CascadeCompleted` | service, services_repaired, services_failed, hard_halted, duration_ms | 423 | no | SystemEvent::CascadeCompleted (new variant) |

Supporting enum: `SessionEndStatus { Success | Aborted | Error }` at `event_bus.rs:435-442` — remains, used by `SessionEvent::Completed`.

Notes on mapping:
- 12 domains in frozen lisp: SlotEvent / BoardEvent / TaskEvent / QuestionEvent / LlmEvent / WorkerEvent / MemoryEvent / MessageEvent / SessionEvent / SystemEvent / ObservabilityEvent / IncidentEvent.
- No current `DaemonEvent` variant maps directly to `ObservabilityEvent` or `IncidentEvent`. Observability currently emitted via the synthetic `health_snapshot` WS payload (main.rs:1310-1357, not through bus). Incidents currently bypass the bus via `incident_tx` MPSC.
- `ContextualCommitDetected` is technically Observability-like but its consumers (arch_maintenance, lisp_survey workers) make it a SystemEvent trigger.
- Cascade events (48,49) could be either SystemEvent or a new CascadeEvent; keeping in SystemEvent keeps domain count at 12 unless a hotspot emerges.
- `KBBatchMutated` domain is ambiguous — MemoryEvent is the cleanest fit given KB = memory store.

→ Phase 1 work: emit trait+12 domain enums; Phase 2 work: provide per-variant `From<DaemonEvent>` shim during transition; deprecated Gemini/Codex* variants should map to `LlmEvent` to avoid a permanent compat shim.

---

## 2. All publish call sites

Counted: 42 `.publish(…)` + 41 `.publish_traced(…)` = **83 total publish points**.

Grouped by producer context. File:line → variant emitted.

### 2a. LLM / gateway layer

- `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/llm/gemini_client.rs:366` — `CliToolActivity` (forwarder from GeminiCliProgress)
- `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/llm/gemini_client.rs:518` — `CliRequestStarted`
- `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/llm/gemini_client.rs:597` — `CliRequestCompleted`
- `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/llm/codex_cli.rs:353` — `CliRequestStarted`
- `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/llm/codex_cli.rs:422` — `CliRequestCompleted`
- `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/llm/sonnet_gateway.rs:416` — `WorkerLlmCall`
- `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/llm/minimax_gateway.rs:387` — `WorkerLlmCall`

### 2b. Workers (sonnet/local/gemini/codex)

- `briefing_worker.rs:72` — `BriefingSummaryGenerated`
- `briefing_worker.rs:94` — `BriefingSummaryGenerated`
- `briefing_worker.rs:185` — `BriefingSummaryGenerated`
- `briefing_worker.rs:297` — `BriefingBatchStarted`
- `translation_worker.rs:83` — `TranslationStarted` (traced)
- `translation_worker.rs:137` — `TranslationCompleted` (traced)
- `translation_worker.rs:157` / `:172` — `TranslationFailed` (traced)
- `gemini/strategy_worker.rs:116` — `DeepAnalysisCompleted`
- `gemini/strategy_worker.rs:725` — `InsightGenerated`
- `codex/step_narrator.rs:165` — `NarrationSessionStarted` (traced)
- `codex/step_narrator.rs:250,272,294,315` — `NarrationBatchCompleted`, `NarrationFailed`, `NarrationSessionCompleted` (traced variants)
- `local/pty_event_worker.rs:144` — `SessionCompleted` (traced)
- `local/pty_event_worker.rs:175` — `SlotStateChanged` (traced)
- `local/pty_event_worker.rs:201` — `SlotBecameIdle`
- `local/pty_event_worker.rs:291` — `MemoryPhaseChanged` (traced)
- `local/pty_event_worker.rs:310` — `SlotBecameIdle`
- `local/pty_event_worker.rs:426` — `TaskCompleted` (traced)
- `local/pty_event_worker.rs:440` — `TaskCompleted`
- `local/tagger_chunker.rs:193,217` — `TurnExtracted`
- `local/tagger_chunker.rs:318` — `ContextualCommitDetected`
- `local/conversation_organizer.rs:117` — `SessionOrganized`
- `local/gemini_logger.rs:*` — ONLY **consumes** bus events to persist DB rows; grep false positive (references `DaemonEvent::…` in match arms, not publishes).

### 2c. Handlers (MCP tool surface)

- `handlers/knowledge/board.rs:79` — `BoardTaskCreated` (traced)
- `handlers/knowledge/board.rs:95` — `BoardTaskUpdated` (traced)
- `handlers/knowledge/board.rs:115` — `BoardTaskStatusChanged` (traced)
- `handlers/knowledge/board.rs:431` — `BoardTaskDeleted` (traced)
- `handlers/knowledge/board.rs:472` — `BoardTaskClaimed` (traced)
- `handlers/knowledge/board.rs:531` — `BoardTaskNoteAdded` (traced)
- `handlers/knowledge/board.rs:708` — `SlotTaskDispatched`
- `handlers/knowledge/kb.rs:347,423,464,536` — `KBBatchMutated` (4 sites for different mutation actions)
- `handlers/knowledge/kb.rs:1786` — `TaskCreated`
- `handlers/knowledge/cascade.rs:266` — `CascadeTriggered`
- `handlers/knowledge/cascade.rs:280` — `CascadeCompleted`
- `handlers/comm/question.rs:164` — `QuestionCreated` (traced)
- `handlers/comm/question.rs:220` — `TaskCompleted`
- `handlers/comm/question.rs:223` — `QuestionResolved` (traced)
- `handlers/comm/question.rs:248` — `QuestionResolved` (traced)
- `handlers/compute/task.rs:176` — `SlotTaskDispatched`
- `handlers/compute/task.rs:292` — `SlotTaskDispatched`
- `handlers/compute/task.rs:315` — `TaskCreated` (traced)
- `handlers/sysinfra/misc.rs:270` — `QuestionCreated`
- `handlers/sysinfra/misc.rs:304` — `QuestionCreated`

### 2d. Engines (learning / intent / memory-scheduler)

- `engine/intent_engine/autopilot.rs:41` — `JarvisTaskCompleted` (traced)
- `engine/intent_engine/autopilot.rs:376` — `BoardTaskStatusChanged` (traced)
- `engine/intent_engine/autopilot.rs:660` — `SlotTaskDispatched`
- `engine/intent_engine/autopilot.rs:730,868,1087` — `BoardTaskStatusChanged` (traced)
- `engine/intent_engine/autopilot.rs:983` — `JarvisTaskCompleted` (traced)
- `engine/intent_engine/autopilot.rs:1631` — `JarvisProactivePush`
- `engine/intent_engine/flow_engine.rs:320` — `SlotTaskDispatched`
- `engine/intent_engine/memory_scheduler.rs:196` — `SlotTaskDispatched`
- `engine/intent_engine/memory_scheduler.rs:292` — `TaskCreated`
- `engine/learning_engine/extraction.rs:23` — `MemoryPhaseChanged` (traced)
- `engine/learning_engine/extraction.rs:48` — `SlotTaskDispatched`
- `engine/learning_engine/decision_engine.rs:97,190,214,907` — `QuestionResolved` (traced)
- `engine/learning_engine/decision_engine.rs:508,596,703` — `TaskCreated`
- `engine/learning_engine/decision_engine.rs:512,599,706,837,906` — `DecisionResolved` (traced, with 906 also `QuestionResolved`)
- `engine/learning_engine/idle_explorer.rs:475` — `BoardTaskStatusChanged`
- `engine/learning_engine/intent_analyst.rs:247` — `IntentAnalyzed`
- `engine/learning_engine/historical_scanner.rs:120` — `SlotTaskDispatched`
- `engine/learning_engine/historical_scanner.rs:152` — `MemoryPhaseChanged`
- `engine/learning_engine/timeline_analyst.rs:314` — `InsightGenerated` (traced)

### 2e. Infrastructure

- `infra/message_handler.rs:542` — `ConversationMessageLogged` (traced)
- `infra/message_handler.rs:635` — `ToolCompleted` (traced)
- `infra/aiops.rs:316,409` — `TaskCreated` (AIOps creates triage tasks)
- `main.rs:1722` — `ConfigFileChanged` (fsnotify debounced)

→ Phase 1-2 work: every site above must migrate to `log.append()`. The split between `publish` and `publish_traced` collapses into one API — trace context becomes an `AppendOpts` / `SpanContext` field. Expect ~15 mechanical rewrites per variant family, grouped by domain to parallelize Phase 2.

---

## 3. Timeline Writer — `run_timeline_writer`

Source: `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/main.rs:91-200`. Spawned at `main.rs:1262-1270`.

Signature:
```rust
async fn run_timeline_writer(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<event_bus::TimelineEntry>,
    store: Arc<dyn missiond_core::db::traits::MissionStore>,
    timeline_tx: broadcast::Sender<event_bus::TimelineEvent>,
    ws_tx: broadcast::Sender<String>,
)
```

### 6-step loop

1. **recv** — `rx.recv().await` blocks for first `TimelineEntry` (unbounded MPSC ensures no drops); on `None` (channel closed) → break + `tracing::warn!` shutdown.
2. **micro-batch** — drain up to 99 additional entries via `rx.try_recv()` (total batch ≤100).
3. **partition** — split batch into `(persistent, ephemeral)` by `entry.event.is_ephemeral()` (see section 1 table; 8 variants are ephemeral).
4. **ephemeral fan-out** — for each ephemeral entry, build `TimelineEvent { seq: 0, … }`, serialize `to_frontend_json()`, send via `ws_tx` (WS broadcast) + `timeline_tx` (internal broadcast). No DB write.
5. **persistent DB insert** — build `(trace_id, span_id, parent_span_id, event_type, summary, payload_json)` tuples; call `store.insert_timeline_batch(&params).await` → `Vec<i64>` of assigned seqs. Failure → `tracing::error!` + `continue` (batch dropped, consumer does NOT get events).
6. **fan-out persistent** — `.zip(seqs)` to build `TimelineEvent { seq, … }`; same dual send pattern as step 4.

### I/O channels

| Input MPSC | `tokio::sync::mpsc::UnboundedReceiver<TimelineEntry>` | main.rs:483 |
| Output broadcast A | `broadcast::Sender<TimelineEvent>` capacity 512 | main.rs:485 |
| Output broadcast B | `broadcast::Sender<String>` capacity 256 (WS JSON) | main.rs:488 |
| DB writer | `insert_timeline_batch(entries: &[(Option<&str>, &str, Option<&str>, &str, Option<&str>, &str)]) -> DbResult<Vec<i64>>` at `crates/missiond-core/src/db/traits.rs:459`; pg impl at `/Users/jinchen/Projects/missiond/crates/missiond-core/src/db/pg/timeline.rs:57` |

### Invariants

- `seq=0` is the sentinel for ephemeral events (checked nowhere explicitly — frontend just treats it as volatile).
- Persistent events get DB-assigned BIGSERIAL seq.
- DB failure ⇒ batch silently skipped (no retry; anti-pattern vs v2 design `Backpressure`/`LogUnavailable`).
- Batch ordering across persistent/ephemeral split is NOT preserved in the broadcast stream: ephemeral fanned out BEFORE persistent insert, so seq-based ordering guarantees break here.

→ Phase 3-4 work: `run_timeline_writer` is entirely replaced by `LogWriter` task draining an `append channel` and executing `INSERT RETURNING seq`. Dispatcher becomes a separate task per the v2 spec (log tail → domain match → topic fanout). The ephemeral/persistent split remains but moves to `AppendOpts.ephemeral` rather than a per-variant `is_ephemeral()` method.

---

## 4. Timeline subscribers — `broadcast::Receiver<TimelineEvent>`

All receivers get a clone of `timeline_broadcast_tx` via `.subscribe()`.

### 4a. `event_router.rs` consumers — 8 subscribe calls

- `event_router.rs:82` — `spawn_extraction_consumer` (sub #1)
- `event_router.rs:167` — `spawn_submit_consumer` (sub #2)
- `event_router.rs:244` — `spawn_decision_consumer` (sub #3)
- `event_router.rs:311` — `spawn_harvest_consumer` (sub #4)
- `event_router.rs:348` — `spawn_realtime_extraction_consumer` (sub #5)
- `event_router.rs:428` — `spawn_session_reflection_consumer` (sub #6)
- `event_router.rs:519` — `spawn_kb_consolidation_consumer` (sub #7)
- Plus `engine/learning_engine/intent_analyst.rs:44` — `spawn_intent_consumer` (sub #8, spawned FROM event_router.rs:67-71)

### 4b. Worker subscriptions — 6 receivers constructed in main.rs

- `main.rs:1126` — `GeminiLoggerWorker.timeline_rx` (file `workers/local/gemini_logger.rs:15`). Filters `CliRequestStarted`/`CliRequestCompleted`/legacy `GeminiRequest*`/`CodexRequest*`. Writes to `gemini_requests` DB table.
- `main.rs:1150` — `TranslationWorker.timeline_rx` (file `workers/sonnet/translation_worker.rs:201`). Filters `ConversationMessageLogged { role=="thinking" }` via `extract_thinking_ctx`. Lagged-tolerant; has `CIRCUIT_BREAKER` + `poll_pending` fallback.
- `main.rs:1443` — `ArchMaintenanceWorker.timeline_rx` (file `workers/sonnet/arch_maintenance_worker.rs:33`). Filters `ContextualCommitDetected`, 30 s per-branch debounce + self-filter (`ARCH_COMMIT_PREFIX`).
- `main.rs:1452` — `LispSurveyWorker.timeline_rx` (file `workers/sonnet/lisp_survey_worker.rs:31`). Filters `ContextualCommitDetected`.
- `main.rs:1499` — `ConversationOrganizerWorker.timeline_rx` (file `workers/local/conversation_organizer.rs:24`). Filters `ConversationMessageLogged` → triggers session organize.
- `main.rs:1509` — `TaggerChunkerWorker.timeline_rx` (file `workers/local/tagger_chunker.rs:74`). Filters `SessionOrganized` → extract turns.

### 4c. Summary table (matches × actions)

| Consumer | Filters | Debounce | Action | Lagged-handling |
|---|---|---|---|---|
| extraction_consumer | `SlotBecameIdle { slot_id==MEMORY_SLOT_ID \|\| MEMORY_SLOW_SLOT_ID }` | 500 ms trailing-edge | `schedule_memory_tasks` | exp backoff 100→2000 ms (`lagged_backoff`), stats inc |
| submit_consumer | `TaskCreated`/`TaskCompleted` | 100 ms | `dispatch_queued_submit_tasks` + `schedule_memory_tasks` | backoff + defensive dispatch |
| decision_consumer | `QuestionCreated` | 100 ms | `process_pending_master_questions` | backoff + defensive |
| harvest_consumer | `NarrationSessionCompleted` | none | `experience_harvester::harvest_session` | debug log only |
| realtime_extraction_consumer | `ConversationMessageLogged { slot_id.is_some() }` | 3 s | `check_realtime_extraction` | stats inc |
| session_reflection_consumer | `SessionCompleted { status=Success }` | 5 s | `check_deep_analysis` + notify strategy/retro | stats inc |
| kb_consolidation_consumer | `DeepAnalysisCompleted` | counter-based (thresh=5) | `check_kb_consolidation` | defensive trigger on lag |
| intent_consumer | `TurnExtracted` | per-session debounce 60 s OR accum 10 turns | `process_session_intents` | warn-log |
| GeminiLoggerWorker | `CliRequest*` + legacy | none | `gemini_log_insert_started` / `gemini_log_update_completed` | warn-log only |
| TranslationWorker | `ConversationMessageLogged { role==thinking }` | idle poll 30 s | `process_single` translation | circuit breaker + poll_pending fallback |
| ArchMaintenanceWorker | `ContextualCommitDetected` | 30 s per-branch | `process_commit` YAML update | `ctx.wait_if_paused()` blocking |
| LispSurveyWorker | `ContextualCommitDetected` | (debounce logic inside) | intent.lisp incremental survey | pause-aware |
| ConversationOrganizerWorker | `ConversationMessageLogged` | (internal) | organize + splice compaction | TODO verify |
| TaggerChunkerWorker | `SessionOrganized` | (internal) | extract turns → push to EmbeddingTask | TODO verify |

→ Phase 3-4 work: each consumer becomes `bus.subscribe::<DomainEvent>(name, SubscriptionOpts{…})` with per-sub cursor persisted in `event_subscriptions` table. Debounce/coalesce becomes `sub.debounce(…)` combinator per v2 spec §4.3. Lagged handling is absorbed into `PauseBehavior::DropAndLiveResume` default + `FailurePolicy::Retry`.

---

## 5. `event_router.rs` — 8 consumer full breakdown

File: `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/event_router.rs` (658 lines, from line 53 `start_event_consumers`).

### Shared helpers

- `lagged_backoff(consecutive_lags: u32) -> Duration` at L31 — 100 ms × 2^n capped at 2 s with ±25 % jitter. Tested at L611-657.
- `spawn_sweeper(state)` at L569 — 30 min periodic + startup scan via `reconciliation_sweep` (L585-605).
- `reconciliation_sweep` invokes `check_realtime_extraction`, notifies strategy/retro via `Notify`, runs `check_deep_analysis`, `check_kb_consolidation`, `check_kb_reflection`.

### Consumers

| Function | L | Watches | Debounce | Action | ControlTree check |
|---|---|---|---|---|---|
| `spawn_extraction_consumer` | 76 | `SlotBecameIdle { slot_id∈{MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID} }` | 500 ms fixed deadline (trailing) | `schedule_memory_tasks` | `is_domain_paused(Memory)` at L114, L134, L147 |
| `spawn_submit_consumer` | 161 | `TaskCreated` \| `TaskCompleted` | 100 ms | `dispatch_queued_submit_tasks` + conditional `schedule_memory_tasks` | `is_domain_paused(Memory)` at L196 |
| `spawn_decision_consumer` | 238 | `QuestionCreated` | 100 ms | `process_pending_master_questions` | none |
| `spawn_harvest_consumer` | 305 | `NarrationSessionCompleted` | — | `experience_harvester::harvest_session` | none |
| `spawn_realtime_extraction_consumer` | 342 | `ConversationMessageLogged { slot_id.is_some() }` | 3 s, per-session HashSet | `tokio::spawn(check_realtime_extraction)` | `is_domain_paused(Memory)` gates enqueue |
| `spawn_session_reflection_consumer` | 422 | `SessionCompleted { status=Success }` | 5 s, per-session HashSet | `strategy_notify.notify_one()` + `retro_notify.notify_one()` + `check_deep_analysis` | `is_domain_paused(Memory) \|\| global_paused` skips |
| `spawn_kb_consolidation_consumer` | 513 | `DeepAnalysisCompleted` | counter ≥5 | `check_kb_consolidation` | none |
| `spawn_intent_consumer` (external) | `intent_analyst.rs:32` | `TurnExtracted` | 60 s OR accum 10 turns per session | `process_session_intents` | `is_provider_paused(Sonnet)` |

Common shape: `rx = timeline_tx.subscribe()` → inner `tokio::select!` with `shutdown_rx.changed()` biased before `rx.recv()`. Pending state usually HashSet<session> or bool flag. Trailing-edge debounce via `timeout_at(deadline, rx.recv())`.

→ Phase 3-4 work: eight consumers become 8 `bus.subscribe::<T>().debounce(…)` call sites. The helper `lagged_backoff` is replaced by v2's `FailurePolicy::Retry { max, backoff: exp }`. `reconciliation_sweep` becomes unnecessary once subscription cursors persist (reconciler existed to paper over broadcast Lagged). Control-tree gates stay in consumer callbacks OR move into Dispatcher per v2 §4.2.c `control-gate`.

---

## 6. Four MPSC bypass channels — end-to-end trace

### 6a. `embedding_tx` — `mpsc::Sender<EmbeddingTask>` (bounded 256)

- **Sender type:** `tokio::sync::mpsc::Sender<EmbeddingTask>` (main.rs:472)
- **Receiver type:** `mpsc::Receiver<EmbeddingTask>` consumed by `EmbeddingLoopWorker.rx` (workers/sonnet/embedding_worker.rs:978)
- **Payload:** `enum EmbeddingTask { ProcessSession(String), ProcessTurns, ProcessKBEntry, ProcessSkillTopic, BackfillAll, RunBackfillPhase{phase,cursor}, ProcessAstBatch(Vec<String>), ProcessMessage{message_id, session_id, role, content} }` at `state.rs:301-323`
- **Producers (9 sites):**
  - `main.rs:1108` — startup `BackfillAll`
  - `main.rs:1240` — health monitor 15-min periodic `BackfillAll`
  - `infra/message_handler.rs:572` — `ProcessMessage` on new conversation msg
  - `handlers/knowledge/kb.rs:288, 531` — `ProcessKBEntry` after remember
  - `handlers/knowledge/skill.rs:507, 575` — `ProcessSkillTopic`
  - `workers/local/tagger_chunker.rs:155, 197, 222` — `ProcessTurns`
  - `workers/local/pty_event_worker.rs:113` — `ProcessSession` on session close
  - `workers/local/ast_sync_worker.rs:538` — `ProcessAstBatch` after AST sync
  - `workers/local/conversation_logger.rs:103` — `ProcessSession` on compaction
  - `workers/sonnet/embedding_worker.rs:1265` `backfill_enqueue` — recursive self-trigger for phase batches
- **Consumer context:** `EmbeddingLoopWorker` inside `workers/sonnet/embedding_worker.rs:1016-1200` processes tasks sequentially with yield between batches; gated by `ControlTree.is_domain_paused(Memory)` via `Dependency::Domain(CtlDomain::Memory)` at L989-990.
- **Bind point (daemon wiring):** main.rs:472 (create), 786 (→ state), 1118 (→ worker spawn with `rx`)

→ Phase 1-2 work: replace by `EmbeddingEvent::Requested { kind: ProcessKind, …payload }` appended to bus. EmbeddingLoopWorker becomes a `bus.subscribe::<EmbeddingEvent>(name, DropAndLiveResume)`.

### 6b. `ast_sync_tx` — `mpsc::Sender<AstSyncTask>` (bounded 64)

- **Sender type:** `tokio::sync::mpsc::Sender<AstSyncTask>` (main.rs:475)
- **Receiver type:** `mpsc::Receiver<AstSyncTask>` consumed by `AstSyncWorker.rx` (workers/local/ast_sync_worker.rs:58)
- **Payload:** `enum AstSyncTask { CommitSync{repo_path, repo_name, old_hash, new_hash}, FullSync{repo_path, repo_name}, FileSync{repo_path, repo_name, file_path} }` at workers/local/ast_sync_worker.rs:34-53
- **Producers (1 site):** `main.rs:1298` — startup `FullSync` per repo root. **No other producer.** Commit sync is currently triggered via `ast_sync_worker::coalesce_commits` internal loop reading from its own rx buffer (L133) after it received the first CommitSync — but the only external sender is the startup scan.
- **Consumer context:** `AstSyncWorker` (workers/local/ast_sync_worker.rs:68) dispatches to `process_commit_sync` / `process_full_sync` / `process_file_sync`. Emits `EmbeddingTask::ProcessAstBatch` downstream.
- **Gap noted:** no commit-driven producer in this tree. `ContextualCommitDetected` (emitted by tagger_chunker.rs:318) is consumed by arch_maintenance + lisp_survey but does NOT trigger AST sync. AST sync relies only on startup full-sync and manual/FullSync dispatch. This is likely a design gap — investigate during Phase 1.
- **Bind point:** main.rs:475 (create), 794 (→ state), 1275 (→ worker spawn)

→ Phase 1-2 work: replace by `AstSyncEvent::Requested { kind: Full|Commit|File, payload }`. Startup code becomes an `append` call. Worker subscribes. Closes the gap where ContextualCommitDetected could wire to CommitSync naturally.

### 6c. `incident_tx` — `mpsc::Sender<MissionIncident>` (bounded 500)

- **Sender type:** `tokio::sync::mpsc::Sender<missiond_core::types::MissionIncident>` (main.rs:468)
- **Receiver type:** `mpsc::Receiver<MissionIncident>` consumed inline by a task at main.rs:1380
- **Payload:** `MissionIncident { id, severity, source, title, description, server_id, raw_payload, created_at }` at `/Users/jinchen/Projects/missiond/crates/missiond-core/src/types/incident.rs:53`
- **Producers (5 sites):**
  - `handlers/sysinfra/misc.rs:400` — `try_send` from MCP `mission_incident` tool
  - `infra/aiops.rs:123` — health-scan derived incidents
  - `engine/intent_engine/autopilot.rs:810` — autopilot-detected incidents
  - `workers/local/pty_event_worker.rs:712` — PTY MCP error incidents
  - `crates/missiond-core/src/ws/server.rs` — webhook parsing at L167-205 (deploy webhooks and test webhooks); `frontend_events_tx` passthrough
- **Consumer context:** single task at `main.rs:1377-1385` calls `process_incident(&state, incident)` in `infra/aiops.rs:141`. That function does board-task dedup via `dedupe_key`, creates Board tasks, and publishes `DaemonEvent::TaskCreated` back on the event bus (aiops.rs:316, 409).
- **Bind point:** main.rs:468 (create), 499 (→ WS options), 787 (→ state), 1377-1385 (consumer task)

→ Phase 1-2 work: unify with `IncidentEvent::Reported { …fields }`. The aiops.rs process_incident logic moves into an `IncidentEvent` subscriber. TaskCreated re-publish stays as an effect within the subscriber (causation_depth + 1).

### 6d. `cursor_ack_tx` — `mpsc::UnboundedSender<(String, u64)>`

- **Sender type:** `tokio::sync::mpsc::UnboundedSender<(path: String, offset: u64)>` (main.rs:843)
- **Receiver type:** unbounded `Receiver<(String, u64)>` consumed by an ad-hoc task at main.rs:846-858
- **Payload:** `(jsonl_file_path, read_end_offset)` tuple. Path suffix routes to the correct watcher.
- **Producers (1 site):** `workers/local/conversation_logger.rs:58` — acks cursor after PG INSERT succeeds so watcher can advance its persisted cursor.
- **Consumer context:** inline task (main.rs:846-858) — routes `.json` → `gemini_tasks_ref.persist_cursor_ack`; other (i.e. `.jsonl`) → `cc_tasks_ref.persist_cursor_ack`. Watchers persist cursor to `watcher_cursors` DB table only AFTER ack.
- **Bind point:** main.rs:839 (create+inline consumer task), 860 (→ state)

**Design intent for v2 (per frozen lisp §4.1 dead-bypass):** `cursor_ack_tx` is NOT promoted to an event — per `intent-event-bus.lisp:80` it stays inside `conversation-logger` worker as internal cursor tracking. The v2 plan is to remove the cross-task channel and merge the ack loop into the logger worker task itself.

→ Phase 1-2 work: first 3 bypasses become domain events; cursor_ack_tx is eliminated, not migrated. Phase 0 flag: `conversation_logger` worker must be refactored to own both the watcher-event consumer AND the cursor persist, removing the ack MPSC hop entirely.

---

## 7. WebSocket layer contract

### 7a. `frontend_events_tx` definition

- Defined at `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/main.rs:488`:
  ```rust
  let (frontend_events_tx, _) = broadcast::channel::<String>(256);
  ```
- Type: `tokio::sync::broadcast::Sender<String>` — each message is a pre-serialized JSON string.

### 7b. Senders (producers)

- `run_timeline_writer` at `main.rs:132, 195` — sends `TimelineEvent.to_frontend_json()` for both ephemeral and persistent events.
- Health snapshot injector at `main.rs:1312-1357` — sends a `health_snapshot` JSON object every 5 s (type="health_snapshot", seq=-1 sentinel). Skipped when `ws_tx.receiver_count() == 0`.

### 7c. Receivers (consumers)

- WS server `/events` route handler at `crates/missiond-core/src/ws/server.rs:1840 → 2092 handle_events_subscription` — subscribes, forwards to browser client. Ping keepalive 15 s, lagged resync after 3 consecutive Lags → disconnect with close code 4008.
- WS option struct at `ws/server.rs:67` (`WSServerOptions.frontend_events_tx`); wired via `main.rs:500`.

### 7d. `to_frontend_json()` wire format

Implementation at `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/event_bus.rs:1009-1020`:
```json
{
  "type":           "<wire_type()>",      // e.g. "board_task_created"
  "ts":             <unix_ms>,
  "seq":            <i64 from DB, 0 for ephemeral, -1 for health_snapshot>,
  "trace_id":       "<uuid> | null",
  "span_id":        "<uuid>",
  "parent_span_id": "<uuid> | null",
  "payload":        { …to_frontend_payload() variant-specific object… }
}
```

### 7e. Frontend catch-up / sync protocol

In `ws/server.rs:2092 handle_events_subscription`:
- On connect, send `{"type":"connected", "ts":…, "seq":<timeline_latest_seq>}` (L2123-2128).
- Client can send `{"action":"sync","since_seq":N}` — server replays from DB via `query_timeline_since(since_seq, 1000)`; caps gap at 1000 → `{"type":"too_far_behind"}`; completes with `{"type":"caught_up", "seq":…}` (`handle_catch_up` at L2201-2258).
- Client slow → `{"type":"resync","missed":n}` then disconnect after 3 consecutive.

### 7f. Frontend consumption (packages/…)

Not examined in detail; the contract is defined by the JSON schema above. Any Phase touching this must grep `packages/` for `type === "..."` switch statements.

→ Phase 6-7 work: v2 must preserve this JSON envelope shape OR version it (`type_v2` prefix?). The frontend `sync` protocol is strongly coupled to DB `timeline_latest_seq` / `query_timeline_since` — both must continue working on `event_log` (renamed or aliased) until frontend is updated.

---

## 8. `system_timeline` table — all usages

### 8a. DB schema

- Migration `/Users/jinchen/Projects/missiond/crates/missiond-core/migrations/20260318000000_init.sql:546-565` — creates table + 6 indexes + `fts_doc` generated `tsvector` (PG). Columns: `seq BIGINT GENERATED BY DEFAULT AS IDENTITY PK, trace_id, span_id, parent_span_id, event_type, summary, payload TEXT, created_at, fts_doc`.
- Legacy SQLite migration at `crates/missiond-core/src/db/migration.rs:987-1271` (still compiled but PG is primary).

### 8b. Write path

- Single writer: `run_timeline_writer` → `store.insert_timeline_batch` (main.rs:177, see §3).
- PG impl: `/Users/jinchen/Projects/missiond/crates/missiond-core/src/db/pg/timeline.rs:57` — `INSERT INTO system_timeline (…) RETURNING seq`.
- SQLite impl: `crates/missiond-core/src/db/sqlite/timeline.rs`.

### 8c. Read path

- MCP tool `mission_timeline` at `crates/missiond-mcp/src/tools/comm/timeline.rs:11` (definition) with handler `crates/missiond-daemon/src/handlers/comm/timeline.rs` (query/trace/stats/search action dispatch).
- MCP handler calls `query_timeline_filtered`, `query_timeline_stratified`, `query_timeline_by_trace`, `query_timeline_stats`, `query_timeline_search` (all on `MissionStore` trait at `db/traits.rs:459-469`).
- WS catch-up: `ws/server.rs:2201 handle_catch_up` uses `query_timeline_since(since_seq, 1000)`.
- Briefing worker: `find_timeline_needing_briefing` (traits.rs:463) + `update_timeline_summary` (traits.rs:464) — consumers in `workers/sonnet/briefing_worker.rs` mutate existing rows.
- Timeline Analyst: reads stratified timeline for LLM analysis (engine/learning_engine/timeline_analyst.rs).
- Conversation handler: `handlers/comm/conversation.rs` queries timeline for conversation context.

### 8d. TTL / cleanup

- Called once at startup: `main.rs:1367` — `store.cleanup_timeline_ttl(7).await` (7-day retention).
- Trait: `cleanup_timeline_ttl(days: i64) -> usize` at `db/traits.rs:462`.
- Global note in execution lisp: `system_timeline 旧数据不迁,保留 7 天 TTL 只读归档,3 月后废弃` (execution.lisp:81).

### 8e. FTS

- PG: `fts_doc tsvector` generated column + `idx_tl_fts GIN` (migration L556-564).
- SQLite legacy: `system_timeline_fts` virtual table via FTS5 + content/delete/insert triggers (migration.rs:1231-1271).

→ Phase 5 work: new `event_log` table per v2 spec co-exists with `system_timeline`. Writer migration → `event_log`. Reader migration is phased — WS catch-up and briefing worker migrate early, MCP `mission_timeline` migrates late (may alias query to `event_log` with a view). 3-month sunset planned.

---

## 9. `events_sync.rs` — naming clarification

**File:** `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/events_sync.rs` (1043 lines).

Despite the name this module has **nothing to do with the event bus**. Module doc at L1-5 confirms:

> JSONL event sync: routing, backfill, and TTL cleanup.
> - handle_new_events: routes raw JSONL values to conversation_messages / conversation_events
> - backfill_conversation_events: one-time historical data backfill on startup

### Real functions

- `extract_visible_text` / `extract_tool_names` / `extract_tool_names_csv` / `extract_text_content` / `sanitize_raw_content` / `floor_char_boundary` — pure content helpers (L17-287).
- `extract_tool_calls_from_assistant` (L288) / `extract_tool_results_from_user` (L325).
- `handle_new_events(state, session_id, events: Vec<Value>)` (L394) — ingestion entry for raw JSONL values read by watcher; routes progress → `conversation_messages` with `agent_*` roles, everything else → `conversation_events` table. **NOT an event-bus producer.**
- `backfill_conversation_events(state)` (L670) — one-time startup backfill scanning historical JSONL files.
- `backfill_tool_calls(state)` (L764) — extracts tool call structure from existing `conversation_messages` into `conversation_tool_calls` table.
- `reconcile_conversation_messages(state, session_id, path)` (L932) — integrity reconciler invoked from `conversation_logger` and `handlers/comm/conversation.rs`.

### Callers

- `main.rs:1005` — `events_sync::backfill_conversation_events` on startup
- `main.rs:1013` — `events_sync::backfill_tool_calls` on startup
- `workers/local/conversation_logger.rs:61, 274` — `handle_new_events` + `reconcile_conversation_messages`
- `handlers/comm/conversation.rs:1421` — manual reconcile from MCP tool

### Confirmation

No `event_bus.publish` calls in the file. No `DaemonEvent::…` construction. The "events" in the filename refers to JSONL "events" (progress/system/file-history-snapshot rows), NOT `DaemonEvent`.

→ Phase 0 note: rename to `conversation_jsonl_sync.rs` during v2 cleanup to remove confusion. Not a blocker.

---

## 10. Control-tree domain gate — current usage

Module: `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/control_tree.rs`. Gate API: `ControlTree::is_domain_paused(&self, d: CtlDomain) -> bool` at L135-137 — returns `global_paused OR domains[d]`.

`CtlDomain` variants (L55-60): `Memory | Flow | Board | Strategy` (4 total, not 12 as per v2).

### All `is_domain_paused` call sites

- `event_router.rs:114, 134, 147, 196, 403, 461` — 6 sites, all `CtlDomain::Memory`. Response: skip work (no enqueue / no dispatch). Events continue flowing; only the **consumer action** is gated.
- `engine/intent_engine/autopilot.rs:90` — `CtlDomain::Memory`
- `engine/learning_engine/idle_explorer.rs:36` — global_paused OR `CtlDomain::Memory`
- `handlers/knowledge/memory.rs:164, 189` — MCP gate/ungate
- `handlers/sysinfra/misc.rs:103` — health snapshot payload inclusion
- `context/claude_md_sync.rs:15` — `CtlDomain::Strategy`
- `main.rs:1331` — health snapshot payload

### Domain-pause behavior summary

**Current (v1):** the gate lives at the **consumer callback** (e.g. inside `schedule_memory_tasks` invocation site). Events still go through the Timeline Writer → broadcast → consumer; consumer decides per-call whether to act. Side effects: paused domains still **write events to DB** and **fan out to WS/consumers**. Only the scheduling/handler side-effect is suppressed.

**v2 design (§4.2.c):** Dispatcher consults `is_domain_paused` BEFORE fanning out; paused domain's events are **not delivered** (still persisted to Log). Default pause_behavior `DropAndLiveResume` means the subscriber's cursor jumps to head on resume — no backlog replay.

### Mismatch — domain cardinality

v1 has 4 domains (Memory/Flow/Board/Strategy); v2 has 12 `Domain` values (one per enum). **These are orthogonal concepts**:
- v1 `CtlDomain` = "functional area to pause" (4 coarse buckets).
- v2 `Domain` = "type of event" (12, one per `DomainEvent` enum).

The frozen lisp §4.2.c `control-gate` says "暂停域不进 topic", which needs clarification — likely the v2 ControlManager keeps its 4-value `CtlDomain` semantics (Memory/Flow/Board/Strategy) and maps each event's `Domain::SlotEvent → CtlDomain::Memory` etc. when deciding gating. This is **NOT spelled out explicitly** in the frozen lisp and is a Phase 3 decision point. Flag as `decision` entry during Phase 3.

→ Phase 3-4 work: the existing 4-value `CtlDomain` model stays as pause keys; Dispatcher adds a mapping layer Domain → CtlDomain (many-to-one). This mapping should live in `control_tree.rs` as a `Domain::to_ctl_domain() -> CtlDomain` helper.

---

## 11. Other bus-adjacent code

### 11a. Sweeper

- `spawn_sweeper(&state)` at `event_router.rs:569` — runs `reconciliation_sweep` on startup + every 30 min.
- `reconciliation_sweep` (L585-605) calls: `check_realtime_extraction`, `strategy_notify.notify_one()`, `retro_notify.notify_one()`, `check_deep_analysis`, `check_kb_consolidation`, `check_kb_reflection`.
- Purpose: recovers events missed by broadcast `Lagged` or consumer restart gaps.

→ Phase 3 work: sweeper becomes redundant once subscription cursors persist. Remove during migration cleanup.

### 11b. Backfill functions

- `events_sync::backfill_conversation_events` — JSONL historical scan (not bus-related; see §9).
- `events_sync::backfill_tool_calls` — DB-to-DB backfill of `conversation_tool_calls` (not bus-related).
- Embedding backfill at `main.rs:1018-1084` — issues `EmbeddingTask::BackfillAll` and warms caches; one-shot.
- AST embedding health monitor at `main.rs:1220-1253` — 15-min loop, issues `EmbeddingTask::BackfillAll` when gap detected (anti-pattern: polling recovery).
- Startup catchup for watchers at `main.rs:1411-1420` — `run_startup_catchup` on cc_tasks + gemini_tasks.

### 11c. WorkerRegistry + BackgroundWorker trait

- `crates/missiond-daemon/src/workers/mod.rs` — `BackgroundWorker` trait with `KIND: WorkerKind`, `name()`, `run(state, ctx)`, optional `dependencies()` returning `Vec<Dependency>`. `WorkerContext` has `wait_if_paused`/`wait_until_paused` for cooperative pause.
- `spawn_worker(worker, state, shutdown_rx)` is the unified spawn API.
- Worker pause is orthogonal to event bus — it sits between the Dispatcher and consumer callback. v2 `control-gate` at Dispatcher + `PauseBehavior` at subscription subsumes most of this, but the worker's own queue drain semantics remain.

### 11d. Notify channels (non-bus control-flow)

- `briefing_notify`, `strategy_notify`, `retro_notify` at state.rs — `Arc<tokio::sync::Notify>` used to wake workers on demand (complements broadcast subscription). Used in `session_reflection_consumer` (event_router.rs:473-474) and `reconciliation_sweep` (event_router.rs:592-593).

→ Phase 3-4 work: these Notify channels should probably go away — a subscription with debounce accomplishes the same thing more uniformly. Flag for Phase 3.

### 11e. Frontend events bypass

- The `frontend_events_tx` broadcast channel (see §7) is a **derivative** of the timeline broadcast in v1 — the Timeline Writer produces both in lockstep. v2 merges them: Dispatcher's live fan-out plus WS server subscribing like any other consumer.

### 11f. TraceContext — partial implementation

- `TraceContext { trace_id, span_id, parent_span_id, summary }` at `event_bus.rs:961-971` — currently only populated by ~41 `publish_traced` call sites; `publish` sites leave it `None`. Used by Timeline Writer for DB columns. v2 treats trace/span as first-class `SpanContext` in `AppendOpts` — no parallel `publish`/`publish_traced` split.

### 11g. PTY manager event stream

- `pty_logger_rx = pty.subscribe()` at main.rs:464 → `PtyEventWorker.pty_rx` (workers/local/pty_event_worker.rs). This is a **separate** broadcast not related to the timeline bus; it produces `DaemonEvent`s like `SlotBecameIdle`, `SessionCompleted`, `MemoryPhaseChanged`, `TaskCompleted`. So it's a *producer*, not a subscriber of the timeline bus.

### 11h. CC Tasks watcher event stream

- `cc_tasks` manages JSONL file watching — separate subscription pattern at `conversation_logger.rs` (broadcast `WatcherEvent`). Not currently a DaemonEvent publisher. Emission path is: watcher → logger worker → `handle_new_messages_event` → publishes `ConversationMessageLogged` on bus.

→ Phase 7-8 work: decide whether PTY manager + CC watcher events should route through the unified `event_log` or remain internal subsystem broadcasts. Per v2 principle-2 "一条进入路径 — 所有生产者只调 log.append()", they should migrate; but they're internal streams producing external events, not event streams themselves. Likely ruling: keep internal broadcast but synthesize `DomainEvent::append()` at the worker boundary (current pattern).

---

## Summary statistics

- `DaemonEvent` variants: **52** (incl. 6 LEGACY engine-specific).
- Ephemeral variants (skip DB write): **8**.
- Publish call sites: **83** total (42 `publish` + 41 `publish_traced`).
- Unique producer files: **21**.
- `timeline_tx.subscribe()` call sites: **14** (8 in event_router.rs, 6 worker constructors in main.rs — verified by grep).
- `is_domain_paused` call sites: **14** total (most gate consumer actions, not the bus itself).
- MPSC bypasses: **4** (embedding=256 bounded, ast_sync=64 bounded, incident=500 bounded, cursor_ack=unbounded).
- `system_timeline` DB readers: **7 distinct API paths** (timeline_latest_seq, query_timeline_since, query_timeline_filtered, query_timeline_stratified, query_timeline_by_trace, query_timeline_stats, query_timeline_search) + 2 writers (insert_timeline_batch + update_timeline_summary).
- WS frontend wire `type` strings: **~50** (one per DaemonEvent variant via `wire_type()`).

## Top risks for Phase 1-9 migration

1. **Frontend wire-format stickiness.** The JSON envelope shape (`type`, `seq`, `trace_id`, `span_id`, `parent_span_id`, `payload`) is consumed by 1+ browser clients outside this repo. Changing any field name or adding required fields breaks them silently. Mitigation: version the envelope (`v2` suffix on `type` or add `schema_version`) and run both in parallel during Phase 7.
2. **Out-of-order delivery during migration.** Current `run_timeline_writer` fans out ephemeral events BEFORE batch DB insert completes, so seq-ordering guarantees break across the ephemeral/persistent boundary. v2 fixes this (log-first) but Phase 3 rollout must be sure consumers don't rely on the v1 re-ordering quirk.
3. **`CtlDomain` ≠ `Domain` cardinality mismatch.** 4-value pause keys vs 12-value event domains — mapping needs to be nailed down in Phase 3; otherwise "pausing Memory" could unintentionally silence non-memory events.
4. **4 MPSC bypasses diverge in migration cost.** incident/embedding/ast_sync are clean → `DomainEvent` mappings. `cursor_ack_tx` is NOT migrated (becomes worker-internal) — but the refactor needs to land in the conversation_logger worker specifically, not as part of the event bus rewrite.
5. **`DeepAnalysisCompleted`, `KBBatchMutated`, `SessionOrganized`, `TurnExtracted`, `IntentAnalyzed`, `JarvisProactivePush`, `ContextualCommitDetected`, `CascadeTriggered/Completed` are NOT enumerated in the frozen lisp §4.2.a domain-enums list.** Lisp lists a subset of 40-ish variants; 9 above aren't named. They still fit into the 12 domains (see §1 table), but the frozen lisp needs an implicit acceptance that domain-enums are per-domain sum types (not fixed-variant). Flag as `decision` in Phase 1.
6. **Ephemeral categorization is per-variant and hardcoded.** v2 moves this to `AppendOpts.ephemeral` per-call, but callers must be audited to set the right flag. The 8 current ephemeral variants are a mix of "high-volume observability" and "small-payload worker telemetry" — some may justifiably become persistent under v2.
7. **The control-gate at the Dispatcher is a semantic change.** v1 paused domains still produce WS events (via consumer that chooses to no-op); v2 paused domains don't even fan out. If the frontend showed paused-memory events this stops — Phase 7 must verify.
