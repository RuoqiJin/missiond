# User-Voice Extraction: 独立用户消息记忆提取

## 问题

现有记忆提取把 user 和 assistant 消息混在一起发给 memory agent：

```
[realtime-extract] 2 个会话, 8 条消息
[ts] user: 先别修复，系统性调查问题根因        ← 3 条用户消息 ~50字
[ts] assistant: (500字技术分析...)               ← 5 条 assistant ~2500字
[ts] user: 不要引入外部依赖
[ts] assistant: (800字方案修改...)
[ts] user: 好
[ts] assistant: (1200字实现代码...)
```

用户消息占比 <2%，但信息密度最高。memory agent 从海量 assistant 输出中提取"记忆"，结果：
- 技术细节被提取，用户偏好被忽略
- "好""行"等确认词被跳过（实际是用户认可信号）
- "别...""不要...""先..."等否定/排序词是高价值偏好，但被 assistant 的"好的我改了"覆盖

## 方案

新增独立管线，**只看用户消息**，与现有管线并行互补。

### 三层提取架构

| 层 | 输入 | 频率 | 焦点 | 主要 category |
|---|---|---|---|---|
| **User-Voice** (NEW) | 仅 user 消息 | 每 tick 优先 | 偏好、纠正、决策 | preference |
| Realtime (existing) | user + assistant | 每 tick 次优先 | 技术事实、架构 | memory |
| Deep Analysis | 完整会话 | 会话结束后 | 跨会话模式 | memory + procedure |

### autopilot_tick 执行顺序

```
1. check_slot_context_levels
2. check_user_voice_extraction   ← NEW, 最高优先级
3. check_memory_extraction        (existing)
4. check_deep_analysis
5. sync_claude_md
```

共享 `slot-memory` PTY，每 tick 只有一个能运行。user-voice 优先。

## 改动

### 1. DB: 新增水位线

```sql
ALTER TABLE conversations ADD COLUMN user_voice_forwarded_at TEXT;
```

新查询 `get_pending_user_voice_messages(today)`:
- `role = 'user'` only
- `timestamp > user_voice_forwarded_at`
- `slot_id IS NULL` (仅 CLI 会话)

### 2. MCP 工具: `mission_memory_pending_user`

只返回用户消息 + 专用 prompt header：

```
[user-voice-extract] 3 个会话, 7 条用户消息

你正在分析「用户本人」发出的原始消息，不含 AI 助手的回复。
用户说的每句话都是刻意的。请提取：

1. 偏好/原则 → category: preference
   "不要引入外部依赖" → key: no-external-deps-principle

2. 纠正/否定 → category: preference
   "先别修复，先调查" → key: investigate-before-fix

3. 决策 → category: memory
   "用方案A" → key: chose-plan-a-for-xxx

4. 上下文知识 → category: memory
   "ECS 从本地直连不通" → key: ecs-direct-unreachable

规则：
- 「好」「行」等确认词 = 用户认可了 AI 上一轮方案，也值得记录为决策
- 「别...」「不要...」「先...」= 高价值偏好
- 重复出现的指令 = 极重要，提升 confidence
- 不要存: 纯粹的任务指令("帮我改这个文件")，除非包含偏好信息
```

### 3. Daemon: `check_user_voice_extraction()`

```rust
// AppState 新增
user_voice_extracting: Arc<AtomicBool>,

async fn check_user_voice_extraction(state: &AppState) {
    // 1. 如果 user_voice_extracting 或 memory_extracting，跳过
    // 2. 确认 slot-memory idle
    // 3. 查 DB: get_pending_user_voice_messages(today)
    // 4. 触发: "调用 mission_memory_pending_user 获取用户消息，提取用户偏好和指令。"
    // 5. 设置 user_voice_extracting = true
}
```

### 4. 水位线管理

`update_user_voice_forwarded_at()` — 在 `mission_memory_pending_user` 返回时更新。

两个水位线独立运行：
- `memory_forwarded_at`: 现有 realtime extraction 的进度
- `user_voice_forwarded_at`: user-voice extraction 的进度

## 不改的部分

- 现有 `mission_memory_pending` 不变，继续处理 user+assistant
- 现有 `check_memory_extraction()` 逻辑不变
- Deep analysis 不变
- KB 存储格式不变（`mission_kb_remember` 不改）
- `sync_claude_md()` 不变（preference 条目自动同步到 CLAUDE.md）
