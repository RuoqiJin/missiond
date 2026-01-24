# missiond-rs

Rust 版 XJP Mission Control：用于替代 Node 版 `packages/managed-mcp` + `packages/missiond`。

## 组件

- `mission-mcp`：MCP stdio Server（给 Claude Code 配置用）
- `missiond`：单例守护进程（全局状态、Unix Socket IPC、WebSocket attach + tasks）
- `missiond-attach`：CLI attach 客户端（连接 `ws://localhost:9120/pty/<slotId>`）

## 安装

```bash
cd /path/to/missiond-rs
cargo install --path crates/missiond-mcp --bin mission-mcp
cargo install --path crates/missiond-daemon --bin missiond
cargo install --path crates/missiond-attach --bin missiond-attach
```

确保 `~/.cargo/bin` 在 `PATH` 中。

## 配置

### 1) slots.yaml

默认读取 `~/.xjp-mission/slots.yaml`（可用 `MISSION_SLOTS_CONFIG` 覆盖）：

```yaml
slots:
  - id: slot-coder-1
    role: coder
    description: 编码专家
    cwd: /Users/jinchen/Projects
```

### 2) Claude Code MCP

把 `~/.claude/settings.json` 中的 `mission` MCP server 改成：

```json
{
  "mcpServers": {
    "mission": {
      "command": "mission-mcp",
      "args": [],
      "env": {
        "MISSION_LOG_LEVEL": "warn"
      }
    }
  }
}
```

## 协议兼容性

- MCP tools：工具名、schema、输出字段与 Node 版对齐
- WebSocket：
  - PTY attach：`ws://localhost:9120/pty/<slotId>`
  - Tasks events：`ws://localhost:9120/tasks`

## 环境变量

- `XJP_MISSION_HOME`：默认 `~/.xjp-mission`
- `MISSION_DB_PATH`：默认 `$XJP_MISSION_HOME/mission.db`
- `MISSION_SLOTS_CONFIG`：默认 `$XJP_MISSION_HOME/slots.yaml`
- `MISSION_IPC_SOCKET`：默认 `$XJP_MISSION_HOME/missiond.sock`
- `MISSION_WS_PORT`：默认 `9120`
- `MISSION_LOG_LEVEL`：兼容 Node（`silent|fatal|error|warn|info|debug|trace`）
- `RUST_LOG`：Rust 标准日志过滤（优先级高于 `MISSION_LOG_LEVEL`）
