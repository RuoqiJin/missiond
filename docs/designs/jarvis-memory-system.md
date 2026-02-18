# Jarvis Memory System — 无感实时记忆

> Board Task: (待创建)
> Status: 设计中
> Date: 2026-02-18
> 依赖: jarvis-kb.md (Phase 1-4 已完成)

## 背景

Jarvis KB 已实现，但记忆必须由 Agent 手动调用 `mission_kb_remember` — 阻断对话、依赖 Agent 自觉、无法持续。

目标：**对话过程中零干预地持续积累记忆**。用户无感，Agent 无负担。

## 核心洞察

> 存储是根基，分析是上层建筑。先把日志可靠地流进数据库，后面怎么分析都行。

## 架构概览

```
用户 ↔ Claude (主会话)
         │
         ├─ JSONL 文件 (Claude Code 自动写入)
         │       ↓
         │  CCTasksWatcher (已有, 扩展)
         │       ↓
         │  conversation_messages 表 (新增)
         │       ↓
         │  ┌─────────────────────────────────────┐
         │  │ Memory Agent (常驻 slot-memory)      │
         │  │                                      │
         │  │ Layer 1: 实时分析                     │
         │  │   新消息写入 DB → 通知 agent          │
         │  │   → 分析 → mission_kb_remember       │
         │  │                                      │
         │  │ Layer 2: 定期深度回顾                  │
         │  │   完整会话 + 已有 KB → 交叉分析        │
         │  │   → 深层洞察 → mission_kb_remember    │
         │  └─────────────────────────────────────┘
         │
PTY 会话 (MissionD 管理的 Agent)
         │
         ├─ PTY 事件流 → conversation_messages 表
         └─ (PTY 日志主要给用户看执行过程)
```

## Phase 1: 日志存储 (~200 行 Rust)

### 数据库 Schema

在 `mission.db` 新增两张表：

```sql
CREATE TABLE conversations (
    id TEXT PRIMARY KEY,              -- Claude Code session UUID
    project TEXT,                     -- 项目路径 (cwd)
    slot_id TEXT,                     -- PTY slot ID (仅 MissionD 管理的会话)
    source TEXT NOT NULL,             -- 'claude_cli' | 'pty'
    model TEXT,                       -- 使用的模型
    git_branch TEXT,
    jsonl_path TEXT,                  -- 原始 JSONL 文件路径
    message_count INTEGER DEFAULT 0,  -- 消息计数
    started_at TEXT NOT NULL,
    ended_at TEXT,
    status TEXT DEFAULT 'active',     -- active | completed
    analyzed_at TEXT                  -- 最近一次深度分析时间
);

CREATE TABLE conversation_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,                -- 'user' | 'assistant' | 'tool_use' | 'tool_result'
    content TEXT NOT NULL,             -- 纯文本内容 (assistant 的 text 部分)
    raw_content TEXT,                  -- 原始 JSON content 数组 (保留 tool_use/thinking 结构)
    message_uuid TEXT,                 -- Claude Code 原始 UUID
    parent_uuid TEXT,                  -- 父消息 UUID
    model TEXT,                        -- assistant 消息使用的模型
    timestamp TEXT NOT NULL,
    metadata TEXT,                     -- JSON: {tokenCount, thinkingTokens, toolNames, ...}
    FOREIGN KEY (session_id) REFERENCES conversations(id)
);

CREATE INDEX idx_conv_msg_session ON conversation_messages(session_id);
CREATE INDEX idx_conv_msg_timestamp ON conversation_messages(timestamp);
CREATE INDEX idx_conv_status ON conversations(status);
```

### 为什么不复用 PTY 解析

| | JSONL (Claude CLI 对话) | PTY (终端流) |
|--|---|---|
| 输入 | 结构化 JSON，`serde_json::from_str()` | 终端像素网格，576 行 IncrementalExtractor |
| 内容 | 已有 role/content/tool_use 字段 | 需从屏幕格子"看"出来 |
| 复杂度 | 极低 | 极高 |

**结论：完全不能复用。JSONL 解析只需扩展已有的 `CCMessageLine` 反序列化。**

### 实现：扩展 CCTasksWatcher

当前 `CCTasksWatcher` 已在监控 JSONL 文件变化，但只提取 `todos` 数组。扩展点：

