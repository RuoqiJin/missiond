# MissionD Drift Audit (2026-04-19)

跨 pillar 代码 snapshot，作为下次 worker/engine/system-layer/infra pillar refactor 的 phase-A 基线数据。

## 1. Worker Footprint

### 磁盘 vs 编译状态
- **磁盘文件**: 19 个实际 worker 文件（不含 mod.rs / registry.rs）
- **实际 spawn**: 17 个（main.rs 行 1007-1385）
- **dead count**: 2 个文件存在但未 spawn

### 按子目录分布

#### local/ (12 files, 10 spawned, 2 non-spawn)
- `ast_sync_worker.rs` — 增量代码索引（git diff → AST 解析）
- `code_prefetch.rs` — P3 混合搜索引擎（FTS5 + embedding）
- `codex_ingestion_worker.rs` — CodeX SQLite 轮询 + JSONL 提取
- `conversation_logger.rs` — CC JSONL 事件主处理管道
- `conversation_organizer.rs` — S2 认知管道：compaction 修复
- `experience_harvester.rs` — 虚拟信标自动生成（高价值探索）
- `gemini_logger.rs` — Gemini 请求日志持久化（v2 bus 订阅）
- `gemini_reconcile_worker.rs` — ~/.gemini/tmp/ 完整性校验
- `pty_event_worker.rs` — PTY 生命周期事件处理
- `reconcile_worker.rs` — JSONL→DB gap 检测+补入
- `tagger_chunker.rs` — S3 认知管道：Turn 提取+噪声标签
- `xjpcode_briefing_worker.rs` — ~/.xjpcode/xjpcode.md 定期生成

#### sonnet/ (5 files, 5 spawned)
- `arch_maintenance_worker.rs` — YAML manifest 自动更新（结构变化时）
- `embedding_worker.rs` — KB/Skill/Conv/AST embedding + backfill
- `lisp_survey_worker.rs` — intent.lisp 增量维护（commit 触发）
- `retro_worker.rs` — Notify 驱动 session 回顾（Sonnet）
- `translation_worker.rs` — 多语言翻译（依赖 SonnetGateway）

#### gemini/ (1 file, 1 spawned)
- `strategy_worker.rs` — Notify 驱动策略分析（Gemini CLI）

#### codex/ (1 file, 1 spawned)
- `vision_worker.rs` — 图像识别 worker（VisionWorker）

### 已删除 workers (注释+代码证据)
- `briefing_worker` (sonnet/) — v1.3.0 SSOT cutover，UPDATE 语义不兼容
- `step_narrator` (codex/) — v0.4.23 Phase 6，message_narrations 表删除

---

## 2. Engine Footprint

### Intent Engine (pillar 二 autocrator)
- `autopilot.rs` — 60s 定期维护 + 空闲触发 board 派发（main.rs:1076-1096）
- `flow_engine.rs` — Board 任务生命周期（Investigate→Plan→Execute→Finalize）
- `memory_scheduler.rs` — 待执行任务队列管理
- `workflow_executor.rs` — skill workflow 步骤执行+MCP 工具调用
- `gen_engine.rs` — FORGE 生成的占位符（零逻辑）

### Learning Engine (pillar 四/五 知识提取)
- `decision_engine.rs` — 决策识别+分类
- `decision_harvest.rs` — 决策泛化+模式归纳
- `extraction.rs` — 事件→知识提取（两阶段：快速+深度）
- `historical_scanner.rs` — 回溯会话扫描
- `intent_analyst.rs` — 意图分析（未必频繁调用）
- `idle_explorer.rs` — 空闲期探索触发
- `timeline_analyst.rs` — 时间轴分析
- `gen_engine.rs` — FORGE 占位符

### Flow / Workflow (pillar 二 DAG)
- `handlers.rs` — 事件路由（FlowStarted → router）
- `runner.rs` — DAG 执行引擎
- `loader.rs` — 工作流定义加载
- `gen_engine.rs` — FORGE 占位符

**Memory 交叉引用**:
- `memory_scheduler` 读取 board_tasks 表（pillar 三 read）
- `autopilot` 派发任务到 board（pillar 三 write）
- Embedding worker 订阅 SessionOrganized 事件（pillar 五 reader）

---

## 3. Infra Footprint

### 7 个基础设施文件

| 文件 | 职责 | Memory 标注 | 交叉 pillar |
|------|------|----------|-----------|
| `ingestion_router.rs` | 消息分类→worker 路由（Conv/Codex/PTY） | 已标：数据入口 | P3 → P2/P4 |
| `message_handler.rs` | JSONL 消息转换+插入（含 ON CONFLICT） | 未标 | P3 ingest |
| `aiops.rs` | AIOps 健康扫描+事件桥接 | 未标：支持层 | P6 monitoring |
| `session_util.rs` | PTY session UUID + project_registry 管理 | 未标 | P2 helpers |
| `daemon_stats.rs` | DB 执行时间+worker 计数器 | 未标：observability | P6 metrics |
| `ipc_handler.rs` | JSON-RPC endpoint（MCP proxy） | 未标 | P6 syscall |
| `mcp_client.rs` | xjp-mcp-config.json 进程客户端 | 未标 | P6 external |

**Memory 覆盖率**: `ingestion_router` 已明确标注为 pillar 三/二 交界；其他 6 个属于 pillar 六 系统层，未来其他 pillar 重构时需补充交叉引用。

---

## 4. Bootstrap Count (Pillar 六)

### 启动时 spawn 的组件

