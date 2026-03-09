# MissionD 核心流程图

> 系统中最重要的几条数据通路。
> 出问题时，对照这些图找到断点在哪一步。

---

## 流程 1：用户通过 Board 提交任务 → 工位执行

这是最核心的流程——从你在前端创建一个任务，到工位（Claude Code）实际执行。

```
用户在 Board 点击"创建任务"
    │
    ▼
Board 前端: TaskDialog.tsx → api/tasks/route.ts
    │  POST { title, description, priority, category, assignee, autoExecute }
    ▼
api/tasks/route.ts → lib/missiond.ts → callTool("mission_board_create", ...)
    │  通过 Unix Socket 发 JSON-RPC 请求
    ▼
missiond-mcp: tools/board.rs → 定义参数 schema，转发给 daemon
    │
    ▼
missiond-daemon: handlers/board.rs → handle_board_create()
    │  校验输入 → 写入数据库
    ▼
missiond-core: db/board.rs → insert_board_item()
    │  INSERT INTO board_items (...)
    ▼
handlers/board.rs → 发布事件 BoardCreated
    │
    ▼
event_bus.rs → 广播 DaemonEvent::BoardCreated
    │
    ├──▶ events_sync.rs → 写入 timeline 数据库
    │
    └──▶ ws/server.rs → WebSocket 推送给前端
         │
         ▼
    Board 前端: eventStream.ts 收到版本更新 → store.ts fetchTasks() 刷新列表
```

**任务被 autopilot 自动执行**（如果设了 autoExecute=true）：

```
autopilot.rs → 每次 tick 检查待执行任务
    │
    ▼
flow_engine.rs → 找到 status=open 且 autoExecute=true 的任务
    │  进入流程状态机: Investigate → Plan → Execute → Finalize
    │
    ▼ [Execute 阶段]
flow_engine.rs → 构建 prompt，调用 PTY
    │
    ▼
pty/manager.rs → spawn_session(slot_id, command, env)
    │  创建新的终端进程 (claude --dangerously-skip-permissions ...)
    ▼
pty/session.rs → 开始监听终端输出
    │  逐帧提取文本 → semantic/state.rs 判断状态
    │  (thinking? responding? tool_running? idle?)
    ▼
message_handler.rs → 解析 JSONL 消息 → 写入对话数据库
    │
    ▼ [任务完成]
flow_engine.rs → 更新 Board 状态为 done
    │  发布 BoardUpdated 事件 → 前端刷新
```

---

## 流程 2：记忆提取 — 从对话中提取知识

你和 Claude Code 的对话（元会话）是原始素材，记忆工位从中提取知识存入 KB。

```
你和 Claude Code 对话结束
    │
    ▼
cc_tasks/watcher.rs → 检测到 JSONL 文件变化
    │  解析对话内容 → 发布 ConversationUpdated 事件
    ▼
message_handler.rs → 记录新消息到数据库
    │
    ▼
autopilot.rs → tick 循环
    │
    ▼
memory_scheduler.rs → 检查是否有待提取的对话
    │  判断走实时通道还是慢速通道
    │
    ▼ [实时通道]
extraction.rs → 状态机: Idle → Sending
    │  构建提取 prompt（包含待分析的对话内容）
    │  通过 mission_submit 发给记忆工位
    ▼
记忆工位 (slot-memory) 收到任务
    │  Claude Code 分析对话 → 调用 mission_kb_remember 存储知识
    │
    ▼
handlers/kb.rs → handle_kb_remember()
    │
    ▼
db/knowledge.rs → 写入 KB 条目
    │
    ▼
embedding_worker.rs → 异步生成向量嵌入
    │  用于后续的语义搜索
```

---

## 流程 3：工位提问 → 决策引擎路由

工位在执行任务时遇到不确定的事情，会提出问题。决策引擎自动路由答案。

```
工位执行任务时调用 mission_question_create(question, decisionType)
    │
    ▼
handlers/question.rs → 写入问题到数据库
    │  发布 QuestionCreated 事件
    ▼
decision_engine.rs → 三级路由
    │
    ├── Tier 1: KB 查询
    │   └── kb/knowledge.rs → 搜索知识库
    │       └── 找到相关知识？→ 直接回答，结束
    │
    ├── Tier 2: Gemini 咨询
    │   └── gemini_client.rs → 调 Gemini API
    │       └── Gemini 能回答？→ 存入 KB + 回答工位，结束
    │
    └── Tier 3: 交给用户/决策工位
        └── 问题出现在 Board 的 DecisionDashboard
            └── 用户手动回答 → 回传给工位
```

