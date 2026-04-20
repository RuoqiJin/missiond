# Worker Pillar · phase-A Iteration Brief (for gptpro)

> 本会话 → gptpro 的 phase-A 施工反馈包。
> 目的: gptpro 基于本 brief 把 `intent-worker.lisp v0.1 → v0.2` 迭代到可 freeze 的程度。
>
> 按 `feedback_gptpro_collaboration` 的分工:
> - **gptpro**: 多 pillar 全景 + 系统抽象 + 章节骨架 + 命名哲学
> - **本会话**: 施工反馈 (代码 ground truth / cross-pillar contract / 施工可行性)
> - **指挥官**: 拍板
>
> 生成时间: 2026-04-21
> 生成人: 主 Claude (missiond 项目)

---

## 0. 材料清单 (gptpro 可能需要回看的输入)

| 位置 | 作用 |
|---|---|
| `.missiond/v2/drafts/gptpro/intent-worker.lisp` | **gptpro 自己 2026-04-21 产出的 v0.1 草稿 (273L)** — 本次要迭代的对象 |
| `.missiond/v2/drift-audit-2026-04-21.md` | 跨 pillar 代码 snapshot (本 brief Section 2 已摘 worker 相关部分) |
| `.missiond/v2/worker-pillar-execution.lisp` | worker pillar 施工 execution log, phase-0 snapshot 含 10 pre-deviations (本 brief Section 3 完整转载) |
| `.missiond/v2/intent-pillar-source-index.lisp` | 判真索引 — worker/tools/system-layer/flow 的真况在哪些旧图 |
| `.missiond/v2/intent-memory.lisp` v0.5.1 frozen | memory pillar 完成态 (包含 memory-pillar-defined trait surface, worker 写/读需与此对齐) |
| `.missiond/workflows/pillar-refactor.lisp` | 5 phase 方法论总纲 (phase-A = 本阶段) |

---

## 1. 任务 (gptpro 要交付什么)

**输出**: `intent-worker.lisp v0.2` (写到 `.missiond/v2/drafts/gptpro/intent-worker.lisp`, 本会话回灌正位到 `.missiond/v2/intent-worker.lisp`)

**交付标准**:
1. 吸收或驳回 10 pre-deviations 每一条 (必填: `accept` / `reject+reason` / `partial+how`)
2. 骨架按 P-D001 / P-D004 / P-D006 / P-D009 升级到 ground-truth ontology
3. path 粒度升到磁盘真况 (P-D003 / P-D005 / P-D007 决议)
4. pillar-egress 补 **table-level cross-ref** (P-D008 基于 Section 4 worker × table 矩阵)
5. zombie 政策拍板 (P-D010)

**不要求** (phase-A 不做):
- 代码实现 (那是 phase-C)
- binds-to 精确写入 (那是 phase-B 基于 v0.2 design scan 才做)
- 文字层级 polish / L1 压缩 (phase-E)

---

## 2. Drift-Audit 摘要 (只挑 worker 相关)

### 2.1 Worker 磁盘 vs spawn

- **磁盘**: 19 文件 (不含 mod.rs / registry.rs)
- **spawn**: 17 个 tokio::spawn 调用 (main.rs L1007-1385)
- **未 spawn**: 2 — `code_prefetch.rs` (由 context_pipeline::execute 调用, 非标准 BackgroundWorker) + `experience_harvester.rs` (计划中/未接入)

### 2.2 按子目录分布 (以磁盘实况为准)

| 子目录 | 磁盘数 | spawn 数 | 特殊说明 |
|---|---|---|---|
| `workers/local/` | 12 | 10 | `code_prefetch` + `experience_harvester` 未 spawn |
| `workers/sonnet/` | 5 | 5 | `briefing_worker` v1.3.0 已删 |
| `workers/gemini/` | 1 | 1 | `strategy_worker.rs` |
| `workers/codex/` | 1 | 1 | `vision_worker.rs` ( `step_narrator` v0.4.23 已删) |

> ⚠ drift-audit 自己的文字有写错:
> - 原写 "local/ (10 files, 10 spawned)" 应为 "(12 files, 10 spawned)"
> - 详见 `worker-pillar-execution.lisp :: P-D002`

### 2.3 Engine 家族 (worker pillar 的编排核心)