#### Workers (17 个 tokio::spawn 调用)
```
main.rs:1007   EmbeddingLoopWorker
main.rs:1014   GeminiLoggerWorker
main.rs:1020   VisionWorker
main.rs:1031   TranslationWorker (条件：sonnet.is_some())
main.rs:1163   AstSyncWorker
main.rs:1282   ConversationLoggerWorker
main.rs:1303   PtyEventWorker
main.rs:1312   RetroWorker
main.rs:1321   ArchMaintenanceWorker
main.rs:1328   LispSurveyWorker
main.rs:1335   StrategyWorker
main.rs:1344   ReconcileWorker
main.rs:1351   XjpcodeBriefingWorker
main.rs:1358   GeminiReconcileWorker
main.rs:1365   CodexIngestionWorker
main.rs:1373   ConversationOrganizerWorker
main.rs:1381   TaggerChunkerWorker
```

#### Engines & Systems
```
main.rs:1004   v2 event bus subscribers (8 router consumers)
main.rs:1076   Autopilot scheduler (isolated, 60s tick)
main.rs:1103   AST health monitor (15min interval)
main.rs:1164   AST sync backfill (full sync at startup)
main.rs:1201   Health snapshot injector (5s WS push)
main.rs:1270   AIOps health scanner (300s interval)
```

### Spawn vs 磁盘对比
- **Workers on disk**: 19 files
- **Workers spawned**: 17 instances
- **Zombie**: 2 files（code_prefetch.rs, experience_harvester.rs 未直接 spawn）

**注**: code_prefetch 通过 `context_pipeline::execute()` 被 Jarvis 上下文冲注调用（非 spawn），experience_harvester 可能是探索阶段代码。

---

## 5. 跨 Pillar 表 Caller 精确数字

### board_tasks 表（Pillar 二/三 交界）
- **总 caller**: 24+ 处
- **Pillar 二**: autopilot (dispatch_board_tasks), flow_engine, memory_scheduler
- **Pillar 三**: board_dispatch 工具，task 查询路由

### conversation_messages / event_log（Pillar 三 记录）
- **event_log**: 只有 v2 event bus 写入（append-only）
- **消息摄取**: conversation_logger 处理 CC JSONL → ingestion_router → message_handler 的原子路径
- **Caller 密度**: ~40+ 处跨 workers + engine（嵌入式 S1/S2/S3 认知管道）

### inbox（Pillar 二 task 队列）
- **Caller 数**: 0（main.rs 中无直接引用）
- **实现**: 由 Board UI 通过 tool_call(mission_board_create) 写入，autopilot 读取

### skill 表（Pillar 五）
- **SkillIndex::build()**: startup 加载 ~/.claude/skills/
- **Caller**: lisp_survey_worker (SKILL.md 摄取), code_prefetch (混合搜索)

---

## 6. Zombie Files & Dead Code

### 磁盘存在但未 spawn
1. **code_prefetch.rs** (local/)
   - 存在但未直接 spawn
   - 由 context_pipeline::execute() 动态调用（Jarvis 上下文富集）
   - 状态：**活跃**，但隐含耦合（非标准 BackgroundWorker）

2. **experience_harvester.rs** (local/)
   - 存在于磁盘 + workers/local/mod.rs
   - **未在 main.rs 中 spawn**
   - 状态：**可能的探索阶段代码或计划功能**，建议扫描源代码确认意图

### 编译验证 (mod.rs 检查)
- workers/local/mod.rs: ✓ 12 个 pub mod（vs 12 个文件），无孤儿
- workers/sonnet/mod.rs: ✓ 5 个，无孤儿（briefing_worker 已注释删除）
- workers/gemini/mod.rs: ✓ 1 个
- workers/codex/mod.rs: ✓ 1 个（step_narrator 已注释删除）

**孤儿文件统计**: 0（所有磁盘文件都在 mod.rs 中声明）

---

## 7. 建议下次 Pillar Refactor 优先级

### 性价比排序

1. **Pillar 六（系统层）** — **优先级最高**
   - 7 个 infra 文件中仅 `ingestion_router` 有 memory 交叉标注
   - 其他 6 个（aiops, daemon_stats, session_util, ipc_handler, mcp_client）属于 pillar 六 SSOT
   - **收益**: 清晰化基础设施→memory 的单向依赖，减少隐含耦合
   - **成本**: 低（涉及面小，无 business logic）

2. **Pillar 二（Worker/Engine/Intent）** — **优先级次高**
   - 19 个 worker 中 17 个已 spawn，但 BackgroundWorker trait 耦合未理清
   - code_prefetch 隐含耦合（非标准生命周期）
   - **收益**: 统一 worker 生命周期管理，理清 engine→worker 数据流
   - **成本**: 中等（涉及 17 个 spawn 点修改）

3. **Pillar 三/五（Memory/Knowledge）** — **优先级第三**
   - 24+ 处 board_tasks caller 分散在 pillar 二/三
   - event_log/message ingestion 路径明确，但缺乏显式契约（contracts.lisp）
   - **收益**: 收敛 pillar 二/三 边界，明确所有权
   - **成本**: 高（影响认知管道 S1/S2/S3）

4. **Pillar 四（分析）** — **依赖 pillar 三/五 完成后**
   - 6 个 learning_engine 文件职责清晰
   - 待 pillar 三 数据结构 SSOT 后优化

---

## 报告生成

- **扫描时间**: 2026-04-19
- **工具**: grep + ls + Read（代码结构 snapshot）
- **覆盖范围**: workers/ (19 files) + engine/ (20 files) + infra/ (7 files)
- **关键指标**:
  - Spawn point consistency: 17/19 (89.5%)
  - Cross-pillar reference density: 24+ board_tasks, 40+ event_log
  - Zombie files: 2 (code_prefetch + experience_harvester, 需人工确认)
  - Orphan modules: 0