---

## 流程 4：前端实时更新 — EventBus → WebSocket → UI

Board 前端如何实现实时刷新（不靠轮询）。

```
daemon 内部发生了事情（任务完成、新消息、KB 更新...）
    │
    ▼
event_bus.rs → publish(DaemonEvent::Xxx)
    │  广播给所有订阅者
    │
    ├──▶ events_sync.rs → 写入 timeline 数据库（持久化）
    │
    └──▶ ws/server.rs → 通过 WebSocket 推送给所有连接的客户端
         │  消息格式: { domain: "board", version: 42 }
         ▼
    Board 前端: eventStream.ts
         │  Zustand store 收到消息 → 更新版本计数器
         │  taskVersion++, questionVersion++, ...
         ▼
    hooks/useEventStream.ts → useEventInvalidation("board")
         │  检测到 taskVersion 变了
         ▼
    对应组件 refetch → 调用 api/tasks/route.ts → 获取最新数据 → UI 更新
```

**关键点**：前端不轮询。它订阅 WebSocket，收到版本号变化才去拉数据。这就是为什么有时候你改了数据但 UI 没更新——可能是 WebSocket 断了，或者版本号没被正确 bump。

---

## 流程 5：PTY 状态识别 — 怎么知道 Claude Code 在干什么

```
PTY 终端进程持续输出文本
    │
    ▼
pty/extractor.rs → IncrementalExtractor
    │  逐帧提取终端文本（对比前后帧差异，只取新增内容）
    ▼
semantic/state.rs → ClaudeCodeStateParser
    │  用多种方法判断当前状态:
    │
    ├── semantic/title.rs → 从终端标题读取状态
    │   (claude code 会在标题里写 "Thinking..." 等)
    │
    ├── semantic/fingerprint.rs → 模式匹配 + 评分
    │   (检测特定文本模式，如 "⠋ Thinking", "> ", 工具输出前缀等)
    │
    ├── semantic/tool.rs → 工具输出检测
    │   (识别 Read, Write, Bash, Edit 等已知工具的输出格式)
    │
    └── semantic/confirm.rs → 确认对话框检测
        (检测 "Allow?" "Yes/No" 等需要用户确认的提示)
    │
    ▼
    判定结果: idle | thinking | responding | tool_running | confirming | error
    │
    ▼
pty/session.rs → 更新会话状态 → 发布 SlotStateChanged 事件
    │
    ├──▶ supervisor.rs → 巡检员检查是否卡住
    │
    └──▶ ws/server.rs → 推送给前端
         │
         ▼
    Board: Terminal.tsx → 显示状态指示器
```

---

## 流程 6：Timeline 数据查询

```
用户打开 Timeline Tab
    │
    ▼
CognitiveTimeline.tsx → 按时间窗口和类型请求数据
    │
    ▼
api/timeline/events/route.ts → callTool("mission_timeline_query", {
    │   start, end, event_types, limit, session_id
    │ })
    ▼
handlers/timeline.rs → 从数据库查询事件
    │  events_sync.rs 之前已经把所有事件写入了 timeline 表
    │  支持按类型过滤 (chat/gemini/board/slot/system)
    ▼
返回事件列表 → 前端渲染
    │
    ▼
CognitiveTimeline.tsx 渲染:
    ├── 顶部色条: 事件密度热力图
    ├── 多轨道: Chat / Slot / AI-LLM / Code / Flow / Board / System
    ├── 左侧: 事件列表 (时间 + 类型 + 摘要)
    └── 右侧: 选中事件的详情 (Summary / Payload)
```

---

## 如何使用这些流程图

**当你发现前端问题时：**

1. 确定问题属于哪个流程（任务没显示？记忆没提取？状态不对？）
2. 沿着流程图找到可能断裂的环节
3. 告诉我："我觉得问题出在 flow_engine.rs 的 Execute 阶段，因为任务状态卡在 running 没有变成 done"
4. 我就能直接去那个文件的那个函数看，不用大海捞针

**当你想增加功能时：**

1. 找到最接近的现有流程
2. 确定你的功能应该插入到哪一步
3. 告诉我："我想在流程 1 的 BoardCreated 事件之后加一个通知"
4. 我就知道要改 event_bus.rs 的事件处理和 ws/server.rs 的推送