| 子目录 | 文件数 | 内容 |
|---|---|---|
| `engine/intent_engine/` | 5 | autopilot / flow_engine / memory_scheduler / workflow_executor / gen_engine |
| `engine/learning_engine/` | 8 | decision_engine / decision_harvest / extraction / historical_scanner / idle_explorer / intent_analyst / timeline_analyst / gen_engine |
| `engine/flow/` (别名 `engine/flow/`) | 3 + examples/ | handlers / runner / loader |

> ⚠ v0.1 section 2.4 orchestration 只 3 条 path (worker-lifecycle / autopilot-tick / flow-engine-v2-node), **完全漏了 learning_engine 8 个**, 漏了 memory_scheduler / workflow_executor. 这是 P-D004 的核心。

### 2.4 Infra (cross-pillar boundary, worker 数据面穿行)

| 文件 | 职责 | 归属 | 与 worker 的关系 |
|---|---|---|---|
| `ingestion_router.rs` | 消息分类→worker 路由 | pillar 三/二 交界 | conversation_logger → ingestion_router → message_handler |
| `message_handler.rs` | JSONL 消息转换+插入 | pillar 三 | worker 大多经它写表 |
| `session_util.rs` | PTY session UUID + project_registry | pillar 二/六 | pty_event_worker 直接调 |
| `aiops.rs` | AIOps 健康扫描 | pillar 六 | 独立 spawn, 不在 worker section |
| `daemon_stats.rs` | DB 执行时间+worker 计数器 | pillar 六 observability | 读 worker 侧产出 |
| `ipc_handler.rs` | JSON-RPC endpoint (MCP proxy) | pillar 六 | 不直接涉 worker |
| `mcp_client.rs` | xjp-mcp-config.json 进程客户端 | pillar 六 | 不直接涉 worker |

> ⚠ v0.1 对 `infra/` 目录完全零认知。这是 P-D006 的核心。**不是要把 infra 并入 worker pillar** (它归 system pillar), 而是 worker pillar 要显式声明 "数据面穿行 infra 的 3 个文件" (ingestion_router / message_handler / session_util)。

### 2.5 跨 pillar 表 caller 密度 (总览, 精确矩阵见 Section 4)

| 表 | 总 caller | 边界 |
|---|---|---|
| `board_tasks` | 24+ | pillar 二/三 交界, memory v0.5.0 D010 phased B 已收到 pillar 三 (board module) |
| `event_log` | 40+ | pillar 四 event-bus SSOT, worker 多数是发射方 |
| `conversation_messages` | 多路径 | pillar 三 conversation-logs module |
| `skill` | 2 | lisp_survey_worker (写) + code_prefetch (读) |
| `inbox_messages` | strategy_worker 写 | pillar 三 system-support module (legacy) |

---

## 3. 10 Pre-Deviations (完整转载 + 本会话建议)

来源: `.missiond/v2/worker-pillar-execution.lisp :: pre-deviations`

每条 gptpro 在 v0.2 里必须给个明确处置。

### P-D001 · worker 子目录分类哲学 — active-roster vs WorkerKind

- **v0.1 现状**: section 2.3 用 `(active-roster (sonnet 5) (codex 1) (gemini 1) (local 12))` 平铺 roster
- **Ground truth**: `workers/mod.rs` 明确声明 "Directory structure is the contract: sonnet/, codex/, gemini/, local/" + `enum WorkerKind { Sonnet, Codex, Gemini, Local }` 作 `trait BackgroundWorker::KIND` 正式 ontology. 每个 kind 对应一套 ControlTree provider dependency 注入
- **Nature**: assumption mismatch — v0.1 把 WorkerKind 降级成 "数量+简介" roster, 丢掉了 **trait 契约 + provider dep 注入** 两个最重要的语义
- **本会话建议**:
  ```
  phase-A 把 section 2.3 改成按 WorkerKind 四分:
    worker-sonnet / worker-codex / worker-gemini / worker-local
  每子节独立声明:
    :kind           (与 rust enum 对应)
    :provider-dep   (ControlTree 注入)
    :active-roster  (该 kind 下的 worker 列表)
    多条 path       (每个成员 worker 一条)
  ```
- **gptpro 决策点**: accept / modify / reject?

### P-D002 · local/ worker 数量 (文字 bug)

