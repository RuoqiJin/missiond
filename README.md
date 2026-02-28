# missiond

**Multi-agent orchestration for Claude Code** — Spawn, control, and coordinate multiple Claude Code instances from a single session via MCP.

[![Crates.io](https://img.shields.io/crates/v/missiond-core.svg)](https://crates.io/crates/missiond-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

## What is missiond?

missiond is a daemon that turns a single Claude Code session into a multi-agent system. The main Claude Code instance acts as an orchestrator, dispatching tasks to background Claude Code agents running in managed pseudo-terminals (PTY slots). Each slot is a fully independent Claude Code session with its own working directory, role, and permissions.

## Features

### Core
- **PTY Sessions** — Spawn Claude Code in pseudo-terminals with full terminal emulation (via Alacritty terminal)
- **Semantic Parsing** — Real-time state detection (Idle/Thinking/Responding/Confirming/Error), tool output extraction, status bar parsing, confirm dialog parsing
- **MCP Integration** — 40+ tools exposed through Model Context Protocol for agent control
- **Task Queue** — Async task submission (`mission_submit`) and sync consultation (`mission_ask`) with automatic slot routing
- **Permission System** — Role-based tool permissions (allow/confirm/deny) with glob pattern matching
- **Cross-Platform** — macOS, Linux, Windows; Unix domain sockets or TCP loopback IPC

### Agent Lifecycle
- **Auto-Restart** — Automatically restarts PTY slots when context window drops below 10%
- **Stuck Detection** — Monitors JSONL activity to detect and recover stuck agents
- **Autonomous Workflow** — Agents can work autonomously with safety guardrails and reporting back to the orchestrator

### Knowledge & Memory
- **Knowledge Base (KB)** — SQLite-backed knowledge store with FTS5 full-text search, auto-GC, and tiered categories
- **Conversation Logging** — Records all Claude Code conversations (including tool_use/tool_result and thinking blocks) into SQLite
- **Memory Extraction** — Real-time and deep analysis pipelines that extract insights from agent conversations
- **KB Injection** — Automatically injects relevant knowledge into agent context via MCP server instructions and UserPromptSubmit hooks

### Monitoring & Dashboard
- **WebSocket API** — Real-time PTY attach, task events, and session monitoring
- **Board UI** — Next.js dashboard with conversation viewer, slot status, and task management
- **CC Tasks Watcher** — Cross-session task monitoring by watching Claude Code's JSONL session files
- **PTY Screenshots** — Render terminal state as PNG images for visual debugging

### Infrastructure Tools
- **Mission Board** — Task/kanban board with notes, hidden tasks, and skip status
- **Question Queue** — Agents can post questions for human review instead of blocking
- **Slot History** — Track task assignment history per slot
- **AI Router** — Route LLM requests through configurable model backends (for KB analysis)
- **Reachability Check** — Probe configured server health endpoints
- **OS Diagnostics** — System resource monitoring (CPU, memory, disk)

## Architecture

```
┌─────────────────┐     MCP      ┌──────────────┐
│  Claude Code    │◄────────────►│  mission-mcp │
│  (Orchestrator) │              └──────┬───────┘
└─────────────────┘                     │ IPC (JSON-RPC)
                                        ▼
                               ┌──────────────────┐
                               │    missiond       │
                               │    (Daemon)       │
                               ├──────────────────┤
                               │ • Task Queue      │
                               │ • PTY Manager     │
                               │ • Permission Mgr  │
                               │ • Knowledge Base  │
                               │ • Memory Pipeline │
                               │ • WebSocket API   │
                               │ • CC Tasks Watcher│
                               └────────┬─────────┘
                                        │ PTY
              ┌─────────────────────────┼─────────────────────────┐
              ▼                         ▼                         ▼
        ┌───────────┐            ┌───────────┐            ┌───────────┐
        │  slot-1   │            │  slot-2   │            │  slot-N   │
        │  Claude   │            │  Claude   │            │  Claude   │
        │  (coder)  │            │ (research)│            │ (memory)  │
        └───────────┘            └───────────┘            └───────────┘
```

## Installation

### From Cargo (Rust)

```bash
cargo install missiond-mcp --bin mission-mcp
cargo install missiond-daemon --bin missiond
cargo install missiond-attach --bin missiond-attach
```

### From npm (Node.js)

```bash
npm install @missiond/core
```

Pre-built binaries for:
- macOS (ARM64, x64)
- Linux (x64 glibc, x64 musl)
- Windows (x64)

## Quick Start

### 1. Configure MCP

Add to `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "mission": {
      "command": "mission-mcp",
      "args": [],
      "env": { "MISSION_LOG_LEVEL": "warn" }
    }
  }
}
```

### 2. Configure Slots

Create `~/.missiond/slots.yaml`:

```yaml
slots:
  - id: coder-1
    role: coder
    description: "Coding specialist"
    cwd: /path/to/projects

  - id: researcher-1
    role: researcher
    description: "Research and documentation"
    cwd: /path/to/docs
```

### 3. Use from Claude Code

```
User: "Spawn an agent to refactor the auth module"

Claude: I'll spawn a coding agent for that task.
[Uses mission_pty_spawn tool]
[Uses mission_pty_send with the refactoring instructions]
```

## MCP Tools

### Task Operations

| Tool | Description |
|------|-------------|
| `mission_submit` | Submit async task to agent (with optional slotId targeting) |
| `mission_task` | Batch query submit task status |
| `mission_ask` | Sync expert consultation |
| `mission_status` | Query task/agent status |
| `mission_cancel` | Cancel running task |
| `mission_spawn` | Spawn a new agent slot |
| `mission_kill` | Kill an agent slot |
| `mission_restart` | Restart an agent slot |
| `mission_agents` | List all agents and their states |
| `mission_slots` | Get slot configuration |
| `mission_inbox` | Check inbox for messages |

### PTY Control

| Tool | Description |
|------|-------------|
| `mission_pty_spawn` | Start PTY session |
| `mission_pty_send` | Send message, wait for response |
| `mission_pty_screen` | Get current terminal screen |
| `mission_pty_screenshot` | Render terminal as PNG image |
| `mission_pty_confirm` | Handle tool confirmation dialogs |
| `mission_pty_interrupt` | Send Ctrl+C |
| `mission_pty_kill` | Close PTY session |
| `mission_pty_status` | Get session state and metadata |
| `mission_pty_history` | Get session message history |
| `mission_pty_logs` | Get PTY log file contents |

### Knowledge Base

| Tool | Description |
|------|-------------|
| `mission_kb_remember` | Store a knowledge entry |
| `mission_kb_forget` | Delete a knowledge entry |
| `mission_kb_search` | Full-text search across KB |
| `mission_kb_get` | Get entry by ID |
| `mission_kb_list` | List entries with filtering |
| `mission_kb_import` | Bulk import entries |
| `mission_kb_discover` | Discover knowledge from infrastructure |
| `mission_kb_gc` | Garbage collect stale entries |
| `mission_kb_analyze` | AI-powered KB analysis (via configurable LLM backend) |

### Board & Tasks

| Tool | Description |
|------|-------------|
| `mission_board_list` | List board tasks |
| `mission_board_create` | Create a board task |
| `mission_board_update` | Update a board task |
| `mission_board_get` | Get task details |
| `mission_board_delete` | Delete a board task |
| `mission_board_toggle` | Toggle task completion |
| `mission_board_note_add` | Add a note to a task |
| `mission_board_summary` | Get board summary |

### Monitoring

| Tool | Description |
|------|-------------|
| `mission_cc_sessions` | List all Claude Code sessions |
| `mission_cc_tasks` | Get tasks for a session |
| `mission_cc_overview` | Global task statistics |
| `mission_cc_in_progress` | All in-progress tasks |
| `mission_cc_trigger_swarm` | Trigger swarm coordination |
| `mission_slot_history` | Task assignment history per slot |

### Conversations & Memory

| Tool | Description |
|------|-------------|
| `mission_conversation_list` | List recorded conversations |
| `mission_conversation_get` | Get conversation messages |
| `mission_conversation_search` | Search conversation content |
| `mission_memory_pending` | Check pending memory queue |
| `mission_memory_pause` | Pause memory extraction (with auto-resume TTL) |
| `mission_memory_done` | Mark memory processing complete |
| `mission_token_stats` | Token consumption statistics |

### Permissions

| Tool | Description |
|------|-------------|
| `mission_permission_get` | Get current permission policy |
| `mission_permission_set_role` | Set slot role permissions |
| `mission_permission_set_slot` | Set per-slot overrides |
| `mission_permission_add_auto_allow` | Add auto-allow rules |
| `mission_permission_reload` | Reload permission config |

### Infrastructure

| Tool | Description |
|------|-------------|
| `mission_infra_list` | List configured infrastructure |
| `mission_infra_get` | Get infrastructure details |
| `mission_health` | Daemon health check |
| `mission_reachability` | Probe server health endpoints |
| `mission_os_diagnose` | System resource diagnostics |
| `mission_skill_list` | List available skills |
| `mission_skill_search` | Search skills by keyword |
| `mission_context_build` | Build context from skills |

### Question Queue

| Tool | Description |
|------|-------------|
| `mission_question_create` | Agent posts a question for human |
| `mission_question_list` | List pending questions |
| `mission_question_get` | Get question details |
| `mission_question_answer` | Human answers a question |
| `mission_question_dismiss` | Dismiss a question |

### AI Router

| Tool | Description |
|------|-------------|
| `mission_router_chat` | Route chat completion through configured LLM backend |

### Jarvis (Logging)

| Tool | Description |
|------|-------------|
| `mission_jarvis_logs` | Query log center entries |
| `mission_jarvis_trace` | Trace request by ID |

## Semantic Terminal Parsing

The daemon includes sophisticated terminal parsing for Claude Code's TUI:

- **State Machine** — Tracks Idle, Thinking, Responding, ToolRunning, Confirming, Error, SlashMenu states with debounce
- **Confirm Dialog Parsing** — Extracts tool name, parameters, file paths from permission prompts
- **Status Bar Parsing** — Reads spinner state and status text from bottom lines
- **Tool Output Extraction** — Parses both boxed (`───`) and inline tool outputs
- **Title Parsing** — Monitors terminal title changes for session info

```rust
pub enum SessionEvent {
    StateChange { new_state, prev_state },
    ConfirmRequired { prompt, info },
    StatusUpdate(ClaudeCodeStatus),
    ToolOutput(ClaudeCodeToolOutput),
    TitleChange(ClaudeCodeTitle),
    TextComplete(String),
}
```

## WebSocket API

### PTY Attach

```
ws://localhost:9120/pty/<slot-id>
```

Connect to watch or interact with a PTY session in real-time. Receives terminal cell data for rendering.

### Tasks Events

```
ws://localhost:9120/tasks
```

Subscribe to task lifecycle events:
- `cc_tasks_changed` — Tasks updated
- `cc_task_started` / `cc_task_completed`
- `cc_session_active` / `cc_session_inactive`

## Node.js Client

```typescript
import { MissionControl } from '@missiond/core';

const mission = new MissionControl();
await mission.connect();  // Auto-starts daemon

// Spawn PTY session
const pty = await mission.pty.spawn('slot-1', 'claude');
pty.on('state', (state) => console.log('State:', state));
pty.on('confirm', (info) => console.log('Confirm:', info));

// Send message and wait for response
const response = await pty.send('Explain this codebase');
console.log(response);

await pty.kill();
mission.close();
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MISSIOND_HOME` | `~/.missiond` | Config directory |
| `MISSION_DB_PATH` | `$MISSIOND_HOME/mission.db` | SQLite database |
| `MISSION_SLOTS_CONFIG` | `$MISSIOND_HOME/slots.yaml` | Slot definitions |
| `MISSION_IPC_ENDPOINT` | Unix socket / TCP (Windows) | IPC endpoint |
| `MISSION_WS_PORT` | `9120` | WebSocket port |
| `MISSION_LOG_LEVEL` | `warn` | Log level |

### Cross-Platform IPC

- **Unix** (macOS/Linux): Unix domain sockets (`~/.missiond/missiond.sock`)
- **Windows**: TCP loopback (`127.0.0.1:port`)

### Permissions

Configure tool permissions in `~/.missiond/permissions.yaml`:

```yaml
roles:
  coder:
    allow:
      - "Bash(*)"
      - "Read(*)"
      - "Write(*)"
    confirm:
      - "Edit(*)"
    deny:
      - "Bash(rm -rf*)"

  researcher:
    allow:
      - "Read(*)"
      - "WebSearch(*)"
    deny:
      - "Bash(*)"
      - "Write(*)"
```

### AI Router (Optional)

To use `mission_kb_analyze` and `mission_router_chat`, configure an LLM backend in `~/.missiond/credentials.json`:

```json
{
  "auth_url": "https://your-llm-api-endpoint.com",
  "api_key": "your-api-key"
}
```

### Secret Resolution (Optional)

Slot environment variables support `${secret:path}` syntax that resolves secrets at spawn time via a configurable command.

## Crates

| Crate | Description |
|-------|-------------|
| `missiond-core` | Core library: PTY management, semantic terminal parsing, task queue, knowledge base, IPC |
| `missiond-mcp` | MCP server binary (`mission-mcp`) — tool definitions and JSON-RPC protocol |
| `missiond-daemon` | Daemon binary (`missiond`) — main process with all subsystems |
| `missiond-runner` | Claude CLI wrapper for slot process management |
| `missiond-attach` | PTY attach CLI (`missiond-attach`) — connect to running sessions |
| `semantic-terminal-napi` | Node.js N-API bindings for semantic terminal parsing |

## Packages (npm)

| Package | Description |
|---------|-------------|
| `@missiond/core` | Node.js client library with auto-daemon management |
| `@missiond/board` | Next.js dashboard for monitoring and management |
| `@missiond/semantic-terminal` | Node.js semantic terminal parser |

## Development

```bash
# Build all crates
cargo build

# Run daemon
cargo run --bin missiond

# Run MCP server
cargo run --bin mission-mcp

# Build Node.js packages
cd packages/node-client && pnpm build
cd packages/board && pnpm build
```

## License

MIT License — see [LICENSE](LICENSE) for details.
