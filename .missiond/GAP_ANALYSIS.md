# MissionD Reforge — Gap Analysis Report
## Phase 1: High-Dimensional Survey (2026-04-02)

---

## Coverage Summary

| Pattern | Jarvis Name | MissionD Coverage | Components | Certainty |
|---------|-------------|-------------------|------------|-----------|
| P1 | crud-gateway | 45 tables, 12 DB modules | Full | 90% |
| P2 | event-listener | 5 workers | Full | 85% |
| P3 | cron-worker | 13 workers | Full | 80% |
| P4 | state-machine | 7 state enums | Full | 75% |
| P5 | mcp-tool | 60+ tools, 4 domains | Full | 80% |
| P6 | bootstrap | daemon main.rs init | Full | 90% |
| **GAP** | — | **10 uncovered subsystems** | — | — |

**Estimated Pattern Coverage: ~55%** (by line count)

The existing 6 patterns cover the "outer shell" (DB/Worker scaffolding/MCP dispatch/DI wiring)
but miss the "inner brain" — the LLM routing, decision cascades, context assembly,
semantic parsing, and orchestration logic that make MissionD unique.

---

## The 10 Gaps — New Molds Needed

### GAP 1: `llm-gateway` (HIGH PRIORITY)
**Location:** `missiond-daemon/src/llm/` (12 files)
**What it is:** Queue-driven multi-provider LLM dispatch with priority channels, rate limiting, backpressure, and retry.
**Why existing patterns fail:** Not CRUD (no DB tables). Not a Worker (it IS what workers call). Not a StateMachine (queue states are internal). It is a **queue-with-channels** pattern unique to LLM orchestration.
**Sub-patterns needed:**
- `queue-channel` — MPSC-based priority queue per use-case (interactive/embedding/translation/briefing)
- `rate-limiter` — 429 backoff, quota tracking per provider
- `provider-router` — Route to Gemini HTTP / Gemini CLI / Sonnet slot / MiniMax HTTP
- `prompt-builder` — System prompt assembly from templates

**Estimated codegen yield:** 40% (queue scaffolding + rate limit boilerplate; provider-specific logic stays hand-written)

---

### GAP 2: `slot-orchestrator` (HIGH PRIORITY)
**Location:** `missiond-daemon/src/slot_orchestrator/` (5 files)
**What it is:** Process lifecycle controller for multi-engine AI sessions (Claude Code, Gemini CLI, Codex).
**Why existing patterns fail:** Combines StateMachine + PTY management + health monitoring + restart policies. More than any single pattern — it is a **supervisor** managing heterogeneous child processes.
**Sub-patterns needed:**
- `process-lifecycle` — spawn/monitor/restart/kill agent processes
- `engine-adapter` — per-engine controller interface
- `context-monitor` — track context window usage, trigger compaction/restart
- `task-dispatch` — route tasks to available slots

**Estimated codegen yield:** 35% (lifecycle scaffolding; engine-specific logic stays hand-written)

---

### GAP 3: `tick-engine` (MEDIUM PRIORITY)
**Location:** `missiond-daemon/src/engine/intent_engine/` (4 files)
**What it is:** Composite orchestrator tick loop that sequences multiple sub-engines in order: memory_scheduler → extraction → task_dispatch → decision_engine → flow_engine → supervision.
**Why existing patterns fail:** cron-worker handles single-tick workers. This is a **pipeline of engines** where each tick runs a chain of sub-ticks.
**Sub-patterns needed:**
- `tick-pipeline` — ordered sequence of sub-engine ticks with error isolation
- `flow-lifecycle` — Board task progression through EngineeringPhase
- `memory-trigger` — condition-based memory extraction scheduling
- `workflow-exec` — skill-driven workflow step execution

**Estimated codegen yield:** 30% (pipeline scaffold + phase transition logic; engine internals stay hand-written)

---

### GAP 4: `learning-engine` (MEDIUM PRIORITY)
**Location:** `missiond-daemon/src/engine/learning_engine/` (7 files)
**What it is:** Multi-strategy decision routing and knowledge extraction. Routes questions through KB → LLM → human cascade. Mines patterns from conversation history.
**Why existing patterns fail:** Not a simple worker or state machine. It is a **decision cascade** with fallback routing and extraction pipelines.
**Sub-patterns needed:**
- `decision-cascade` — KB lookup → Gemini → decision slot → human escalation
- `extraction-fsm` — ExtractionPhase state machine (already partially P4)
- `intent-analysis` — user intent extraction from conversation turns
- `pattern-mining` — historical session analysis

**Estimated codegen yield:** 25% (cascade routing scaffolding; analysis logic stays hand-written)

---

### GAP 5: `context-pipeline` (MEDIUM PRIORITY)
**Location:** `missiond-daemon/src/context/` (5 files)
**What it is:** LLM prompt builder with token budget constraints. Assembles context from KB, skills, history, and topology within a budget.
**Why existing patterns fail:** Not CRUD, not Worker, not Tool. It is a **builder/pipeline** with prioritized source assembly and truncation.
**Sub-patterns needed:**
- `budget-allocator` — token budget partitioning across context sources
- `source-ranker` — prioritize KB/skill/history by relevance score
- `claude-md-sync` — preferences sync to ~/.claude/CLAUDE.md
- `topology-infer` — cross-slot relationship inference