- **v0.1 现状**: local 12 (与磁盘一致)
- **Ground truth**: 磁盘 12 文件, spawn 10, 另 2 文件 (code_prefetch + experience_harvester) 非标准 spawn
- **Nature**: drift-audit 自己的标题 "(10 files, 10 spawned)" 写错, v0.1 数字对
- **本会话建议**: 顺便让 v0.2 在 `:active-roster (local ...)` 里注明 "12 files on disk, 10 spawned, 2 special"; drift-audit 本会话会修
- **gptpro 决策点**: (无须决策, 仅让 v0.2 计数精确)

### P-D003 · sonnet/ worker path 粒度

- **v0.1 现状**: section 2.3 worker-cluster 的 sonnet 5 worker 合并到 `(path timer-worker-cycle)` 一条粗糙 path
- **Ground truth**: embedding / translation / arch_maintenance / retro / lisp_survey 五个 worker **触发源截然不同**:
  - `embedding_worker` — MPSC channel (EmbeddingTask from ast_sync)
  - `translation_worker` — event bus (MessageEvent::thinking)
  - `arch_maintenance_worker` — event bus (SystemEvent::ContextualCommitDetected)
  - `retro_worker` — event (SessionCompleted)
  - `lisp_survey_worker` — event (SystemEvent::ContextualCommitDetected, 与 arch_maintenance 同源但逻辑不同)
- **Nature**: 粒度不足, 一条 timer-worker-cycle 无法涵盖 event-driven / MPSC 两种机制, 更无法表达每 worker 独立的 logic-core
- **本会话建议**: phase-A 在 `worker-sonnet` 子节下为 5 个 worker 各写一条 path (ingress 标清 event / channel 来源, logic-core 分述, egress 标写入表 — 见 Section 4 矩阵)
- **gptpro 决策点**: accept / modify?

### P-D004 · engine 构成 — learning-engine 全家漏掉

- **v0.1 现状**: section 2.4 orchestration 只有:
  - `path worker-lifecycle-governance`
  - `path autopilot-tick`
  - `path flow-engine-v2-node-execution`
- **Ground truth**: `engine/` 实际 3 子目录 × 16 文件 (见 Section 2.3)
  - intent_engine 5 (autopilot / flow_engine / memory_scheduler / workflow_executor / gen_engine) — **漏 memory_scheduler + workflow_executor**
  - learning_engine 8 — **完全零认知**
  - flow 3 — v0.1 勉强涵盖
- **Nature**: missing section — 这是 v0.1 最大骨架漏洞
- **本会话建议**:
  ```
  phase-A 独立出一个顶级 section engine-cluster:
    (subsection intent-engine)
      autopilot / flow_engine / memory_scheduler / workflow_executor
    (subsection learning-engine)
      decision_engine / decision_harvest / extraction / historical_scanner /
      idle_explorer / intent_analyst / timeline_analyst
    (subsection flow-engine)
      handlers / runner / loader
  与 section 2.3 worker-cluster 并列 (同为 compute tenant, 不混)
  ```
- **gptpro 决策点**: engine-cluster 独立 section 还是并入 orchestration? 如独立, 与 worker-cluster 的关系 (编排 vs 被编排)?

### P-D005 · LLM gateway 文件清点 — 粒度严重不足

- **v0.1 现状**: section 2.2 llm-gateways 只提 sonnet / gemini / codex / minimax 四 provider + 一句 "s4 provider-specific client 执行"
- **Ground truth**: `llm/` 磁盘 14 文件 (见 Section 5 文件职责清单 `llm/` 段)
  - gemini 走 **4 件套**: client (HTTP) + driver (PTY 统一) + pty (传输) + file_api (multimodal)
  - codex 走 cli 单件
  - minimax 有 client + gateway **双层** (legacy, Briefing Worker 用)
  - sonnet 单 gateway (30RPM 独立限流, embedding+chat 双用)
  - 还有 `llm_gate.rs` (AtomicBool kill 开关, 不归任何 provider) + `prompts.rs` (中央 prompt 存储)
- **Nature**: granularity mismatch — `(entry-components [LlmGateway llm_gate])` 严重不足以描述 LLM 层
- **本会话建议**: phase-A 在 `llm-gateways` section 按 provider + kill-switch + prompts 细分子节, 每条 path 的 entry-components 精确到 14 文件中的子集 (Section 5 已备清单)
- **gptpro 决策点**: accept / modify (比如是否合并 gemini 4 件套)?

### P-D006 · Infra 层缺席

