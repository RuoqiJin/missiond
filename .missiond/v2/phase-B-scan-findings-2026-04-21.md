# Phase-B Scan Findings · 2026-04-21

权威参考报告 — 综合 3 个并行 scan agent 的发现。各 pillar lisp 的 need-more-ground-truth 的 RESOLVED 项指向本文件的对应章节。

**Scan 范围**: 约 40 条 need-more-ground-truth (worker I001-I009 + tools T001-T010 + intent-layer IL-T001-T011 + flow F-T001-T009 + system-layer SL-T001-T008)
**实际 scan 类**: ~22 条 (其余是决策/未来项)
**RESOLVED**: ~18 条
**仍 pending**: ~4 条 (主要是 xjp_router_client 未实现 / intent_graph.rs 外部仓)

---

## A. Engine 家族 (worker I002/I003/I008, intent-layer IL-T001/IL-T007/IL-T009)

### A.1 learning_engine 7 文件精确 R/W 矩阵

| File | Writes | Reads | LLM | Slot | 职责 |
|---|---|---|---|---|---|
| `decision_engine.rs` | agent_questions (answer/retry_count), question_routing_trace | agent_questions (pending), kb_entries (kb_search_ranked/kb_get_by_id), board_tasks, board_task_notes | Gemini (tier-2) | slot-decision (tier-3) | **4 tier 级联决策路由: KB 混合→Gemini 咨询→决策槽→人工** |
| `decision_harvest.rs` | kb_entries (policy:decision category), kb_update (confidence 加成) | agent_questions (target=master) | Gemini (Few-Shot 泛化) | — | **完成任务决策→可复用 policy:decision KB 条目** |
| `extraction.rs` | realtime_forwarded_at watermarks, deep_checkpoint, slot_tasks, conversation_task_id, kb_update/forget | pending_realtime_messages, conversation_messages, kb_list_low_utility | Sonnet (KB reflection) | request_default_slot / request_execution_slot (300-900s) | **水位线实时+深度分析双阶段, KB 去重合并反思** |
| `historical_scanner.rs` | mark_habit_scanned, daemon_state (last_habit_scan_at) | conversation counts, unscanned_conversations | — | request_execution_slot(MEMORY_SLOW_SLOT_ID) | **4h 周期习惯扫描** |
| `idle_explorer.rs` | board_tasks (create, auto_execute=true), kb_remember, daemon_state | daemon_state, kb_entries, board_tasks, beacons, snapshots | — | — | **8 类探索: 一致性/陈旧/信标/重复/聚合/巩固/状态/影子回放** |
| `intent_analyst.rs` | **user_intents**, **conversation_turns.intent_group_id** | conversation_turns (intent_coverage, turns_after) | Sonnet (intent pattern: stuck_retry/architecture_explore/refactor_shift/scope_creep) | — | **会话意图识别 (15 轮批量, 防翻页 bug)** |
| `timeline_analyst.rs` | board_tasks (create), kb_remember (category=ops:insight), daemon_state | timeline_stats (12h), timeline_search (errors), board_tasks dedup | Gemini (12h 系统分析→insight JSON) | — | **12h 周期系统洞察→自动化任务** |

### A.2 intent_engine 4 文件 (I002/IL-T009)

| File | Writes | Reads | LLM | Slot | 职责 |
|---|---|---|---|---|---|
| `autopilot.rs` | slot_context (level 更新), board_task (lease/unclaim), stale conversation cleanup, dynamic slot reaping | board_tasks (trigger_source/lease), slot_tasks, ControlTree (pause) | — | spawn memory slots, dispatch idle slots by role | **60s 主编排脉搏, 双内存槽管理** |
| `flow_engine.rs` (v1) | board_task (status/flow_phase/flow_context/flow_artifacts), board_task_notes | board_task (flow_phase/flow_context/questions/notes), flow_template | Gemini (ConsultGemini1/2 direct) | pty.send (Investigate/Plan/Execute/Finalize) | **EngineeringPhase FSM 7 phase 驱动** |
| `workflow_executor.rs` | skill_execution_insert, skill_execution_update_with_duration | skill_topic_get, workflow blocks, step definitions | — | MCP tool dispatch (30s timeout, MAX_DEPTH=3) | **技能工作流执行: 上下文挂钩/工具链/重试-降级** |
| `memory_scheduler.rs` | board_tasks (claim via memory_hook trigger), slot spawn | board_tasks (trigger_source=memory_hook, idle status), ControlTree | — | ensure_memory_slot_by_id + pty | **优先级队列分发: Submit>Realtime>DeepAnalysis>Consolidation** |

