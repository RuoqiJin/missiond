# MissionD Knowledge Base — "Jarvis Memory"

> Board Task: `5e5d9a70-f539-4666-a3ec-6474b17f37fe`
> Status: 设计完成，待实施
> Date: 2026-02-18

## 背景

MissionD 当前的知识管理是静态的：`servers.yaml` 手写 352 行，Skills 手写 Markdown，CLAUDE.md 手写规则。新用户安装后什么都没有，需要大量手工配置才能让 Claude 认识自己的环境。

目标：Claude 在日常对话中**无声积累**用户的基础设施、偏好、流程知识。安装即可用，零配置。

## 核心原则

1. **无声学习** — Claude 发现新信息时直接记录，不询问用户
2. **多维知识** — category 开放式（infra/project/preference/memory/procedure/credential...），不预定义枚举
3. **自治** — 不依赖 CLAUDE.md 或 Skills，MCP 工具描述即教学
4. **渐进** — 先发散围猎，再收敛整理，再发散

## 数据库 Schema

在现有 `mission.db` 中新增：

```sql
CREATE TABLE knowledge (
    id TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    key TEXT NOT NULL,
    summary TEXT NOT NULL,
    detail TEXT,                          -- JSON
    source TEXT DEFAULT 'conversation',   -- 'conversation'|'discovery'|'import'
    confidence REAL DEFAULT 1.0,
    access_count INTEGER DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_accessed_at TEXT,
    UNIQUE(category, key)
);

CREATE TABLE credentials (
    id TEXT PRIMARY KEY,
    knowledge_id TEXT REFERENCES knowledge(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    value_encrypted TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE VIRTUAL TABLE knowledge_fts USING fts5(
    key, summary, detail, category,
    content='knowledge', content_rowid='rowid'
);
```

不单独建 relations 表。关系记为 `category='relation'` 的条目，`detail` 存 `{"from": "backend", "to": "gcp", "type": "runs_on"}`。

## MCP 工具

### 写入（Claude 主动调用）

```
mission_kb_remember(category, key, summary, detail?)
```

**工具描述（关键 — 这就是"教学"）：**

> Record knowledge discovered during conversation. Call PROACTIVELY when you learn:
> server IPs, user preferences, project structures, procedures, decisions.
> Do NOT ask permission. Just record. If the key already exists, update it.
> Examples: user mentions a server → infra; user corrects your approach → preference;
> you SSH into a machine → discovery; deployment succeeds → procedure.

```
mission_kb_forget(key)
```

### 查询

```
mission_kb_search(query, category?)   ← FTS 全文搜索
mission_kb_get(key)                   ← 精确查询
mission_kb_list(category?)            ← 列举
```

**mission_kb_search 工具描述：**

> Search the knowledge base BEFORE guessing or asking the user.
> If the user mentions a server, project, or concept, search here first.
> This is your long-term memory across all conversations.

### 导入

```
mission_kb_import(format, path?)      ← 'servers_yaml'|'json'
```

## 自治三层架构

### Layer 1: MCP 工具描述即教学

每个 MCP 工具的 description 字段承担"CLAUDE.md 规则"的角色。新用户安装 MissionD 后，Claude 看到工具列表就知道：
- 发现新信息 → 调 `mission_kb_remember`
- 需要查信息 → 先调 `mission_kb_search`
- 不需要任何 CLAUDE.md 配置

### Layer 2: 启动上下文自注入

MissionD MCP server 通过 prompts/resources 向每个新会话注入：

```
[MissionD] KB: {N} infra, {N} projects, {N} preferences.
Use mission_kb_search before guessing. Use mission_kb_remember when learning.
```

实现：daemon 的 MCP handler 在 `list_prompts` 或 `get_prompt` 中返回 KB 摘要。

### Layer 3: 知识分级

| 知识类型 | 归属 | 理由 |
|----------|------|------|
| 结构化事实（IP/规格/偏好/凭据） | **KB** | 一条记录足够 |
| 操作记忆（决策/时间线） | **KB** | memory 条目 |
| 部署流程（多步骤、条件分支） | **Skills** | 长文档，KB 装不下 |
| 排查手册（故障树） | **Skills** | 需要完整阅读 |
| API 参考 | **Skills** | 结构化参考文档 |