- **v0.1 现状**: 对 `crates/missiond-daemon/src/infra/` 目录完全零认知
- **Ground truth**: 7 文件, 3 文件 (ingestion_router / message_handler / session_util) 是 worker 数据面必经, 4 文件 (aiops / daemon_stats / ipc_handler / mcp_client) 归 system pillar
- **Nature**: missing cross-pillar boundary — worker 大量穿越 infra (conversation_logger → ingestion_router → message_handler) 但 v0.1 pillar-egress 对此零声明
- **本会话建议**:
  ```
  phase-A 在 pillar-egress 里补一个 cross-pillar-notes 子块:
    - infra/ 属 system pillar, 但 worker 数据面必经 3 文件:
      * ingestion_router (worker → infra → message 路由)
      * message_handler (JSONL → DB 写入 SSOT)
      * session_util (PTY session UUID + project_registry)
    - 其余 4 (aiops / daemon_stats / ipc_handler / mcp_client) 不穿行
  同时在相关 path (比如 conversation-jsonl-ingestion) 的 logic-core 里
    显式标 step "经 infra::ingestion_router 分类"
  ```
- **gptpro 决策点**: accept? 建议声明要详到什么粒度?

### P-D007 · PTY + slot_orchestrator 粒度

- **v0.1 现状**: `:targets ["crates/missiond-pty/src/*" "slot_manager/" "slot_orchestrator/"]` 用 glob 概括, 11 + 6 + ? 文件**无一列出**
- **Ground truth**: 见 Section 5 文件职责清单 `slot_orchestrator/` + `missiond-pty/` 段
  - slot_orchestrator 11 文件: 3 层架构 (AgentSlotManager → SlotManager → EngineController)
  - missiond-pty 6 文件: Session + Manager + Extractor + Screenshot + Anomaly + lib
- **Nature**: granularity gap — gptpro 用 glob 概括, phase-A 需具体到每个 module 归于哪条 path
- **本会话建议**: 在 section 2.1 pty 的每条 path 的 entry-components 里具体到文件. 比如:
  - `pty-session-lifecycle` → `[PTYManager (manager.rs) PTYSession (session.rs) extractor.rs anomaly.rs screenshot.rs]`
  - `slot-dispatch` → `[AgentSlotManager (agent.rs) ClaudeCodeSlotManager (claude_code.rs) GeminiCliSlotManager (gemini_cli.rs) spawner.rs perm_injector.rs]`
- **gptpro 决策点**: accept / modify?

### P-D008 · 跨 pillar table-level cross-ref 契约

- **v0.1 现状**: pillar-egress :: egress-1 "写回 memory pillar: conversations / board / KB / slots / observability" (单行总结, 无表级精度)
- **Ground truth**: 见 Section 4 worker × table R/W 矩阵 (完整)
- **Nature**: contract gap — worker → memory 的数据流缺显式 table-level contract, pillar 间 SSOT 无法闭环. memory pillar v0.5.1 已经按 writer/reader 注册表格式定义了每个 trait 的 writer/reader, 此处 v0.2 需对齐。
- **本会话建议**:
  ```
  phase-A 参照 memory intent-memory.lisp 的 :writes / :reads 注册表格式,
  在 worker-pillar 的每个 path 的 egress 里显式列:
    :writes  [table-names...]    ;; 参考 Section 4 矩阵
    :reads   [table-names...]
    :via-bus [event-names...]    ;; 发射事件清单
  ```
- **gptpro 决策点**: accept, 但 gptpro 要决定: 是每条 path 都列, 还是在 worker 子节末尾汇总一份 "该 worker 的 R/W 契约表"?

### P-D009 · context pipeline 分类归属

- **v0.1 现状**: section 2.2 llm-gateways 里含 `(path context-assembly)`, 把 context_pipeline / context_budget / slot_env 归 LLM gateway 层
- **Ground truth**: `context/` 磁盘 7 文件 + pure_budget 子目录:
  - claude_md_sync (KB 托管段同步) — **明显不是 LLM 层**
  - topology_map (动态模块导航, AST+KB 聚合) — **明显不是 LLM 层**
  - context_pipeline / context_budget / slot_env / pure_budget/ — 勉强算 LLM 前置
  - 详见 Section 5 文件职责清单 `context/` 段