```rust
// cc_tasks/types.rs — CCMessageLine 已有 message 字段
pub struct CCMessageLine {
    #[serde(rename = "type")]
    pub msg_type: Option<String>,           // "user" | "assistant" | ...
    pub message: Option<CCMessage>,          // { role, content }
    pub uuid: Option<String>,
    pub parent_uuid: Option<String>,         // ← 已有但未用
    pub timestamp: Option<String>,           // ← 已有但未用
    pub session_id: Option<String>,          // ← 已有但未用
    pub cwd: Option<String>,                 // ← 新增解析
    pub git_branch: Option<String>,          // ← 新增解析
    pub todos: Option<Vec<CCTask>>,
}
```

扩展 watcher 的 `check_session_for_changes_static()`:

```rust
// 伪代码 — 在检测 todos 变化的同时，写入 conversation_messages
fn check_session_for_changes(session, db) {
    let new_lines = read_new_lines(jsonl_path, &mut file_position);

    for line in new_lines {
        // 已有: todos 变化检测
        if let Some(todos) = &line.todos { ... }

        // 新增: 消息写入 DB
        if let Some(msg) = &line.message {
            if matches!(line.msg_type.as_deref(), Some("user") | Some("assistant")) {
                db.insert_conversation_message(ConversationMessage {
                    session_id: session.id,
                    role: msg.role.clone(),
                    content: extract_text_content(&msg.content),
                    raw_content: serde_json::to_string(&msg.content).ok(),
                    message_uuid: line.uuid.clone(),
                    parent_uuid: line.parent_uuid.clone(),
                    model: msg.model.clone(),
                    timestamp: line.timestamp.clone(),
                    metadata: extract_metadata(&line),
                });
            }
        }
    }
}

/// 从 content 数组中提取纯文本
fn extract_text_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr.iter()
            .filter_map(|item| {
                if item.get("type")?.as_str()? == "text" {
                    item.get("text")?.as_str().map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}
```

### PTY 会话日志

PTY 会话通过 `SessionEvent::TextOutput(Complete)` 事件写入同一张表：

```rust
// daemon main.rs — PTY 事件监听器中增加
SessionEvent::TextOutput(TextOutputEvent::Complete { turn_id, content, timestamp }) => {
    db.insert_conversation_message(ConversationMessage {
        session_id: pty_session_id,
        role: "assistant".to_string(),
        content,
        raw_content: None,          // PTY 没有结构化 content
        model: None,
        timestamp: timestamp_to_iso(timestamp),
        ..Default::default()
    });
}
```

### 新增 DB 方法

```rust
impl MissionDB {
    fn ensure_conversation_tables(&self);
    fn upsert_conversation(&self, conv: &Conversation) -> SqliteResult<()>;
    fn insert_conversation_message(&self, msg: &ConversationMessage) -> SqliteResult<i64>;
    fn get_conversation(&self, id: &str) -> SqliteResult<Option<Conversation>>;
    fn list_conversations(&self, status: Option<&str>, limit: i64) -> SqliteResult<Vec<Conversation>>;
    fn get_conversation_messages(&self, session_id: &str, since_id: Option<i64>) -> SqliteResult<Vec<ConversationMessage>>;
    fn mark_conversation_analyzed(&self, id: &str) -> SqliteResult<()>;
    fn get_unanalyzed_conversations(&self) -> SqliteResult<Vec<Conversation>>;
}
```

### 新增 MCP 工具

```
mission_conversation_list(status?, limit?)     — 列出会话
mission_conversation_get(sessionId)            — 获取会话详情 + 消息
mission_conversation_search(query, limit?)     — 搜索消息内容
```

---

## Phase 2: 实时记忆分析 (~150 行 Rust + slot 配置)

### Memory Agent 工位

`~/.xjp-mission/slots.yaml` 新增：

```yaml
- id: slot-memory
  role: memory
  description: "常驻记忆分析 Agent，监听对话实时提取知识"
  cwd: /Users/jinchen
  mcp_config: ~/.xjp-mission/xjp-mcp-config.json
  auto_start: true
```

### 触发机制

在 CCTasksWatcher 消息写入 DB 后，通知 Memory Agent：