**规则：能用 summary + detail JSON 表达的 → KB。需要多段 Markdown 的 → Skills。**

## 知识维度示例

| category | 触发时机 | key 示例 | summary 示例 |
|----------|---------|----------|-------------|
| infra | SSH/连接服务器 | privatecloud | 构建机 192.168.31.174, Ubuntu, 128GB |
| project | 操作代码仓库 | missiond | Rust 多实例编排, /Users/jinchen/Projects/missiond |
| preference | 用户纠正行为 | commit-style | 中文 commit message, 不用 emoji |
| memory | 对话中的关键决策 | wss-migration-2026-02 | GFW 封锁 9876，改 WSS+443 via Caddy |
| procedure | 成功执行操作后 | deploy-backend | 私有云构建→GHCR→GCP拉取 |
| credential | 用户提供凭据 | — | 写入 credentials 表，knowledge 只存引用 |
| relation | 发现依赖关系 | backend-runs-on-gcp | {from: "backend", to: "gcp", type: "runs_on"} |

## 实施阶段

### Phase 1: 基础 KB (~300 行 Rust)

1. `db/mod.rs` — 新增 knowledge + credentials + FTS 表的建表 SQL
2. `db/knowledge.rs` — CRUD 操作：remember (upsert), get, search (FTS), list, forget
3. `types.rs` — KnowledgeEntry, Credential 结构体
4. `missiond-mcp/tools.rs` — 新增 6 个工具定义
5. `missiond-daemon/main.rs` — 新增工具 handler
6. `mission_kb_import` — 解析 servers.yaml → knowledge 条目

### Phase 2: 无声学习 (~100 行)

1. 优化所有 KB 工具的 description 文案（行为注入）
2. `skill.rs` — `build_context()` 扩展，同时搜索 Skills + KB
3. MCP prompts — 返回 KB 摘要作为启动上下文

### Phase 3: 主动发现 (~200 行)

1. 新工具 `mission_kb_discover(host)` — SSH 后自动探测 (uname/free/df/docker ps)
2. 探测结果结构化后调用 remember
3. 工具描述引导 Claude 在 SSH 后主动调用

### Phase 4: 知识治理（迭代）

1. 重复条目合并（same category + similar key）
2. 过时淘汰（access_count=0 + last_accessed 超过 N 天 → 标记 stale）
3. 矛盾检测（同 key 不同 detail → 提示用户确认）

## 现有数据迁移

```
servers.yaml (352行, 25 条 server)
  → mission_kb_import('servers_yaml')
  → 25 条 category='infra' 知识条目
  → credentials 单独提取
  → InfraConfig 改为从 KB 读取（mission_infra_* 工具保持兼容）

CLAUDE.md 凭据表
  → 渐进迁入 KB credentials 表
  → CLAUDE.md 瘦身为纯 Claude Code 配置

Skills
  → 服务器连接信息部分 → 已在 KB（import 完成）
  → 流程文档 → 保留为 Skills
  → Skills 变成可选增强，不是必需依赖
```

## 新用户体验

```
Day 0:  安装 MissionD + 配置 MCP
        KB 为空，但 Claude 已知道如何使用 remember/search

Day 0+: 第一次对话
        用户: "连一下我的服务器 ssh root@10.0.0.1"
        Claude: (连接成功后静默调用 mission_kb_remember)
          → infra / server-10-0-0-1 / "Linux server at 10.0.0.1"

Day 3:  新会话
        用户: "部署到服务器"
        Claude: (调用 mission_kb_search "server") → 找到 → 直接操作

Day 7:  KB 已有: 3 台服务器, 2 个项目, 5 条偏好, 若干操作记忆

Day 30: 完整基础设施拓扑 + 部署流程 + 个人偏好
        Claude: 真正的 Jarvis
```

## 与 Claude Code Memory 的关系

互补，不替代：
- Claude Code memory → 短期、会话级偏好
- MissionD KB → 长期、结构化事实，跨会话持久

## 关联任务

- 隧道改 WSS+443（进行中）— 修复后可验证 KB 的 memory 记录
- 隧道抗 GFW 加固 — KB 可记录决策过程
- 搬瓦工中继迁移 — KB 自动记录新中继配置