- **Nature**: file footprint gap + classification question
- **本会话建议**: context 独立一节 `(section context-assembly)`, 与 llm-gateways 并列 (因为 claude_md_sync + topology_map 明显超出 LLM 层)
- **gptpro 决策点**:
  - 选 A: 独立节 section context-assembly, 与 llm-gateways / worker-cluster 并列
  - 选 B: 继续并入 llm-gateways 但补全 7 文件引用
  - (本会话推荐 A)

### P-D010 · zombie 文件与 active-roster 一致性

- **v0.1 现状**: `(known-footprint-drift :note ...)` 提到 briefing_worker / step_narrator 历史, 但把 experience-harvester 列入 "local 12 active"
- **Ground truth**:
  - `code_prefetch.rs`: 活跃但非 spawn (context_pipeline::execute 调用)
  - `experience_harvester.rs`: 未 spawn, 疑似计划功能未接入
  - briefing_worker / step_narrator: 磁盘已删
- **Nature**: roster 定义偏差 — "active" 该怎么定义?
- **本会话建议**: phase-A 决策两点:
  - (a) experience-harvester 升级为 "计划功能" 独立标签, 或移出 roster 归 zombie
  - (b) code-prefetch 独立一条 path `on-demand-retrieval`, 不归 `event-driven-worker-cycle` (因为它不是 BackgroundWorker)
- **gptpro 决策点**: 
  - "active" 定义: spawn ∪ 被其他 runtime code 调用 (code_prefetch 入围)? 还是仅 spawn (code_prefetch 出局)?
  - 建议给一个 `:lifecycle-style` 字段: `spawned` / `on-demand` / `planned` / `zombie-deleted` 四分

---

## 4. Worker × Table R/W 矩阵 (补强 P-D008)

> 本会话 2026-04-21 扫 19 个 worker 文件产出。用于 gptpro 在每个 path 的 egress 里填 table-level contract。

### local/ 子目录

| Worker | Writes | Reads | Via bus |
|---|---|---|---|
| ast_sync_worker | ast_files, ast_nodes, beacon_nodes | ast_files | EmbeddingTask → embedding_tx channel |
| code_prefetch | — | beacon_nodes, ast_nodes (FTS5), ast_search_hits, kb_entries | — |
| codex_ingestion_worker | conversations, conversation_messages, tool_calls | — | — (polls Codex SQLite) |
| conversation_logger | conversation_messages, conversations, board_tasks | conversations, board_tasks, compaction_fragments | WatcherEvent (NewMessages/SessionInactive) |
| conversation_organizer | conversations (link compaction + fix parent links) | conversations | MessageEvent::Logged → SessionEvent::Organized |
| experience_harvester | beacon_nodes, board_tasks | ast_nodes, conversations, tool_calls | (no explicit subscription) |
| gemini_logger | gemini_requests | — | LlmEvent (RequestStarted/ResponseCompleted) |
| gemini_reconcile_worker | conversation_messages, conversations, reconcile_watermarks | reconcile_watermarks, conversations | — (polls ~/.gemini/tmp) |
| pty_event_worker | conversations, slot_sessions, deep_analysis_checkpoint, message_labels, incidents | conversations, slot_sessions, board_tasks | ManagerEvent (TextComplete/Exited/StateChange/ConfirmRequired) |
| reconcile_worker | conversation_messages, reconcile_watermarks | reconcile_watermarks | — (periodic poll ~/.claude/projects) |
| tagger_chunker | message_labels, turns | conversation_messages | SessionEvent::Organized |
| xjpcode_briefing_worker | — (file to ~/.xjpcode/xjpcode.md) | board_tasks, incidents, projects | — |

### sonnet/ 子目录

| Worker | Writes | Reads | Via bus |
|---|---|---|---|
| arch_maintenance_worker | (via SlotManager.execute, indirect) | — | SystemEvent::ContextualCommitDetected |
| embedding_worker | kb_embeddings, ast_embeddings, turn_topics | conversations, ast_nodes, kb_entries, compaction_fragments, ast_embeddings, kb_embeddings | EmbeddingTask (MPSC from ast_sync) |
| lisp_survey_worker | (via SlotManager.execute, indirect) | — | SystemEvent::ContextualCommitDetected |
| retro_worker | deep_analysis, retrospectives | conversations | SessionCompleted |
| translation_worker | message_translations | conversation_messages | MessageEvent (thinking_message) |

### gemini/ 子目录

