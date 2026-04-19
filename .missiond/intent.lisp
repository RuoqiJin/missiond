;; ══════════════════════════════════════════════════════
;; MissionD — Implementation Detail (Full System)
;; Generated: 2026-04-07 | Forge Deep Cartography v3
;; Updated: 2026-04-16 (restructured: 3-layer pillar grouping + inline summaries)
;; ══════════════════════════════════════════════════════

(intent missiond
  (granularity L3-Implementation)
  (survey-head "8438a7d") ;; everything up to this commit has been surveyed
  (survey-date "2026-04-15T00:00:00Z")

  ;; ── DESIGN CONSTRAINTS ──
  (design-constraints
    (crate-boundary
      (missiond-shared        "CliEngine enum + default_mission_home — re-exports CliEngine from semantic-terminal")
      ;; ⚠ missiond-semantic REMOVED (commit 5a5f805) — now external crate:
      ;;   workspace dep: semantic-terminal = { path = "../semantic-terminal/..." }
      ;;   (github.com/ruoqijin/semantic-terminal, independent open-source crate)
      ;;   pure_parsing/ (Forge GenGap) moved to missiond-core/src/semantic_parsing/
      (semantic-terminal      "[EXTERNAL] semantic terminal parser: fingerprints, state, confirm, title, tool, status, patterns")
      (missiond-pty           "PTY session management: PTYManager, PTYSession, IncrementalExtractor, screenshot")
      (missiond-core          "types, DB traits, IPC, embedding, context — depends on pty + semantic-terminal(external) + shared")
      (missiond-daemon        "business logic, handlers, engines, workers (sonnet/codex/gemini/local), LLM gateways")
      (missiond-mcp           "MCP JSON-RPC stdio server, tool schema + dispatch")
      (missiond-attach        "CLI PTY attach utility")
      (missiond-runner        "Claude CLI process wrapper")
      (semantic-terminal-napi "Node N-API bindings for semantic parser")
      (skill-store            "standalone skill marketplace microservice"))

    (communication
      (internal  "DaemonEvent broadcast + per-worker MPSC channels")
      (external  "IPC Unix-socket/TCP between mcp↔daemon")
      (frontend  "WebSocket tokio-tungstenite for realtime board UI"))

    (db-access
      (backend       "PostgreSQL via sqlx::PgPool — default and only production backend (Cargo default features = [postgres])")
      (abstraction   "Arc<dyn MissionStore> — super-trait aggregating 13 domain stores (db/traits.rs:750)")
      (domain-traits "Conversation Message ToolCall Event Retrospective Vision Knowledge Board Timeline Slot Skill Observability Project")
      (impls
        (pg     "PgMissionStore (db/pg/mod.rs) — native async sqlx, 15 files, ~12k lines")
        (sqlite "SqliteMissionStore (db/sqlite/mod.rs) — legacy, opt-in via --features sqlite, wraps sync MissionDB via DbExecutor/spawn_blocking; retained ONLY to support pg::migrate_from_sqlite"))
      (gateway       "missiond-core/src/db/ is sole DB gateway — consumers inject Arc<dyn MissionStore>")
      (migrations    "16 SQL files in crates/missiond-core/migrations/, run by sqlx::migrate! at daemon startup")
      (orphan-artifacts
        ;; ⚠ Forge codegen for DB layer is NOT wired into the build:
        ;;   - .missiond/intent-db*.lisp (8 files, all :backend sqlite) — not module-linked from this file
        ;;   - db/gen/*.rs (8 tracked files) — no `mod gen` declaration in db/mod.rs; never compiled
        ;;   - db/gen_*.rs (8 untracked files at root) — stale output from old Forge config
        ;; All handwritten DB code lives in db/pg/ and db/sqlite/; Forge pipeline needs re-integration or removal.
        "intent-db*.lisp and db/gen*/*.rs exist but are unused"))

    (authority
      (pty-state       "semantic parser pipeline is authority")
      (task-lifecycle  "autopilot tick loop is authority")
      (slot-lifecycle  "SlotManager is authority")
      (knowledge-truth "PostgreSQL KB tables are authority")
      (permission      "PermissionPolicy + LearnedPermissions is authority")))

  ;; ── UPSTREAM DEPENDENCIES ──
  (upstream-dependency
    (forge    :consumes ("forge-build-cli" "generated-rs-output" "generation-gap-pattern")
              :breaks-if ("codegen-pattern-change" "ir-whitelist-change"))
    (mechanic :consumes ("mechanic-repair-cli" "governance-mode-repair")
              :breaks-if ("repair-cli-interface-change" "governance-mode-change")))

  ;; ── LAYER 1: DATA — schema, tables, persistence ──
  ;; 39 tables across 4 domains
  (layer :data
    (pillar db-core          :file "intent-pillar-db-core.lisp"
      :tables 13 :owns "projects board-tasks conversations messages knowledge compute-slots")
    (pillar db-observability  :file "intent-pillar-db-observability.lisp"
      :tables 10 :owns "audit tool-calls timeline llm-traces retrospectives decision-log")
    (pillar db-pipeline       :file "intent-pillar-db-pipeline.lisp"
      :tables 9  :owns "conversation-turns ast-sync beacons watermarks translations")
    (pillar db-agents         :file "intent-pillar-db-agents.lisp"
      :tables 7  :owns "agent-questions dynamic-slots skills router-chat"))

  ;; ── LAYER 2: RUNTIME — state, events, engines, LLM, parsing ──
  ;; The beating heart: workers react to events, engines orchestrate, parsers perceive
  (layer :runtime
    (pillar state-machines    :file "intent-pillar-state-machines.lisp"
      :machines 6 :owns "PtySessionState BoardTaskStatus SlotState SessionState")
    (pillar event-workers     :file "intent-pillar-event-workers.lisp"
      :workers 21 :owns "event-bus event-router 6x-sonnet 2x-codex 1x-gemini 12x-local")
    (pillar engines           :file "intent-pillar-engines.lisp"
      :owns "autopilot-engine flow-engine-v1 flow-engine-v2 slot-manager")
    (pillar semantic-parser   :file "intent-pillar-semantic-parser.lisp"
      :owns "parser-pipeline pattern-config claude-code-parser gemini-parser")
    (pillar llm-context       :file "intent-pillar-llm-context.lisp"
      :providers 3 :owns "sonnet-gateway gemini-gateway minimax-gateway llm-gate prompts"))

  ;; ── LAYER 3: INTERFACE — MCP protocol, transport, standalone services ──
  ;; Entry points: 78 MCP tools, WebSocket, IPC, CLI binaries
  (layer :interface
    (pillar mcp-dispatch      :file "intent-pillar-mcp-dispatch.lisp"
      :tools 78 :domains 4 :owns "compute-tools sysinfra-tools knowledge-tools comm-tools")
    (pillar transport         :file "intent-pillar-transport-bootstrap.lisp"
      :owns "ws-server ipc pty-manager daemon-init")
    (pillar standalone        :file "intent-pillar-standalone.lisp"
      :owns "skill-store missiond-attach missiond-runner semantic-terminal-napi"))

  ;; ── CROSS-LAYER FLOWS ──
  (flows :file "intent-flows.lisp" :count 12
    :catalog ("user-message-to-knowledge" "board-task-lifecycle" "decision-cascade"
              "mcp-request" "context-assembly" "retrospective"
              "embedding-pipeline" "gemini-llm-call"
              "project-scoped-kb-search" "project-cwd-resolution" "project-sync" "project-init"))

) ;; end intent missiond
