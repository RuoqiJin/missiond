;; ══════════════════════════════════════════════════════
;; MissionD — Implementation Detail (Full System)
;; Generated: 2026-04-07 | Forge Deep Cartography v3
;; Updated: 2026-04-15 (4 commits: flow-v2 committed + xjpcode briefing + forge tools + project context)
;; ══════════════════════════════════════════════════════

(intent missiond
  (granularity L3-Implementation)
  (survey-hash "e17db52-50a5296-76900d1-e18d0bf-5671c95-ac96b3c-eae9bbd-c15f363-5ec3517-84ac1a6-0adbb18-3c10d21-dc45dcb-21a3b63-65c8b59-1ea1838-081b4f9-a9517ec-20813d5-5a5f805-ec269d7-79a877f-ad889d1-48a9834-b3f04a6-ce3f3ec-d8f47de-d75699e-9684b50-43c80f4-49bd316-8e5efaf-34167db-8438a7d")
  (survey-date "2026-04-15T00:00:00Z")
  (last-updated "2026-04-15 — 4 commits since 75949d9→8438a7d: (1) 49bd316 Flow Engine v2 committed + ParallelSlotTasks(JoinSet+Semaphore fan-out, GatherStrategy 3-variant, round-robin slot assignment) — 从 working-tree 转正; (2) 8e5efaf xjpcode_briefing_worker: 60s interval Local worker 写 ~/.xjpcode/xjpcode.md(running tasks+failed+incidents+projects), xjpcode 启动时读此文件注入 system prompt; (3) 34167db mission_forge_build+mission_forge_lint MCP tools: shell out forge CLI, ProjectRegistry longest-prefix 项目解析, tokio::process::Command; (4) 8438a7d Project Context Aggregator: mission_project 新增 context(intent/github/conv-stats/memories/kb/slots 聚合) + memories(list/read Claude Code 项目记忆文件) actions, 新模块 project_memory.rs(YAML frontmatter parsing + path traversal protection), DB 新增 3 ProjectStore 方法(conversation_stats_by_project, recent_conversations_by_project, kb_stats_by_project).")

  ;; ══════════════════════════════════════════════════════
  ;; DESIGN CONSTRAINTS
  ;; ══════════════════════════════════════════════════════
  (design-constraints
    (crate-boundary
      (missiond-shared   "CliEngine enum + default_mission_home — re-exports CliEngine from semantic-terminal")
      ;; ⚠ missiond-semantic REMOVED (commit 5a5f805) — now external crate:
      ;;   workspace dep: semantic-terminal = { path = "../semantic-terminal/..." }
      ;;   (github.com/ruoqijin/semantic-terminal, independent open-source crate)
      ;;   pure_parsing/ (Forge GenGap) moved to missiond-core/src/semantic_parsing/
      (semantic-terminal "[EXTERNAL] semantic terminal parser: fingerprints, state, confirm, title, tool, status, patterns")
      (missiond-pty      "PTY session management: PTYManager, PTYSession, IncrementalExtractor, screenshot")
      (missiond-core     "types, DB traits, IPC, embedding, context — depends on pty + semantic-terminal(external) + shared")
      (missiond-daemon   "business logic, handlers, engines, workers (sonnet/codex/gemini/local), LLM gateways")
      (missiond-mcp      "MCP JSON-RPC stdio server, tool schema + dispatch")
      (missiond-attach   "CLI PTY attach utility")
      (missiond-runner   "Claude CLI process wrapper")
      (semantic-terminal-napi "Node N-API bindings for semantic parser")
      (skill-store       "standalone skill marketplace microservice"))

    (communication
      (internal  "DaemonEvent broadcast + per-worker MPSC channels")
      (external  "IPC Unix-socket/TCP between mcp↔daemon")
      (frontend  "WebSocket tokio-tungstenite for realtime board UI"))

    (db-access
      (backend "PostgreSQL (sqlx) + SQLite (rusqlite) dual-backend via MissionDB trait")
      (gateway "missiond-core/src/db/ is sole DB gateway — 100+ query functions")
      (generated "crates/missiond-core/src/db/gen/*.rs from Forge codegen, custom hand-written in sibling db/ modules"))

    (authority
      (pty-state         "semantic parser pipeline is authority")
      (task-lifecycle    "autopilot tick loop is authority")
      (slot-lifecycle    "SlotManager is authority")
      (knowledge-truth   "PostgreSQL KB tables are authority")
      (permission        "PermissionPolicy + LearnedPermissions is authority")))

  ;; ══════════════════════════════════════════════════════
  ;; UPSTREAM DEPENDENCIES
  ;; ══════════════════════════════════════════════════════
  (upstream-dependency
    (service "forge"
      :consumes ("forge-build-cli" "generated-rs-output" "generation-gap-pattern")
      :breaks-if ("codegen-pattern-change" "ir-whitelist-change")))

  (upstream-dependency
    (service "mechanic"
      :consumes ("mechanic-repair-cli" "governance-mode-repair")
      :breaks-if ("repair-cli-interface-change" "governance-mode-change")))

  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 1: DATABASE SCHEMA — CORE BUSINESS TABLES
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: db-core ──
  (module-link "intent-pillar-db-core.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 2: DATABASE SCHEMA — OBSERVABILITY
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: db-observability ──
  (module-link "intent-pillar-db-observability.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 3: DATABASE SCHEMA — PIPELINE & CODE INTEL
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: db-pipeline ──
  (module-link "intent-pillar-db-pipeline.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 4: DATABASE SCHEMA — AGENTS & SKILLS
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: db-agents ──
  (module-link "intent-pillar-db-agents.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 5: STATE MACHINES
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: state-machines ──
  (module-link "intent-pillar-state-machines.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 6: EVENT BUS + WORKER TOPOLOGY
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: event-workers ──
  (module-link "intent-pillar-event-workers.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 7: ENGINES
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: engines ──
  (module-link "intent-pillar-engines.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 8: SEMANTIC PARSER
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: semantic-parser ──
  (module-link "intent-pillar-semantic-parser.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 9: LLM GATEWAYS + CONTEXT PIPELINE
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: llm-context ──
  (module-link "intent-pillar-llm-context.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 10: MCP DISPATCH — 60+ TOOLS
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: mcp-dispatch ──
  (module-link "intent-pillar-mcp-dispatch.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 11: TRANSPORT & BOOTSTRAP
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: transport-bootstrap ──
  (module-link "intent-pillar-transport-bootstrap.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; PILLAR 12: STANDALONE SERVICES & FRONTENDS
  ;; ══════════════════════════════════════════════════════
  ;; ── Pillar: standalone ──
  (module-link "intent-pillar-standalone.lisp")


  ;; ══════════════════════════════════════════════════════
  ;; FLOWS
  ;; ══════════════════════════════════════════════════════
  ;; ── Flows ──
  (module-link "intent-flows.lisp")

) ;; end intent missiond