| Worker | Writes | Reads | Via bus |
|---|---|---|---|
| strategy_worker | inbox_messages, kb_entries, deep_analysis | conversations, kb_entries (strategic-state), daemon_state | SessionCompleted |

### codex/ 子目录

| Worker | Writes | Reads | Via bus |
|---|---|---|---|
| vision_worker | image_descriptions | conversation_messages (raw_content) | MessageEvent (vision tasks) |

### Worker 未显式写 DB, 而是通过 slot execution 或文件

- arch_maintenance_worker / lisp_survey_worker: 通过 SlotManager.execute() 跑 AI slot, 最终 slot 产出落到 `~/.missiond/flows/...` 或 `project/.missiond/intent.lisp` 文件 (非 DB)
- xjpcode_briefing_worker: 仅写 `~/.xjpcode/xjpcode.md` 文件, 不写 DB

### 表按所属 memory module 归组

| Memory module | 涉及的表 | 写入 worker |
|---|---|---|
| `conversation-logs` | conversations, conversation_messages, compaction_fragments, message_labels, message_translations, tool_calls, turns, turn_topics, retrospectives | conversation_logger, codex_ingestion, conversation_organizer, gemini_reconcile, reconcile, tagger_chunker, translation_worker, retro_worker, pty_event_worker, vision_worker |
| `board` | board_tasks | conversation_logger, experience_harvester |
| `kb-manager` | kb_entries, kb_embeddings, beacon_nodes, ast_files, ast_nodes, ast_embeddings, ast_search_hits | ast_sync, embedding_worker, experience_harvester, strategy_worker |
| `project-management` | projects | xjpcode_briefing (read-only) |
| `slot-support` | slot_sessions, slot_tasks | pty_event_worker |
| `system-support` | incidents, inbox_messages, daemon_state, reconcile_watermarks, deep_analysis, deep_analysis_checkpoint, image_descriptions | pty_event_worker, reconcile/gemini_reconcile, strategy_worker, retro_worker, vision_worker |
| `llm-support` | gemini_requests | gemini_logger |

---

## 5. 文件职责清单 (补强 P-D005 / P-D007 / P-D009)

> 本会话 2026-04-21 扫产出。每条一句话职责。用于 gptpro 在 path 的 entry-components 里精确到文件。

### llm/ (14 files)

- `codex_cli.rs` — Codex CLI 子进程包装, vision/image 输入, JSONL 事件解析
- `gemini_cli.rs` — Gemini CLI 子进程, stream-json 模式, tool 执行扩展超时
- `gemini_client.rs` — Gemini API 统一客户端, HTTP/CLI 模式切换, 速率限制
- `gemini_driver.rs` — Gemini PTY 统一驱动, @file 上传 / /clear 隔离 / 事件机制
- `gemini_file_api.rs` — Gemini 文件上传 API, 视频/PDF multimodal, 缓存去重
- `gemini_pty.rs` — Gemini PTY 传输层, Driver 包装, Mutex 原子 /clear+send
- `gen_engine.rs` — LLM 领域引擎框架代码 (Forge 生成), 路由接口定义
- `llm_gate.rs` — 统一 kill 开关, AtomicBool 热路径, provider disable 持久化
- `llm_gateway.rs` — 基础设施层 API 客户端, 多 provider 路由, 无业务逻辑
- `minimax_client.rs` — MiniMax M2.5 HTTP 客户端, Briefing Worker 用, 轻量级
- `minimax_gateway.rs` — MiniMax 优先级 Actor, 4 通道 / 配额跟踪 / 速率控制
- `mod.rs` — 模块导出聚合
- `prompts.rs` — LLM prompt 中央存储, 文件 override 热加载, Tier2/3 模板
- `sonnet_gateway.rs` — Sonnet 优先级 Actor, 30 RPM 独立限流, embedding+chat 双用

### context/ (7 files + pure_budget/)

- `claude_md_sync.rs` — CLAUDE.md 托管段同步, KB preferences + hot topics 自动注入
- `context_budget.rs` — 路由 API HTTP 载荷限流, 6MB 上限防 502, 消息修剪策略
- `context_pipeline.rs` — 懂我 v2 context 预取, Router 意图路由 / 相似度截断 / 并行搜索
- `slot_env.rs` — 工位环境变量构建, secret resolve / session tracking file 生成
- `topology_map.rs` — 动态模块导航层, AST 聚合 / KB module 存储 / 降级查询
- `mod.rs` — 模块导出聚合
- `pure_budget/generated.rs` — TokenBudget trait 定义 (Forge 生成)
- `pure_budget/custom.rs` — Token 估算纯函数, ASCII÷4 CJK÷2, 预算分配衰减
- `pure_budget/mod.rs` — pure_budget 子模块导出