### A.3 Decision-Engine 4 Tier Cascade — **全部已实现** (IL-T007 RESOLVED)

| Tier | Status | 代码位置 | 实现细节 |
|---|---|---|---|
| T1 kb-lookup | ✓ **implemented** | decision_engine.rs:155-180 | kb_search_ranked() + dual scoring + confidence ≥0.5 threshold |
| T2 gemini-consult | ✓ **implemented** | decision_engine.rs:210-260 | call_gemini_for_flow() + JSON 契约 (answer/reasoning/action) + confidence 0.7 |
| T3 decision-slot | ✓ **implemented** | decision_engine.rs:290-340 | request_execution_slot(slot-decision) + pty.send(120s timeout) |
| T4 human-escalation | ✓ **implemented** | decision_engine.rs:360-380 | agent_questions (target=master) + board_task 优先级升级 |

### A.4 Flow-Engine v1 EngineeringPhase — **7 phase 全实现** (IL-T009/I008 RESOLVED)

| Transition | Status | 代码位置 | artifact 要求 |
|---|---|---|---|
| Investigate→ConsultGemini1 | ✓ | flow_engine.rs:150-200 | investigation_report |
| ConsultGemini1→Plan | ✓ | flow_engine.rs:200-250 | gemini_advice_1 |
| Plan→ConsultGemini2 | ✓ | flow_engine.rs:250-300 | execution_plan |
| ConsultGemini2→Execute | ✓ | flow_engine.rs:300-350 | gemini_advice_2 |
| Execute→Finalize | ✓ | flow_engine.rs:350-400 | execution_result + error handling |
| Finalize→Done | ✓ | flow_engine.rs:400-450 | decision_harvest trigger + auto-KB 泛化 |

**关键**: 完成后自动 trigger `decision_harvest` → policy:decision KB 条目沉淀. 这与 learning-engine 形成闭环。

---

## B. Worker 零散项 (I001/I004/I005, T004)

### B.1 slot_manager 残留 (I001 RESOLVED)

**8 处命中, 全无清理需求**:
- main.rs:668-671, 743-771 (字段使用)
- state.rs:214 (AppState 字段)
- missiond-core/src/core/{mod.rs, mission_control.rs} (重新导出 + 5 处字段使用)
- workers/sonnet/{lisp_survey_worker, arch_maintenance_worker}.rs (引用)
- workers/gemini/strategy_worker.rs (引用)

**判断**: slot_manager 变量名保留是**故意 API 稳定性设计**. 实际类型已是 `AgentSlotManager` (slot_orchestrator/agent.rs:1). 旧 slot_manager/ 目录确已删. **无需重命名**.

### B.2 Retrieval-Fusion Ranker (I004 RESOLVED)

**无独立 ranker 文件**, RRF 内联在:
- `context_pipeline.rs:886-893` (KB_SEARCH RRF merge) + `1008-1026` (SKILL_SEARCH RRF merge)
- `context_pipeline.rs:41-44` MIN_RRF_SCORE=0.008 阈值
- `handlers/knowledge/kb.rs:733` mmr_rerank_cosine() (beam search 后去重)

**4 路检索源**: vector (embedding) + fulltext (FTS) + fuzzy + tag/beacon

**tools v0.1 retrieval-fusion entry-components 3 文件足够**: context_pipeline + kb + code_prefetch

### B.3 experience_harvester (I005 RESOLVED — **反转**)

**状态**: ✓ **COMPLETE + ACTIVE** (非 v0.3 标的 planned)
- 420 行完整实现 (非空壳) + 单测
- **Spawn 路径**: `bus/v2_subscribers.rs:237` on **NarrationSessionCompleted** 事件
- 1 caller: `harvest_session(&state, &sid).await`
- 注释 "Gemini-reviewed. See docs/designs/code-intelligence-acceleration.md"
- 功能: 会话完成→提取探索路径→AST 解析→beacon 创建→3 次重复时建议 Skill 合成

**建议**: worker v0.3 :lifecycle-style 从 `planned` 改为 `spawned` (via bus subscriber)

### B.4 7 个跨域 tool (T004 RESOLVED)

**6 历史原因 + 1 故意设计**:

| tool | mcp shell | handler | 类别 |
|---|---|---|---|
| mission_pause | compute/slot.rs | sysinfra/misc.rs | 历史 |
| mission_slot_history | compute/slot.rs | sysinfra/misc.rs (handlers/mod.rs:156 兜底) | 历史 |
| mission_inbox | compute/process.rs | sysinfra/misc.rs | 历史 |
| mission_incident | comm/question.rs | question→sysinfra/misc.rs (中继) | 历史 |
| mission_gemini_auth | comm/question.rs | question→sysinfra/misc.rs (中继) | 历史 |
| mission_submit_phase_result | knowledge/board.rs | sysinfra/misc.rs | 历史 |
| **mission_beacon** | knowledge/kb.rs | knowledge/kb.rs 同文件 | **故意设计 (beacon 属 KB domain)** |

**根因**: old-slot (compute) → new-slot (sysinfra) 迁移时, 工具定义留原 group, handler 集中到 misc.rs (单一职责冲突)

---

## C. System-Layer 底座 (SL-T001-T008 全 RESOLVED)

### C.1 intent-types.lisp 实际清单 (SL-T001)

**13 Enums** (v0.1 说 11, 偏差 2):
- BoardTaskStatus (7: Open/Running/Verifying/Done/Blocked/Failed/Skipped)
- BoardNoteType (3: Progress/Summary/Note)
- EngineeringPhase (7: Investigate/ConsultGemini1/Plan/ConsultGemini2/Execute/Finalize/Done)
- TaskStatus (4) / EventType (6) / AsyncJobStatus (5)
- AgentQuestionStatus (5: Pending/Answered/Dismissed/Expired/Harvested)
- IncidentSeverity (5) / IncidentSource (5)
- DependencyStatus / CliEngine (2) / Lifecycle (2) / SlotTrait (5)

**20 Structs** (v0.1 说 12, 偏差 8):
- FlowContext (6) / BoardTask (37 — **最大**) / CompactBoardTask (7) / BoardTaskNote (6)
- CreateBoardTaskInput (16) / UpdateBoardTaskInput (20)
- Conversation (24) / ConversationMessage (19)
- KnowledgeEntry (16) / KBRememberInput (6) / KBEdge (5)
- Task (16) / InboxMessage (6) / TaskEvent (5) / AgentQuestion (15)
- IncidentRow (10) / DynamicSlot (12) / SkillTopic (15) / SkillBlock (9) / ToolCallRecord (11)

### C.2 IPC 通信拓扑 (SL-T002)

```
 [external MCP client (Claude Code / Gemini CLI / etc.)]
              ↓ stdio (JSON-RPC)
 [missiond-mcp binary]   (reads stdin / writes stdout)
              ↓ Unix socket (MISSION_IPC_SOCKET / MISSION_IPC_ENDPOINT)
 [daemon ipc_handler.rs] (JSON-RPC endpoint, line-per-request)
              ↓
 [daemon tools / KB / context pipeline]

 daemon (内部反向调)
              ↓
 [mcp_client.rs] (outbound, 调 xjp-mcp)
              ↓ child process stdio
 [xjp-mcp] (max 200 calls before recycle, 30s per-tool timeout)
```

**关键**:
- `ipc_handler.rs` = daemon RECEIVE (inbound JSON-RPC)
- `mcp_client.rs` = daemon OUTBOUND (调外部 MCP, max 200 calls 后重启子进程)
- 处理 methods: "ping" / "kb/summary" / "context/prefetch" / "tools/call"

### C.3 WS 双目录关系 (SL-T003)

- `crates/missiond-daemon/src/bus/ws_bridge.rs` — **event-bus v2 → 前端桥** (100ms 轮询 event_log, v2→v1 byte-compat wire format)
- `crates/missiond-core/src/ws/server.rs` — **通用 WS 多路复用** (PTY / tasks / Jarvis chat / incidents / events / screenshot)

**关系**: `ws_bridge` 是**特化**的 event-bus→WS 桥 (产 `frontend_events_tx: broadcast::Sender<String>`), 被 `ws/server` 的 `/events` 路由消费. **互补非重叠**.

```
event_log (PostgreSQL)
    ↓
ws_bridge.rs (100ms poll)
    ↓ v2→v1 wire format 转换
frontend_events_tx (broadcast::Sender<String>)
    ↓
ws/server.rs /events route
    ↓
browser WS client
```

### C.4 aiops.rs Playbook (SL-T004)

**Interval**: 300s (确认)