```rust
// 新组件: RealtimeForwarder
struct RealtimeForwarder {
    pty: Arc<PTYManager>,
    db: Arc<MissionDB>,
    pending_messages: Arc<Mutex<Vec<ConversationMessage>>>,
}

impl RealtimeForwarder {
    /// CCTasksWatcher 写入新消息后调用
    fn on_new_message(&self, msg: &ConversationMessage) {
        // 过滤: 不转发 memory agent 自己的会话
        if msg.session_id == self.memory_session_id() { return; }

        // 攒批: 等 assistant turn 完成后一起发
        if msg.role == "assistant" {
            let batch = self.flush_pending(msg.session_id);
            self.send_to_memory_agent(batch);
        } else {
            self.pending_messages.lock().push(msg.clone());
        }
    }

    /// 发送给 memory agent 分析
    async fn send_to_memory_agent(&self, messages: Vec<ConversationMessage>) {
        let prompt = format!(
            "[realtime-extract]\n\
             以下是一段对话的最新轮次。提取值得长期记忆的事实，\
             用 mission_kb_remember 存入。不要存显而易见的事实。\
             只存：架构决策、bug 修复经验、用户偏好、关键发现。\n\n\
             {}",
            format_messages(&messages)
        );

        // fire-and-forget: 不等结果
        let _ = self.pty.send("slot-memory", &prompt, 120_000).await;
    }
}
```

### Memory Agent 行为

Memory Agent 是一个普通 Claude Code 会话，通过 system prompt (MCP instructions) 引导行为：

```
你是 Jarvis Memory Agent。你的唯一职责是分析对话片段，提取值得长期记忆的知识。

规则:
1. 收到 [realtime-extract] 消息 → 分析对话，调用 mission_kb_remember 存储
2. 收到 [deep-analysis] 消息 → 结合已有 KB 深度分析完整会话
3. 不要存储显而易见的事实 (如 "用户让我读文件")
4. 要存储: 架构决策、bug 根因、用户偏好、操作流程、关键发现
5. category 选择: memory (决策/经验)、preference (偏好)、procedure (流程)、project (项目发现)
6. 用最精炼的 summary，detail 存结构化信息
7. 先用 mission_kb_search 检查是否已存在，避免重复
8. 使用最好的模型进行分析，不要使用 haiku
```

这个 prompt 通过 daemon 的 `kb/summary` 或 slot 的 MCP config instructions 注入。

### 防循环

```rust
// RealtimeForwarder 中的过滤逻辑
fn should_forward(&self, msg: &ConversationMessage) -> bool {
    // 1. 不转发 memory agent 自身的会话
    if self.is_memory_session(&msg.session_id) { return false; }

    // 2. 不转发 system/progress 类型
    if msg.role != "user" && msg.role != "assistant" { return false; }

    // 3. 不转发太短的消息 (< 50 字符)
    if msg.content.len() < 50 { return false; }

    true
}
```

---

## Phase 3: 定期深度回顾 (~100 行 Rust)

### 触发条件

复用已有的 Autopilot Scheduler (60s interval loop)，新增逻辑：

```rust
// daemon main.rs — autopilot loop 中新增
async fn check_deep_analysis(state: &AppState) {
    let db = state.mission.db();

    // 查找已完成但未分析的会话
    let unanalyzed = db.get_unanalyzed_conversations().unwrap_or_default();

    for conv in unanalyzed {
        // 只分析已结束 ≥ 5 分钟的会话
        if !is_stale_enough(&conv, Duration::from_secs(300)) { continue; }

        // 获取完整消息历史
        let messages = db.get_conversation_messages(&conv.id, None).unwrap_or_default();
        if messages.is_empty() { continue; }

        // 获取已有 KB 摘要
        let kb_summary = db.kb_summary().unwrap_or_default();
        let kb_recent = db.kb_list(None, 20).unwrap_or_default();

        let prompt = format!(
            "[deep-analysis]\n\
             以下是一个完整的对话会话。请结合已有 KB 知识，分析出更深层的洞察。\n\
             关注: 跨会话的模式、反复出现的主题、隐含的用户偏好、可以抽象成工具/服务的操作。\n\n\
             ## 已有 KB 概览\n{}\n\n\
             ## 近期 KB 条目\n{}\n\n\
             ## 完整会话 ({})\n{}",
            format_kb_summary(&kb_summary),
            format_kb_entries(&kb_recent),
            conv.project.as_deref().unwrap_or("unknown"),
            format_messages(&messages)
        );

        let _ = state.pty.send("slot-memory", &prompt, 300_000).await;
        db.mark_conversation_analyzed(&conv.id).ok();
    }
}
```

### 深度分析产出

Memory Agent 在深度分析模式下会产出更高层次的知识：

| 实时记忆 (Layer 1) | 深度回顾 (Layer 2) |
|---|---|
| "FTS5 delete 需要传实际值" | "MissionD 的 SQLite 使用有3个坑: FTS5 delete、WAL 模式、并发锁" |
| "用户说不要过度工程" | "用户偏好: 最小改动原则 — 只改要求的，不加额外功能" |
| "Jarvis KB Phase 1 完成" | "Jarvis 系列 (KB → Memory → Anywhere) 是当前核心方向，已持续3个会话" |