### slot_orchestrator/ (11 files)

- `agent.rs` — AgentSlotManager 顶层路由, task_type→engine sub-manager 分发
- `cc_controller.rs` — Claude Code PTY 操作, JSONL session binding / TextComplete 抽取
- `claude_code.rs` — ClaudeCodeSlotManager 并发治理, persistent Mutex + ephemeral 信号量
- `controller.rs` — EngineController trait, PTY 操作抽象, spawn/ask/clear 统一接口
- `gemini_cli.rs` — GeminiCliSlotManager 并发治理, persistent + ephemeral 生命周期
- `gemini_controller.rs` — GeminiCliController PTY 操作, Driver 委托 / synthetic session_id
- `gen_engine.rs` — SlotOrchestrator 领域引擎框架代码 (Forge 生成)
- `perm_injector.rs` — spawn 时权限注入, LearnedPermissions → settings.local.json 同步
- `spawner.rs` — 统一 tracked slot 生成器, session UUID capture / perm 注入集中化
- `types.rs` — SlotTaskConfig/Request 数据结构, engine / lifecycle / timeout 定义
- `mod.rs` — 3 层架构: AgentSlotManager → SlotManager → EngineController

### missiond-pty/ (6 files)

- `anomaly.rs` — PTY 异常检测器, state stuck / parser 信心 / anchor 缺失被动监控
- `extractor.rs` — 增量 Extractor, frame-by-frame 终端缓冲 diff, spinner / 状态栏过滤
- `lib.rs` — PTY 模块导出聚合 (Session / Manager / Extractor / Screenshot)
- `manager.rs` — PTYManager 多 session 管理, broadcast 事件 / 状态机 / slot 绑定
- `screenshot.rs` — 终端网格 PNG 截屏, ab_glyph 渲染 / 两阶段 capture+render
- `session.rs` — PTYSession 交互式会话, portable-pty + alacritty_terminal + semantic 堆栈

---

## 6. 请 gptpro 明确决策的问题清单

gptpro iterate v0.1 → v0.2 时每一条必须给个明确答复, 写在 v0.2 的 `:phase-A-decisions` section:

| Q# | 关联 P-D | 问题 | 本会话倾向 |
|---|---|---|---|
| Q1 | P-D001 | worker-cluster section 是否改成按 WorkerKind 四分 (sonnet/codex/gemini/local 子节)? | yes |
| Q2 | P-D004 | engine-cluster 独立为顶级 section 还是放 orchestration 子节? | 独立顶级, 与 worker-cluster 并列 |
| Q3 | P-D004 | learning_engine 8 个如何归组? 全部 path 还是按业务 group (decision/extraction/analysis)? | 按 group, 3 subsection: decision / extraction / analysis |
| Q4 | P-D005 | llm-gateways 的 gemini 4 件套合并一条 path 还是分 4 条? | 合 1 条 path `gemini-unified-gateway`, entry-components 列 4 文件 |
| Q5 | P-D006 | infra cross-pillar-notes 用独立块还是散在每条 path 的 logic-core 注释? | 独立块 + 每条 path step 注释双写 (冗余但 grep 友好) |
| Q6 | P-D008 | R/W table 在每条 path 的 egress 列, 还是在每个 worker 子节末尾汇总一份矩阵? | 每条 path 的 egress 里列 (细) + 可选 worker 子节末汇总 |
| Q7 | P-D009 | context 独立 section 还是并入 llm-gateways? | 独立 `(section context-assembly)`, 与 llm-gateways 并列 |
| Q8 | P-D010 | "active" 定义是 spawn ∪ on-demand-call? 建议加 `:lifecycle-style` 四分字段? | 是, 加字段 `spawned / on-demand / planned / zombie` |
| Q9 | 全局 | v0.2 目标行数是多少? (v0.1 是 273 行, memory pillar v0.5.1 frozen 是 2721 行) | 估 500-700 行 (worker 比 memory 行为更多路径) |
| Q10 | 全局 | v0.2 是否保留 `:actual-state-sources` 这种 "底稿指向" 顶部元信息? | 保留但改指 `.missiond/v2/drift-audit-2026-04-21.md` + `.missiond/v2/intent-pillar-source-index.lisp` |

