# MissionD 模块目录

> 每个文件一句话，告诉你它负责什么。
> 看到前端问题 → 查这份目录 → 找到对应文件 → 告诉 AI 去改。

## 子系统导航

| 子系统 | Beacon | Entrypoint | 核心文件 |
|--------|--------|------------|----------|
| 多实例编排 | `@beacon:orchestration` | `autopilot_tick()` | autopilot.rs, flow_engine.rs |
| 工位管理 | `@beacon:slot` | `slot_manager::reload()` | slot_manager.rs, supervisor.rs, slot_env.rs |
| 知识库 | `@beacon:knowledge` | `handle_kb_search()` | db/knowledge.rs, handlers/kb.rs |
| 全息信标 | `@beacon:holographic` | `code_prefetch()` | code_prefetch.rs, ast_sync_worker.rs |
| 认知时间轴 | `@beacon:timeline` | `events_sync::persist()` | events_sync.rs, CognitiveTimeline.tsx |
| 决策引擎 | `@beacon:decision` | `handle_question()` | decision_engine.rs, handlers/question.rs |
| 记忆提取 | `@beacon:memory` | `extraction_tick()` | extraction.rs, memory_scheduler.rs |
| PTY 管理 | `@beacon:pty` | `PtySession::new()` | pty/session.rs, semantic/state.rs |
| Board 任务 | `@beacon:board` | `handle_board_create()` | handlers/board.rs, flow_engine.rs |
| Router Chat | `@beacon:router` | `GeminiClient::send()` | gemini_client.rs, gemini_cli.rs |
| EventBus | `@beacon:eventbus` | `EventBus::publish()` | event_bus.rs, event_router.rs |
| MCP 网关 | `@beacon:mcp` | `dispatch()` | mcp/server.rs, handlers/mod.rs |

> 想了解某子系统的代码签名？用 `mission_code_search` 搜对应 beacon 标签。
> 想看架构决策历史？用 `mission_kb_search` 搜子系统关键词。

---

## 系统架构总览

```
┌─────────────────────────────────────────────────────┐
│  Board (Next.js 前端)                                │
│  用户看到的界面，通过 API Route 调用后端              │
└──────────────┬──────────────────────────────────────┘
               │ HTTP (localhost:3000 → Unix Socket IPC)
┌──────────────▼──────────────────────────────────────┐
│  missiond-mcp (MCP Server)                           │
│  定义所有工具的名字和参数，转发给 daemon 处理         │
└──────────────┬──────────────────────────────────────┘
               │ IPC (Unix Socket / Named Pipe)
┌──────────────▼──────────────────────────────────────┐
│  missiond-daemon (守护进程)                           │
│  核心业务逻辑：任务调度、记忆提取、PTY 管理           │
│  调用 missiond-core 的数据库和 PTY 能力               │
└──────────────┬──────────────────────────────────────┘
               │ 函数调用
┌──────────────▼──────────────────────────────────────┐
│  missiond-core (核心库)                              │
│  数据库操作、PTY 会话、语义解析、WebSocket 广播       │
└─────────────────────────────────────────────────────┘
```

**数据流方向**：Board → MCP → Daemon → Core → SQLite/PTY
**事件流方向**：PTY/DB → EventBus → WebSocket → Board 实时更新

---

## 一、missiond-daemon（守护进程）

> 整个系统的大脑。所有业务逻辑在这里。

### 启动与状态

| 文件 | 职责 |
|------|------|
| `main.rs` | 程序入口，启动所有子系统 |
| `state.rs` | AppState — 全局共享状态，持有 DB、PTY 管理器、事件总线等所有核心对象 |

### 事件系统

| 文件 | 职责 |
|------|------|
| `event_bus.rs` | 事件总线 — 系统内部所有模块通过它发布/订阅事件，实现解耦 |
| `event_router.rs` | 事件路由 — 收到事件后分发给对应的处理器 |
| `events_sync.rs` | 事件持久化 — 把事件写入数据库，维护同步水位线 |
| `message_handler.rs` | 消息处理 — 处理 Claude Code 会话产生的新消息（JSONL 格式），管理对话生命周期 |