### 回顾频率

| 条件 | 动作 |
|------|------|
| 会话结束 ≥ 5 分钟 | 触发深度分析 |
| 每 24 小时 | 全量 KB 回顾，合并/整理 |
| 手动触发 | `mission_memory_analyze(sessionId?)` 工具 |

---

## Phase 4: 查询工具 (~50 行)

新增工具让 Memory Agent 和用户都能查询对话历史：

```
mission_conversation_list(status?, limit?)
  → 列出会话: ID, 项目, 消息数, 时间, 分析状态

mission_conversation_get(sessionId, tail?)
  → 获取会话消息 (默认最后 50 条)

mission_conversation_search(query, limit?)
  → FTS 搜索消息内容 (可选: 新增 conversation_messages_fts)
```

---

## 实施顺序

| Phase | 内容 | 改动量 | 依赖 |
|-------|------|--------|------|
| **1** | DB schema + CCTasksWatcher 扩展 + PTY 日志写入 | ~200 行 | 无 |
| **2** | slot-memory 配置 + RealtimeForwarder + 防循环 | ~150 行 | Phase 1 |
| **3** | DeepAnalysisScheduler + 回顾 prompt | ~100 行 | Phase 2 |
| **4** | conversation_list/get/search 工具 | ~50 行 | Phase 1 |

**建议**: Phase 1 + 4 先做 (纯存储 + 查询)，验证日志流通后再做 Phase 2 + 3 (分析)。

---

## 关键设计决策

### Q: 为什么不直接用 JSONL 文件？

| | JSONL 文件 | SQLite |
|--|---|---|
| 查询 | 逐行扫描 | SQL 索引 |
| 跨会话搜索 | 遍历所有文件 | 一条 SQL |
| 增量读取 | 自己维护 offset | 自增 ID |
| 分析状态追踪 | 另建文件 | 同库字段 |
| 数据安全 | 文件可能被 Claude Code 清理 | 独立持久 |

### Q: Memory Agent 用什么模型？

**必须用最好的模型** (Claude Opus/Sonnet)。记忆分析需要理解上下文、识别模式、做抽象 — haiku 做不到。成本通过控制触发频率 (攒批 + 阈值) 来管理。

### Q: 如何防止 Memory Agent 分析自己？

三层过滤:
1. `slot_id == "slot-memory"` → 跳过
2. 会话的 `source == "pty"` 且 slot 是 memory → 跳过
3. 消息内容以 `[realtime-extract]` 或 `[deep-analysis]` 开头 → 跳过

### Q: 实时分析会不会拖慢主对话？

不会。RealtimeForwarder 是 fire-and-forget — 发完就走，不等结果。Memory Agent 在独立 PTY 会话中运行，完全不阻塞主会话。

### Q: 日志存储量有多大？

估算: 每天 ~20 次对话，每次 ~50 条消息，每条 ~2KB
→ 20 × 50 × 2KB = 2MB/天 ≈ 60MB/月
SQLite 轻松处理。

---

## 与现有系统的关系

```
Jarvis KB (已完成)
  ├─ 手动记忆: mission_kb_remember (Agent 主动调用)
  ├─ 被动查询: mission_kb_search (Agent 查询)
  └─ 知识治理: mission_kb_gc

Jarvis Memory System (本文档)
  ├─ 日志存储: JSONL → DB (自动)
  ├─ 实时分析: Memory Agent → KB (自动)
  └─ 深度回顾: 完整会话 + KB → 更深洞察 (定时)

两者关系: Memory System 是 KB 的自动化填充管道
  KB 仍然是最终存储，Memory System 负责"喂"数据
```

## 文件改动清单

| 文件 | 改动 |
|------|------|
| `missiond-core/src/db/mod.rs` | 新增 conversations/messages 表 + CRUD |
| `missiond-core/src/cc_tasks/types.rs` | CCMessageLine 扩展字段 |
| `missiond-core/src/cc_tasks/watcher.rs` | 消息写入 DB |
| `missiond-core/src/types.rs` | Conversation, ConversationMessage 结构体 |
| `missiond-daemon/src/main.rs` | RealtimeForwarder + DeepAnalysis + PTY 日志 + 新工具 handler |
| `missiond-mcp/src/tools.rs` | conversation_list/get/search 工具定义 |
| `~/.xjp-mission/slots.yaml` | slot-memory 配置 |