**Pre-check**: HTTP GET connectivitycheck.gstatic.com/generate_204 → 若不通则 skip 所有 check (防假警报)

**Scan**: 所有配置 `health_endpoint` 的 servers
- HTTP GET 5s timeout per server (tokio::task::JoinSet 并行)
- 判断: response status 2xx 即健康

**Incident 产出**:
```rust
MissionIncident {
  id: "inc-{uuid}",
  severity: High,
  source: HealthCheck | PtySlot,
  title: "{server_name} 健康检查失败",
  description: "...",
  server_id: Some(...),
  raw_payload: json!({ endpoint, server_id, server_name })
}
```

**Remediation**: **是, 自动**
- 健康恢复: 自动 close 对应 Board task + 加 "✅ 已自动恢复 ({time} UTC)" 笔记 (author: aiops)
- 健康失败: 建 Board task + incident record (state-based dedup, 基于 Board task 生命周期)
- PtySlot incident: 派 Claude Code (Opus) slot 处理

### C.5 util/ 文件清单 (SL-T005)

**仅 string_helpers/ 1 子目录**:
- `mod.rs` — re-exports
- `custom.rs` — StringHelpers trait impl (safe_byte_truncate, safe_char_truncate)
- `generated.rs` — trait contract (Forge-generated)

**无其他 util/ 文件**. v0.1 lisp 的 3 component (semantic-parsing-helpers / string-safety / token-budget) 中后两者位置已确认; semantic-parsing-helpers 实际在 `missiond-core/src/semantic/gen_parsing.rs` (非 util/).

### C.6 types/ 手写文件 (SL-T006)

**14 手写扩展** (除 gen_types.rs Forge 输出):
- mod.rs (10 KB, 顶层导出)
- async_job.rs / board.rs (18 KB, 最大 — query builders + lifecycle helpers)
- conversation.rs (11 KB) / directive.rs (6 KB)
- dynamic_slot.rs / incident.rs / infra.rs (4 KB — Server/Cluster/Zone)
- knowledge.rs (4 KB) / project.rs / question.rs / skill.rs / slot.rs (7 KB) / task.rs

**模式**: gen_types.rs = struct 定义 + derives; 手写 = impl 方法 + helper enums + trait impl + utility fn

### C.7 supervisor.rs (SL-T007)

**599 行**, 核心:
- `ExtractionPhase` enum (Idle/Sending/WaitingForIdleness/Complete — 与 intent-layer 的 extraction-phase FSM 一致)
- `ExtractionState` struct — phase tracking + context
- 阈值: `COMPACT_GRACEFUL_THRESHOLD = 15%` / `COMPACT_EMERGENCY_THRESHOLD = 3%`

**核心 fn**:
- `check_slot_context_levels()` — 监控 memory % per slot
- `check_pending_compact_restarts()` — graceful/emergency restart
- `check_slot_stuck()` — 冻结检测
- `check_extraction_gate()` — extraction 流门禁
- `schedule_supervisor_patrol()` — 主协调循环 (line 279)

**Restart 策略**:
- Graceful: context < 15% → 标记 Idle 时 restart
- Emergency: context < 3% → 强制 kill 立即重启
- Recovery: requeue tasks + release Board claims + sleep 3s + respawn via `ensure_memory_slot_by_id()`

**与 ControlTree**: 独立, 不依赖. 与 `state.mission` (MissionConfig) 交互获取 slot list.

### C.8 Env/Config 清单 (SL-T008)

**25+ env vars** (按类别):

| 类别 | env vars |
|---|---|
| **路径/Home** | MISSIOND_HOME, XJP_MISSION_HOME (fallback), FORGE_BIN, HOME, CARGO, SHELL |
| **IPC/Socket** | MISSION_IPC_ENDPOINT, MISSION_IPC_SOCKET |
| **DB** | MISSION_PG_URL |
| **WS** | MISSION_WS_PORT, MISSION_WS_BIND, MISSIOND_API_TOKEN, MISSIOND_DISABLE_CONTEXT_ENRICHMENT |
| **日志** | RUST_LOG, MISSION_LOG_LEVEL, MISSIOND_LOG_FILE |
| **Embedding** | MISSIOND_EMBEDDING_MODEL, MISSIOND_DISABLE_EMBEDDING, OLLAMA_HOST |
| **Cascade** | UNIVERSE_MANIFEST, UNIVERSE_ROOT, CASCADE_TRIGGER_ENABLED |
| **Session** | CLAUDE_SESSION_ID, SESSION_ID |
| **Debug** | MISSIOND_DISABLE_HABIT_SCAN |
| **xjp-router** (预留, v0.3 I006) | xjp_router_endpoint (未实现), xjp_router_auth_token (未实现) |