### 自动驾驶（Autopilot）

| 文件 | 职责 |
|------|------|
| `autopilot.rs` | **主循环** — 定时 tick，依次执行：记忆提取 → 任务派发 → 决策引擎 → 流程引擎 → 巡检 |
| `memory_scheduler.rs` | 记忆调度 — 确保记忆工位在运行，分发提取任务到实时/慢速通道 |
| `extraction.rs` | 提取状态机 — 管理提取流程（空闲 → 发送中 → 等待工位空闲），触发实时提取、深度分析、KB 整合 |
| `flow_engine.rs` | 流程引擎 — Board 任务的执行生命周期（调查 → 规划 → 执行 → 收尾），驱动 PTY 工位干活 |
| `supervisor.rs` | 巡检员 — 监控工位健康（上下文不足、卡住、认证错误），检查提取门控 |
| `decision_engine.rs` | 决策引擎 — 工位提出问题时的路由：先查 KB → 再问 Gemini → 最后交给决策工位 |
| `decision_harvest.rs` | 决策收割 — 任务执行完后从工位响应中提取决策结果 |

### 外部服务客户端

| 文件 | 职责 |
|------|------|
| `gemini_client.rs` | Gemini API 客户端 — HTTP 调用，请求排队、限速、响应解析 |
| `llm_gateway.rs` | LLM 网关 — 纯基础设施层，为流程引擎调 Gemini，加载对话历史 |
| `minimax_client.rs` | MiniMax 客户端 — 文本处理（摘要、翻译） |
| `mcp_client.rs` | MCP 代理客户端 — 管理 MCP 代理进程的生命周期（启动、杀死、IPC 通信） |

### 后台工作者

| 文件 | 职责 |
|------|------|
| `embedding_worker.rs` | 嵌入工作者 — 异步生成 KB 条目的向量嵌入 |
| `vision_worker.rs` | 视觉工作者 — 处理工具输出中的图片（调 Vision API） |
| `translation_worker.rs` | 翻译工作者 — 将 thinking 内容翻译成中文 |
| `briefing_worker.rs` | 简报工作者 — 任务执行前生成上下文简报 |

### 辅助模块

| 文件 | 职责 |
|------|------|
| `context_budget.rs` | 上下文预算 — 管理 Gemini 请求的上下文窗口，执行截断规则 |
| `codex_cli.rs` | Codex CLI 调用 — 用 codex 命令做代码库索引 |
| `claude_md_sync.rs` | CLAUDE.md 同步 — 把 ~/.claude/CLAUDE.md 的偏好同步到守护进程状态 |
| `slot_env.rs` | 工位环境变量 — 为工位 PTY 会话构建环境变量，捕获 session UUID |
| `daemon_stats.rs` | 守护进程统计 — 聚合指标（tier 命中、任务计数、记忆阶段转换） |
| `prompts.rs` | 提示词模板 — 存储决策引擎、流程引擎等使用的提示词 |
| `session_util.rs` | 会话工具 — 会话管理和查找的辅助函数 |
| `timeline_analyst.rs` | Timeline 分析 — 分析 timeline 事件用于可观测性和调试 |
| `git_watcher.rs` | Git 监视 — 监控 git 仓库变化，发布事件 |
| `aiops.rs` | AIOps — 事故检测和自动响应流程 |
| `helpers.rs` | 工具函数 — 字符边界校验、mission 主目录检测 |
| `lenient.rs` | 宽松反序列化 — MCP 参数的容错解析 |

### 处理器（handlers/）

> 每个 MCP 工具调用最终由这里的函数处理。
> 前端调 API → MCP 转发 → 这里执行 → 返回结果。