---

## 7. 本会话不会动的红线 (gptpro 放心)

phase-A 完全 gptpro 主导, 本会话承诺:

- ❌ **不自己写 v0.2**: 即使本 brief 给了 "建议" 也只是输入, 骨架决策 gptpro 拍板
- ❌ **不动 memory pillar** (frozen v0.5.1, 除非 phase-E 回灌 worker→memory cross-ref 才动)
- ❌ **不改 v0.1 starter**: gptpro 的 v0.1 `.missiond/v2/drafts/gptpro/intent-worker.lisp` 保留不动, v0.2 gptpro 另存
- ✅ **本会话负责**: drift-audit 文字 bug 修 (P-D002 标题) + v0.2 回灌正位 (drafts/ → .missiond/v2/) + 给 gptpro 二轮反馈 (若有)
- ✅ **施工细节**: phase-B 基于 v0.2 design 扫 binds-to / phase-C 代码对齐 / phase-D 双向校验 / phase-E polish — 全 phase-A 后再启动, 本会话主力

---

## 8. v0.2 产出期待 (gptpro 写到哪里 / 怎么回本会话)

### 写到

```
.missiond/v2/drafts/gptpro/intent-worker-v0.2.lisp
```

保留 v0.1 为历史, 并列存在。

### v0.2 结构建议 (非强制)

```lisp
(pillar worker
  :version "v0.2"
  :status "phase-A draft 2 — pending 指挥官 review"
  :predecessor "v0.1 2026-04-21"

  (phase-A-decisions
    :Q1 ...
    :Q2 ...
    ...)

  (purpose ...)
  (pillar-ingress ...)
  (pillar-core ...)
  (pillar-egress
    :egress-1 ...
    :cross-pillar-notes
      (memory "...")
      (system-infra "...")
      (event-bus "..."))

  (section pty ...)
  (section llm-gateways ...)
  (section context-assembly ...)    ;; 若 Q7=A
  (section worker-cluster
    (subsection worker-sonnet ...)    ;; 若 Q1=yes
    (subsection worker-codex ...)
    (subsection worker-gemini ...)
    (subsection worker-local ...))
  (section engine-cluster             ;; 若 Q2=独立
    (subsection intent-engine ...)
    (subsection learning-engine ...)
    (subsection flow-engine ...))
  (section orchestration-governance   ;; lifecycle + autopilot + spawn
    ...)
  (section worker-side-computation    ;; forge + retrieval
    ...)
)
```

### 回本会话的触发

gptpro 把 v0.2 写到路径后, 指挥官 review, 若 approve 则本会话:
1. 正位 `drafts/gptpro/intent-worker-v0.2.lisp` → `.missiond/v2/intent-worker.lisp`
2. 更新 `worker-pillar-execution.lisp`:
   - 10 pre-deviation 对应 升格 `D001-D010` / drop / merge
   - phase-A-design 标 completed
   - phase-B-scan 启动
3. 进入 phase-B (scan 真实代码 binds-to)

---

## 9. 参考材料索引 (gptpro 如需深读)

| 资料 | 路径 | 何时看 |
|---|---|---|
| memory pillar v0.5.1 (frozen SSOT) | `.missiond/v2/intent-memory.lisp` | cross-pillar contract 对齐 (Q6) |
| memory pillar 施工方法论 | `.missiond/workflows/pillar-refactor.lisp` | phase-A/B/C/D/E 分工 |
| 判真索引 | `.missiond/v2/intent-pillar-source-index.lisp` | 老 intent-pillar-*.lisp 的真相权威 |
| 全景 v2 intent | `.missiond/v2/intent.lisp` | navigation-assets 指路 |
| drift-audit 全文 | `.missiond/v2/drift-audit-2026-04-21.md` | 跨 pillar 背景 (非仅 worker) |
| execution log | `.missiond/v2/worker-pillar-execution.lisp` | 10 pre-deviations 原文 + 施工 lease/claim 协议 |

---

**END OF BRIEF**

gptpro 请基于本文档 iterate v0.1 → v0.2。有需要进一步深扫的 path / file / trait 请在 v0.2 里标 `:need-more-ground-truth <what>`, 本会话会派 agent 补。