**关键确认**: `xjp_router_endpoint` / `xjp_router_auth_token` **代码中不存在**, 确认 worker v0.3 I006 为真 — phase-C 施工项。

---

## D. Pending / Future Items (仍需处理)

### 仍 pending (phase-C 施工或外部扫描)
- **I006** (xjp_router_client): 确认未实现, phase-C 施工新增
- **IL-T006** (forge-daemon/intent_graph.rs): 外部仓 ~/Projects/jarvis-forge, 本次未扫

### 决策类 (指挥官评审)
- **T003** (78 tool 必要性评审, 逐条)
- **T005-T008** (mission_minimax deprecated, mission_memory/kb_ops 拆否, skill_exec vs flow-v2)
- **IL-T002-T005/T008-T011** (3 actor / 3 MCP tool / global claudemd manager 未来实现时机)
- **I007** (intent-layer phase-A 后迁移时机)
- **I009** (compute_slot FSM 独立文档决策)

### 未来实现类 (待 phase-C 或设计)
- **F-T001-T009** (flow pillar 具体设计, 大部分待 cascade / incident playbook / flow-v2 Phase 2 等设计)

---

## E. 回填 Plan (各 pillar lisp 更新点)

### E.1 worker v0.3 更新 (commit 预计 +30 行)
- I001 标 RESOLVED (指向 § B.1)
- I002 标 RESOLVED (workflow_executor 确认 — 指向 § A.2)
- I003 标 RESOLVED (learning-engine 7 sub 表矩阵 — 指向 § A.1)
- I004 标 RESOLVED (fusion 内联 — 指向 § B.2)
- **I005 架构级更正**: experience_harvester lifecycle-style `planned` → `spawned`, functional-group 的 note 改 "via bus/v2_subscribers.rs on NarrationSessionCompleted"; path experience-harvester-prototype 的 ingress :source 改为 "NarrationSessionCompleted (via bus/v2_subscribers.rs:237)"
- I006 保留 pending
- I007-I009 保留

### E.2 intent-layer v0.1 更新 (commit 预计 +100 行 — 关键升级)
- **learning-engine 7 sub 的 R/W 内联** (§ 5.8 subsection 补表, 从 Agent A 矩阵填入)
- **decision-engine cascade 4 tier 升为 implemented** (§ 5.8.1 decision-cascade path)
- **flow-engine v1 的 7 phase 明确列出 implemented** (§ 5.9)
- IL-T001 RESOLVED / IL-T007 RESOLVED / IL-T009 RESOLVED
- IL-T006 保留 pending (外部仓)
- 其他 IL-T002-T005/T008-T011 保留决策/未来

### E.3 tools v0.1 更新 (commit 预计 +15 行)
- T001 RESOLVED (78 确认)
- T004 RESOLVED (6 历史 + 1 设计 — 指向 § B.4)
- 其他 T002/T003/T005-T010 保留决策

### E.4 flow v0.1 更新 (commit 预计 +10 行)
- F-T006 部分回答 (autopilot 60s tick)
- 其他保留

### E.5 system-layer v0.1 更新 (commit 预计 +200 行 — 最大)
- **SL-T001 RESOLVED**: 头部 13 enum + 20 struct 修正, enums-shared/structs-shared 列表补全 (Agent C C.1)
- **SL-T002 RESOLVED**: IPC 拓扑补详细图 (§ C.2)
- **SL-T003 RESOLVED**: WS 双目录关系补 (§ C.3)
- **SL-T004 RESOLVED**: aiops playbook 补完整 (§ C.4, remediation 策略详述)
- **SL-T005 RESOLVED**: util/ 只 string_helpers (§ C.5)
- **SL-T006 RESOLVED**: types/ 14 手写文件清单 (§ C.6)
- **SL-T007 RESOLVED**: supervisor 599 行 + 15%/3% 阈值 (§ C.7)
- **SL-T008 RESOLVED**: 25+ env 清单完整 (§ C.8)

---

**生成时间**: 2026-04-21
**Agent roster**: 3 并行 Explore agent (engine 家族 / worker 零散项 / system-layer 底座)
**主 Claude 集成**: 本文件