| 文件 | 处理的工具 |
|------|-----------|
| `handlers/mod.rs` | 总调度器 — 按工具名前缀路由到对应 handler |
| `handlers/board.rs` | `mission_board_*` — 任务创建/更新/认领/重试/分解 |
| `handlers/kb.rs` | `mission_kb_*` — 知识库搜索/记忆/分析/整合 |
| `handlers/memory.rs` | `mission_memory_*` — 记忆提取/管道追踪/暂停恢复 |
| `handlers/pty.rs` | `mission_pty_*` — PTY 启动/发送/杀死/截屏/状态 |
| `handlers/question.rs` | `mission_question_*` — 问题创建/回答/驳回，决策统计 |
| `handlers/conversation.rs` | `mission_conversation_*` — 对话列表/搜索/清理，token 统计 |
| `handlers/timeline.rs` | `mission_timeline_*` — 时间线查询/追踪/统计/事件过滤 |
| `handlers/skill.rs` | `mission_skill_*` — Skill 搜索/执行/列表/回滚 |
| `handlers/task.rs` | `mission_submit/status/cancel` — 任务提交/状态/取消 |
| `handlers/permission.rs` | `mission_permission_*` — 权限策略获取/设置 |
| `handlers/process.rs` | `mission_spawn/kill/restart` — 进程管理 |
| `handlers/router_chat.rs` | `mission_router_chat_*` — Gemini 对话管理 |
| `handlers/audit.rs` | `mission_audit_*` — 审计追踪 |
| `handlers/cc_tasks.rs` | `mission_cc_*` — Claude Code 任务监控 |
| `handlers/infra.rs` | `mission_infra_*` — 基础设施查询 |
| `handlers/minimax.rs` | `mission_minimax_*` — MiniMax 文本处理 |
| `handlers/health.rs` | `mission_health` — 健康检查 |
| `handlers/misc.rs` | `mission_inbox` 等 — 杂项 |

---

## 二、missiond-core（核心库）

> 被 daemon 调用的底层能力：数据库、PTY、语义解析。

### 数据库（db/）

| 文件 | 职责 |
|------|------|
| `db/mod.rs` | 数据库初始化 — SQLite 连接管理、表结构定义、事务处理 |
| `db/task.rs` | 任务表 — Task 的增删改查、状态更新、按角色查询 |
| `db/slot.rs` | 工位表 — 工位配置和状态持久化 |
| `db/board.rs` | Board 表 — Board 任务操作（创建/更新/列表/按状态分类查询） |
| `db/question.rs` | 问题表 — Agent 提问的追踪（创建/列表/回答/路由追踪） |
| `db/knowledge.rs` | 知识库表 — KB 条目（记忆/搜索/整合队列） |
| `db/skill.rs` | Skill 表 — Skill 索引、版本管理、话题追踪 |
| `db/conversation.rs` | 对话表 — 对话生命周期、会话关联、完成追踪 |
| `db/audit.rs` | 审计表 — 工具调用的审计日志（请求/响应记录） |
| `db/router_chat.rs` | Gemini 对话表 — Gemini 对话历史（获取/创建/加载消息/追加） |
| `db/incident.rs` | 事故表 — 事故追踪（创建/列表/关联 Board 任务） |
| `db/gemini_log.rs` | Gemini 日志表 — API 请求记录（延迟/token 数/模型） |
| `db/vision.rs` | Vision 表 — Vision API 结果存储 |
| `db/executor.rs` | DB 执行器 — 通过 tokio::task::spawn_blocking 异步执行阻塞 DB 操作 |
| `db/error.rs` | 错误类型 — 数据库错误定义 |

### 核心管理（core/）