**Estimated codegen yield:** 20% (mostly hand-written logic; some struct scaffolding)

---

### GAP 6: `semantic-parser` (MEDIUM PRIORITY)
**Location:** `missiond-core/src/semantic/` (7 files)
**What it is:** Terminal output pattern matching and state inference. Parses raw PTY screen lines into structured states using regex fingerprints.
**Why existing patterns fail:** Not a state-machine (it FEEDS state machines). It is a **parser/recognizer** pattern with regex pattern databases.
**Sub-patterns needed:**
- `screen-parser` — line-by-line terminal output analysis
- `fingerprint-db` — regex pattern database for state detection
- `confirm-parser` — permission dialog structure extraction
- `tool-recognizer` — tool invocation output parsing
- `multi-engine` — per-engine parser variants (Claude/Gemini)

**Estimated codegen yield:** 15% (regex patterns are inherently hand-written; some dispatch scaffolding)

---

### GAP 7: `event-bus` (LOW PRIORITY)
**Location:** `missiond-daemon/src/event_bus.rs`, `event_router.rs`, `events_sync.rs`
**What it is:** Publish-subscribe event infrastructure with persistent timeline. Includes event routing, persistence, and multi-consumer fan-out.
**Why existing patterns fail:** Not a Worker or Tool. It is the **nervous system** — infrastructure that other patterns depend on.
**Sub-patterns needed:**
- `broadcast-hub` — tokio broadcast with DaemonEvent enum
- `event-router` — registered handler dispatch
- `timeline-writer` — persist events to system_timeline with FTS
- `frontend-bridge` — relay to WebSocket

**Estimated codegen yield:** 50% (channel setup + event enum + router dispatch is highly mechanical)

---

### GAP 8: `worker-registry` (LOW PRIORITY)
**Location:** `missiond-daemon/src/workers/registry.rs`, `control_tree.rs`
**What it is:** Supervisor pattern for worker lifecycle management. BackgroundWorker trait + registry + hierarchical pause/resume.
**Why existing patterns fail:** Not a worker — it is the **meta-pattern** that manages all workers.
**Sub-patterns needed:**
- `trait-contract` — BackgroundWorker trait definition
- `registry` — worker tracking, health checks
- `control-tree` — hierarchical pause/resume: provider → worker → sub
- `graceful-shutdown` — ordered shutdown with drain

**Estimated codegen yield:** 60% (trait + registry + control dispatch is highly mechanical)

---

### GAP 9: `ipc-protocol` (LOW PRIORITY)
**Location:** `missiond-mcp/src/server.rs`, `protocol.rs`
**What it is:** JSON-RPC 2.0 over stdio transport layer. The mcp-tool pattern covers tool DEFINITIONS but not the protocol/dispatch layer.
**Sub-patterns needed:**
- `jsonrpc-server` — JSON-RPC 2.0 request/response/notification
- `stdio-transport` — stdin/stdout message framing
- `tool-dispatch` — route tool_name → handler function
- `ipc-bridge` — Unix socket / TCP bridge to daemon

**Estimated codegen yield:** 70% (protocol layer is highly standardized; very mechanical)

---

### GAP 10: `ws-bridge` (LOW PRIORITY)
**Location:** `missiond-core/src/ws/` (3 files)
**What it is:** WebSocket server for realtime frontend communication. PTY screenshot distribution, trace relay, state updates.
**Sub-patterns needed:**
- `ws-server` — tokio-tungstenite acceptor
- `screenshot-broker` — distribute PTY frames to subscribers
- `trace-relay` — forward request traces to UI

**Estimated codegen yield:** 55% (WebSocket server setup is mechanical; broker logic is hand-written)

---

## Priority Matrix

```
                    HIGH CODEGEN YIELD
                         ↑
          ipc-protocol(9)│  event-bus(7)
     worker-registry(8)  │  ws-bridge(10)
                         │
LOW ─────────────────────┼──────────────────── HIGH
PRIORITY                 │                   PRIORITY
                         │
    semantic-parser(6)   │  tick-engine(3)
    context-pipeline(5)  │  learning-engine(4)
    learning-engine(4)   │  slot-orchestrator(2)
                         │  llm-gateway(1)
                         ↓
                    LOW CODEGEN YIELD
```

**Recommended Phase 2 attack order:**
1. `event-bus` + `worker-registry` + `ipc-protocol` — HIGH yield, LOW risk, foundational
2. `llm-gateway` — HIGH priority, MEDIUM yield, core to MissionD's value
3. `slot-orchestrator` — HIGH priority, MEDIUM yield, core to multi-agent
4. `tick-engine` + `learning-engine` — MEDIUM priority, compose existing patterns
5. `context-pipeline` + `semantic-parser` — mostly hand-written, low codegen ROI

---

## Statistics

| Metric | Count |
|--------|-------|
| Crates | 7 |
| Source files | 130+ |
| DB tables | 45 |
| MCP tools | 60+ |
| Workers | 18 |
| State machines | 7 |
| Async channels | 10+ |
| Existing patterns applied | 6 (P1-P6) |
| New patterns needed | 10 (GAP 1-10) |
| Estimated total lines | ~25,000 |
| Coverable by existing molds | ~55% |
| Coverable after new molds | ~75% |
| Pure algorithm (uncoverable) | ~25% |