| 文件 | 职责 |
|------|------|
| `core/mission_control.rs` | **主协调器** — 统一管理任务队列、工位配置、Agent 进程、收件箱 |
| `core/slot_manager.rs` | 工位管理器 — 工位生命周期（启动/重载/杀死），管理配置文件 |
| `core/process_manager.rs` | 进程管理器 — Agent 进程追踪（状态/重启/环境） |
| `core/permission.rs` | 权限策略 — 基于角色和工位的权限规则执行 |
| `core/inbox.rs` | 收件箱 — 任务消息管理（创建/列表/标记已读） |

### PTY 管理（pty/）

| 文件 | 职责 |
|------|------|
| `pty/mod.rs` | PTY 模块入口 |
| `pty/session.rs` | PTY 会话 — 单个终端会话（消息处理、状态追踪、确认对话框） |
| `pty/manager.rs` | PTY 管理器 — 协调多个 PTY 会话，分发事件 |
| `pty/extractor.rs` | 增量提取器 — 逐帧提取终端文本（帧差异、行组装） |
| `pty/screenshot.rs` | PTY 截屏 — 将终端画面渲染为 PNG 图片 |

### 语义终端解析（semantic/）

> 解析 Claude Code 终端输出，判断当前状态。

| 文件 | 职责 |
|------|------|
| `semantic/mod.rs` | 模块入口，公共解析接口 |
| `semantic/confirm.rs` | 确认对话框解析 — 检测终端中的确认提示（选项、动作） |
| `semantic/state.rs` | 状态解析 — 判断终端状态（thinking/responding/tool_running 等） |
| `semantic/status.rs` | 状态指示器 — 解析进度条和状态转轮 |
| `semantic/title.rs` | 标题解析 — 从终端标题提取状态信息 |
| `semantic/tool.rs` | 工具输出解析 — 识别已知工具的输出 |
| `semantic/fingerprint.rs` | 指纹检测 — 基于模式匹配和评分的状态判定 |
| `semantic/types.rs` | 共享类型 — ParserContext、StateDetectionResult、ConfirmInfo 等 |

### 其他核心模块

| 文件 | 职责 |
|------|------|
| `types.rs` | **核心类型定义** — Task、Slot、Board、Conversation 等所有领域对象 |
| `skill.rs` | Skill 索引 — Skill 文件的索引和搜索 |
| `embedding.rs` | FastEmbed — 本地向量嵌入生成 |
| `ipc/mod.rs` | IPC 监听器 — 跨平台进程间通信抽象 |
| `ws/server.rs` | WebSocket 服务器 — 向前端广播事件 |
| `ws/screenshot_broker.rs` | 截屏分发 — 将截屏事件分发给订阅者 |
| `ws/jarvis_trace.rs` | Jarvis 追踪 — 通过 WebSocket 追踪请求 |
| `cc_tasks/watcher.rs` | CC 任务监视器 — 监控项目目录中的 Claude Code 任务变化 |
| `cc_tasks/parser.rs` | CC 任务解析 — 解析 Claude Code JSONL 任务格式 |

---

## 三、missiond-mcp（MCP 服务器）

> 定义工具的"接口"，不含业务逻辑。
> Claude Code 通过 stdio 调用这里，这里转发给 daemon。

| 文件 | 职责 |
|------|------|
| `lib.rs` | 模块根 — JSON-RPC 2.0 协议和服务器导出 |
| `server.rs` | MCP 服务器 — 基于 stdio 的 JSON-RPC 传输，工具调度 |
| `protocol.rs` | 协议定义 — JSON-RPC 2.0（Request/Response/Error） |
| `bin/mission-mcp.rs` | 可执行入口 |

### 工具定义（tools/）

| 文件 | 定义的工具 |
|------|-----------|
| `tools/mod.rs` | 工具注册表 — all_tools()、get_tool()、ToolResult |
| `tools/task.rs` | mission_submit、mission_ask、mission_status、mission_cancel |
| `tools/process.rs` | mission_spawn、mission_kill、mission_restart、mission_agents |
| `tools/pty.rs` | mission_pty_*（spawn/send/kill/screen/history/status/confirm/interrupt） |
| `tools/permission.rs` | mission_permission_*（get/set_role/set_slot/add_auto_allow） |
| `tools/cc_tasks.rs` | mission_cc_*（sessions/tasks/overview/in_progress/trigger_swarm） |
| `tools/kb.rs` | mission_kb_*（search/get/list/remember/forget/analyze 等） |
| `tools/router_chat.rs` | mission_router_chat_*（send/history/list/delete/clear/restore） |
| `tools/memory.rs` | mission_memory_*（pending/done/pause） |
| `tools/conversation.rs` | mission_conversation_*、mission_token_stats |
| `tools/timeline.rs` | mission_timeline_*（query/trace/stats/search） |
| `tools/question.rs` | mission_question_*、mission_decision_stats |
| `tools/board.rs` | mission_board_*（list/create/update/get/delete/toggle/claim 等） |
| `tools/skill.rs` | mission_skill_*、mission_context_build、mission_context_resolve |
| `tools/audit.rs` | mission_audit_*（trace/detail/stats/export） |
| `tools/slot.rs` | mission_slots、mission_slot_history |
| `tools/infra.rs` | mission_infra_*、mission_reachability、mission_os_diagnose |
| `tools/power.rs` | mission_power_control |
| `tools/misc.rs` | mission_inbox、mission_health |

---

## 四、Board 前端（packages/board/）

### 页面与布局

| 文件 | 职责 |
|------|------|
| `app/layout.tsx` | 根布局 — 设置中文 locale，引入全局样式 |
| `app/page.tsx` | 根页面 — 挂载 App 组件 |
| `App.tsx` | **主界面** — 10 个 Tab 的导航容器（Tasks/Terminal/Knowledge/Logs/Memory/Decisions/Deploy/Research/Engine/Timeline） |

### 状态管理

| 文件 | 职责 |
|------|------|
| `store.ts` | Zustand 全局状态 — 任务列表、筛选、分组、对话框控制 |
| `eventStream.ts` | EventBus WebSocket — 维护与 daemon 的实时连接，版本计数器驱动 UI 刷新 |
| `api.ts` | 任务 API 客户端 — 任务的增删改查 |
| `questionsApi.ts` | 问题 API 客户端 — 问题的获取/回答/驳回 |

### Hooks

| 文件 | 职责 |
|------|------|
| `hooks/useEventStream.ts` | EventBus 连接管理 — 初始化 WS、订阅版本变化触发 refetch、连接状态 |
| `hooks/useTimelineGestures.ts` | Timeline 手势 — 滚轮缩放/平移，用 CSS 变量实现 GPU 驱动的位置更新（不触发 React 重渲染） |

### 核心组件

| 文件 | 职责 |
|------|------|
| `CognitiveTimeline.tsx` | **Timeline 可视化** — 分层事件加载，按类型着色（Gemini=品红/GPT=青柠/Commit=黄），支持关键词搜索和因果链追踪 |
| `ResearchBoard.tsx` | 研究面板 — 带笔记的任务卡片，状态循环（open → running → done） |
| `Terminal.tsx` | PTY 终端 — xterm.js 终端查看器，连接 WebSocket，显示工位输出和状态 |
| `KnowledgeBase.tsx` | 知识库浏览器 — KB 条目列表，按分类筛选，删除操作 |
| `Conversations.tsx` | 对话日志 — 会话列表、消息搜索、单条对话查看 |
| `MemoryDashboard.tsx` | 记忆面板 — 提取状态、工位任务统计、token 用量、暂停/恢复控制 |
| `DecisionDashboard.tsx` | 决策面板 — 待处理问题列表、Tier 路由可视化、回答/驳回操作 |
| `DeployDashboard.tsx` | 部署面板 — 部署任务列表、成功率、工位状态 |
| `EngineDashboard.tsx` | 引擎面板 — daemon 健康指标、工位状态、DB/Gemini 延迟统计 |
| `TaskListView.tsx` | 任务列表 — 拖拽排序、树形层级、分组、筛选、折叠展开 |
| `TaskDialog.tsx` | 任务对话框 — 创建/编辑任务（标题/描述/优先级/分类/自动驾驶配置） |
| `TaskFilters.tsx` | 筛选栏 — 搜索、分组方式、分类、优先级 |
| `TaskItem.tsx` | 任务行 — 单个任务的展示（拖拽手柄、状态徽章、进度条） |
| `QuickAdd.tsx` | 快速添加 — 一行输入框，回车创建任务 |
| `PendingQuestions.tsx` | 悬浮问题 — 需要用户回答的 Agent 问题浮层 |

### API Routes

| 路由 | 职责 |
|------|------|
| `api/tasks/route.ts` | 任务 CRUD — GET 列表、POST 创建、PATCH 更新、DELETE 删除 |
| `api/questions/route.ts` | 问题管理 — 列表/回答/驳回 |
| `api/conversations/route.ts` | 对话管理 — 列表/搜索/获取消息 |
| `api/kb/route.ts` | 知识库 — 列表/删除 |
| `api/timeline/events/route.ts` | Timeline 事件 — 分层获取，支持搜索和会话元数据 |
| `api/timeline/stats/route.ts` | Timeline 统计 — 时间窗口内的事件计数 |
| `api/timeline/traces/route.ts` | Timeline 追踪 — 完整因果链（所有事件 + Gemini 详情） |
| `api/memory/status/route.ts` | 记忆状态 — 暂停/运行状态 |
| `api/memory/pause/route.ts` | 记忆暂停 — 暂停/恢复提取 |
| `api/memory/task-stats/route.ts` | 记忆任务统计 — 各类型任务的执行次数 |
| `api/memory/token-stats/route.ts` | Token 统计 — 24h 内各模型的 token 消耗 |
| `api/pty/spawn/route.ts` | PTY 启动 — 为工位启动终端 |
| `api/pty/status/route.ts` | PTY 状态 — 查询工位终端运行状态 |
| `api/pty/kill/route.ts` | PTY 终止 — 杀死工位终端 |
| `api/pty/agents/route.ts` | Agent 列表 — 所有 Agent 进程状态 |
| `api/slots/route.ts` | 工位列表 — 工位配置 + PTY 运行状态 |
| `api/decisions/stats/route.ts` | 决策统计 — Tier 命中率、降级次数 |
| `api/deploy/status/route.ts` | 部署状态 — 成功率、平均时长、运行中任务 |
| `api/system/health/route.ts` | 健康检查 — daemon 存活状态 |
| `api/system/conversation-message/route.ts` | 消息详情 — 按 ID 获取单条消息 |
| `api/system/gemini-content/route.ts` | Gemini 请求详情 — 按 request_id 获取完整内容 |
| `api/system/tool-call/route.ts` | 工具调用详情 — 按 ID 获取审计记录 |
| `api/system/llm-traces/route.ts` | LLM 追踪 — Gemini API 调用日志和统计 |
| `api/system/message-image/route.ts` | 消息图片 — 从 JSONL 对话文件提取图片 |
| `api/conversation-image/route.ts` | 对话图片 — 提供对话中的嵌入图片 |
| `api/images/route.ts` | 缓存图片 — 按 SHA256 提供 vision 缓存图片 |

### 工具与类型

| 文件 | 职责 |
|------|------|
| `lib/missiond.ts` | IPC 客户端 — 通过 Unix Socket 调用 daemon 的 JSON-RPC 接口 |
| `lib/time.ts` | 时间工具 — UTC 解析、北京时间格式化 |
| `lib/utils.ts` | 工具函数 — Tailwind className 合并 |
| `types.ts` | TypeScript 类型 — Task、TaskNote、AgentQuestion、DecisionStats |
| `constants.ts` | 常量配置 — 分类/优先级/状态的颜色和标签、工位选项、流程模板 |
