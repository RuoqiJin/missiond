;; ═════════════════════════════════════════════════════════════
;; MissionD — System-Layer Pillar (phase-B recursive-contract v0.2)
;; 目标: 无业务语义的运行时底座 — 类型 / 进程 / 传输 / RPC / 工具
;; 底稿: gptpro intent-system-layer.lisp (163 行) + v2/intent.lisp 已有详细占位
;;       + intent-pillar-transport-bootstrap.lisp + intent-types.lisp 等老图
;; 定位: DB 与 observability 已迁 memory pillar; event-bus 独立 pillar 四
;; ═════════════════════════════════════════════════════════════

(pillar system-layer
  :version "v0.2"
  :status "phase-B recursive architecture contract 2026-04-25 — runtime substrate → bootstrap/transport/type surfaces"
  :predecessor "drafts/gptpro/intent-system-layer.lisp (163 行 starter) + v2/intent.lisp 已有详细占位 (L745-832)"
  :target-path ".missiond/v2/intent-system-layer.lisp"

  :actual-state-sources
    [".missiond/v2/intent.lisp :: pillar system-layer (最详细, 已有 bootstrap phases + depends-graph + core-types + pure-utils)"
     ".missiond/intent-types.lisp (v1 老图 — gen_types.rs 源)"
     ".missiond/intent-rpc-gateway.lisp (v1 RPC gateway 老图)"
     ".missiond/intent-pillar-transport-bootstrap.lisp (bootstrap + infra-modules 权威)"
     ".missiond/intent-pure-utility.lisp (pure helpers 老图)"
     ".missiond/intent-pillar-state-machines.lisp (FSM 总览 — 实际 ownership 分散各 pillar)"
     "crates/missiond-core/src/types/gen_types.rs (Forge 冲压输出)"
     "crates/missiond-daemon/src/main.rs (bootstrap)"
     "crates/missiond-daemon/src/state.rs (AppState)"
     "crates/missiond-daemon/src/supervisor.rs"
     "crates/missiond-daemon/src/infra/ (7 文件)"
     "crates/missiond-core/src/ws/ + ipc/"]

  :design-correction-sources
    ["gptpro 5 section (types/bootstrap/transport/rpc/pure-utils) 继承"
     "v2/intent.lisp 已有 4 section 详细内容采纳"
     "worker v0.3 + intent-layer v0.1 的 cross-pillar-notes 对齐"]

  :historical-footprint-sources
    ["DB pool 与 observability 已迁 memory pillar v0.5.4 (原本在 system-layer)"
     "event-bus refactor v2 (commit e139ecf 2026-04-19) 独立成 pillar 四, 原 event_router demoted"
     "git-watcher (infra/git_watcher.rs) v65c8b59 删, 由 tagger_chunker commit detection 接手"]

  ;; ══════════════════════════════════════════════════════════
  ;; phase-A-decisions
  ;; ══════════════════════════════════════════════════════════
  (phase-A-decisions
    (Q-SL1
      :question "system-layer 与 memory pillar 的边界?"
      :decision "memory 管 DB schema + trait + 所有业务 durable state; system-layer 管进程/传输/类型/工具 — 无业务语义"
      :boundary-examples
        ["memory: conversations/board_tasks/kb_entries 等表 schema"
         "system-layer: gen_types.rs 的 enum/struct, 是跨 crate 类型, 但不是业务表"])

    (Q-SL2
      :question "infra/ 7 文件的 ownership?"
      :decision "全部归 system-layer; worker 的 data-plane 穿越 3 文件 (ingestion_router/message_handler/session_util) 已在 worker v0.3 声明"
      :files-owned-here ["aiops.rs" "daemon_stats.rs" "ipc_handler.rs" "mcp_client.rs"]
      :files-owned-but-worker-穿越 ["ingestion_router.rs" "message_handler.rs" "session_util.rs"])

    (Q-SL3
      :question "RPC gateway 与 tools pillar 的关系?"
      :decision "tools pillar 管 schema (78 tool 定义) + dispatch 目的地; system-layer :: rpc-gateway 管传输 (JSON-RPC 协议 / stdio / IPC / error code 归一)"
      :split "schema on tools, transport on system-layer"
      :files-owned-here ["crates/missiond-mcp/src/server.rs" "gateway_impl.rs" "gen_gateway.rs (Forge)" "protocol.rs"])

    (Q-SL4
      :question "state machines 5 FSM 分散各 pillar, system-layer 如何体现?"
      :decision "本 pillar 提供 state-machines-overview section — 列 5 FSM 清单 + ownership 指路, 不复制具体 transition"
      :fsm-crossref
        ["pty-session → worker pillar"
         "board-task → memory pillar :: module board"
         "task/question → memory pillar :: module system-support"
         "engineering-phase → intent-layer pillar :: flow-engine-v1"
         "extraction-phase → intent-layer pillar :: learning-engine :: extraction"])

    (Q-SL5
      :question "bootstrap 已经在 worker v0.3 里详述了, 本 pillar 再写会重复?"
      :decision "本 pillar 详述 bootstrap 细节 (是真正 ownership); worker v0.3 的 daemon-bootstrap-spawn-order 是 cross-pillar 引用"
      :cross-pillar-primacy "本 pillar = primary-ownership; worker pillar = reader"
      :no-duplicate "两处都详写以便查阅, 但约定: 改 bootstrap 从本 pillar 改, worker 跟进"))

  (purpose "无业务语义的运行时底座 — 类型 / 进程 / 传输 / RPC / 纯工具; DB 与 observability 已迁 memory")

  (recursive-architecture-contract
    :shape "pillar = ingress → logic-core → egress; runtime-function = process/transport/type ingress → ordered bootstrap or adapter steps → shared runtime surface"
    :unit "runtime component 是系统层原子; bootstrap phase / transport adapter / type surface 是系统层分子"
    :rule-1 "system-layer 只拥有无业务语义的底座: types / process bootstrap / transport / RPC protocol / pure utils / infra adapters"
    :rule-2 "任何有业务语义的状态机 ownership 必须指向 memory/intent-layer/worker, 本 pillar 只做 overview"
    :rule-3 "bootstrap 必须按 phase 顺序写, 不允许隐式依赖"
    :rule-4 "RPC gateway 只做协议与错误归一, tool schema 归 tools pillar")

  (pillar-ingress
    (entry-1 "daemon 进程启动 (binary main)")
    (entry-2 "stdio JSON-RPC (MCP 协议) / IPC (daemon) / WebSocket (前端)")
    (entry-3 "worker / handler / tool 对共享类型和纯函数的调用")
    (entry-4 "infra 自发监控 (aiops / daemon_stats)"))

  (pillar-core
    :contract "system-layer 负责把进程拉起来、把协议接进来、把共享类型和纯函数提供给上层, 不拥有业务动作"

    (function daemon-bootstrap
      (ingress
        :source "missiond daemon binary main")
      (logic-core
        (step s1 "load config / env / project registry")
        (step s2 "init DB pool and memory gateways")
        (step s3 "init event-bus services")
        (step s4 "construct AppState shared dependencies")
        (step s5 "spawn supervised workers / runtime tasks")
        (step s6 "expose RPC / IPC / WS transports"))
      (egress
        :returns "running daemon AppState"
        :to-worker "worker bootstrap reader"
        :to-tools "RPC gateway ready"))

    (function rpc-transport-normalization
      (ingress
        :sources ["stdio JSON-RPC" "IPC" "WebSocket" "internal mcp_client"])
      (logic-core
        (step s1 "decode transport frame")
        (step s2 "normalize request / error code / response envelope")
        (step s3 "route tool_name or daemon command to gateway boundary"))
      (egress
        :to-tools "MCP tool dispatch"
        :to-frontend "WS/IPC response"))

    (function shared-types-and-pure-utils
      (ingress
        :sources ["Forge generated types" "pure helper callers"])
      (logic-core
        (step s1 "gen_types.rs 提供跨 crate type truth")
        (step s2 "pure utils 执行无 I/O deterministic helper")
        (step s3 "state-machines-overview 只指向各 owner pillar"))
      (egress
        :to-all-pillars "shared structs/enums/helpers"
        :no-business-ownership true))

    (function infra-adapters
      (ingress
        :sources ["aiops" "daemon_stats" "ipc_handler" "mcp_client" "ingestion_router" "message_handler" "session_util"])
      (logic-core
        (step s1 "接入底层系统/外部进程/内部桥接")
        (step s2 "把无业务语义的信号转成上层可消费 adapter output")
        (step s3 "业务副作用交给 memory/worker/tools owner"))
      (egress
        :to-worker "data-plane bridge"
        :to-memory "incidents / stats only through owner APIs"))

    (core-invariants
      (core-1 "gen_types.rs = 跨 crate 单一类型真理源")
      (core-2 "bootstrap = daemon 启动 6 phase 严格依赖序列")
      (core-3 "AppState = 运行时共享依赖 Arc/RwLock 聚合, 启动后近只读")
      (core-4 "RPC gateway = protocol + error normalization; tool schema belongs to tools")
      (core-5 "state machines overview 只做指路, ownership 分散各 pillar")
      (core-6 "pure utils = 无 I/O 的确定性算法")
      (core-7 "ws/ipc = 传输层, 不携带业务语义")))

  (pillar-egress
    (egress-1 "把 AppState / 传输 / 共享类型 暴露给 worker + tools + intent-layer")
    (egress-2 "把 JSON-RPC / IPC / WS 包装成统一运行时接口")
    (egress-3 "把合法状态机 + helper 纯函数 注入上层模块")
    (egress-4 "infra::aiops 产 incidents (写 memory :: system-support)")

    (cross-pillar-notes
      (memory
        :observability-migrated "observability (daemon_stats / llm_traces / incidents schema) 已迁 memory v0.5.1 module system-support"
        :types-cross-ref "gen_types.rs 中的 enum/struct 供 memory schema 用 — 如 BoardTaskStatus enum"
        :fsm-owned-by-memory ["board-task" "task" "question" "incident"])

      (worker
        :data-plane-bridge "worker 穿越 infra/ 的 3 文件 (ingestion_router / message_handler / session_util)"
        :bootstrap-reader "worker v0.3 :: daemon-bootstrap-spawn-order 是本 pillar bootstrap 的 cross-pillar reader")

      (tools
        :rpc-split "tools 管 schema (78 tool 定义); 本 pillar 管 transport (server.rs / gateway_impl / gen_gateway)")

      (intent-layer
        :fsm-owned-by-intent-layer ["engineering-phase" "extraction-phase"]
        :forge-boundary "forge 本体归 intent-layer; 本 pillar 仅提供 process spawn / shell out 机制")

      (event-bus
        :pillar-four "event-bus 独立 pillar 四, 本 pillar 只提供 crate 依赖 (tokio::broadcast / watch channel 等基础设施)")))

  ;; ══════════════════════════════════════════════════════════
  ;; 6.1 Core Types — 跨 crate 共享类型 SSOT
  ;; ══════════════════════════════════════════════════════════
  (section core-types
    :desc "跨 crate 共享的枚举 + 结构体 — 单一真理源, Forge 冲压"
    :targets
      ["crates/missiond-core/src/types/gen_types.rs (Forge-generated)"
       "crates/missiond-core/src/types/*.rs (手写扩展)"]
    :source-lisp ".missiond/intent-types.lisp (v1 Forge 源)"
    :forge-role "core-types.lisp → Forge → gen_types.rs (Generation Gap)"

    (enums-shared
      :count 13
      :phase-B-verified "phase-B-scan-findings-2026-04-21.md § C.1"
      :list
        ["BoardTaskStatus (7: Open/Running/Verifying/Done/Blocked/Failed/Skipped)"
         "BoardNoteType (3: Progress/Summary/Note)"
         "EngineeringPhase (7: Investigate/ConsultGemini1/Plan/ConsultGemini2/Execute/Finalize/Done)"
         "TaskStatus (4: Queued/Running/Completed/Failed)"
         "EventType (6)"
         "AsyncJobStatus (5)"
         "AgentQuestionStatus (5: Pending/Answered/Dismissed/Expired/Harvested)"
         "IncidentSeverity (5: Critical/High/Medium/Low/Info)"
         "IncidentSource (5: Monitoring/AlertManager/Manual/System/Webhook — 注: 代码里还有 HealthCheck/PtySlot 为 aiops 路径用)"
         "DependencyStatus (as_str:false)"
         "CliEngine (2: ClaudeCode/ClaudeMd)"
         "Lifecycle (2: Persistent/Transient)"
         "SlotTrait (5: Memory/Sonnet/AutoPilot/Decision/Strategy)"])

    (structs-shared
      :count 20
      :phase-B-verified "phase-B-scan-findings-2026-04-21.md § C.1"
      :list
        ["FlowContext (6)"
         "BoardTask (37 fields — 最大)"
         "CompactBoardTask (7)"
         "BoardTaskNote (6)"
         "CreateBoardTaskInput (16)"
         "UpdateBoardTaskInput (20)"
         "Conversation (24)"
         "ConversationMessage (19)"
         "KnowledgeEntry (16)"
         "KBRememberInput (6)"
         "KBEdge (5)"
         "Task (16)"
         "InboxMessage (6)"
         "TaskEvent (5)"
         "AgentQuestion (15)"
         "IncidentRow (10)"
         "DynamicSlot (12)"
         "SkillTopic (15)"
         "SkillBlock (9)"
         "ToolCallRecord (11)"])

    (path shared-type-usage
      :lifecycle-style "compile-time"
      (ingress
        :source "任意 crate (daemon / mcp / core) 需要共享枚举/结构/serde 边界"
        :entry-components ["gen_types.rs" "types/ 手写扩展"])
      (logic-core
        (step s1 "在 .missiond/intent-types.lisp 定义 enum / struct / fields")
        (step s2 "Forge 冲压生成 gen_types.rs")
        (step s3 "派生 Serialize/Deserialize + as_str/from_str + match arms")
        (step s4 "供 DB (sqlx derive) / IPC / JSON-RPC / event payload 共用")
        (step s5 "上层模块按这些类型约束状态值域 (如 BoardTaskStatus 枚举定义合法迁移)"))
      (egress
        :writes []
        :reads []
        :returns "shared runtime types (compile-time contract)"
        :serde "derive Serialize/Deserialize — DB/IPC/JSON-RPC 共享"
        :authority "枚举定义合法状态迁移 (例: BoardTaskStatus Open→Running→Done, 禁跳)")))

  ;; ══════════════════════════════════════════════════════════
  ;; 6.2 Process & Transport — bootstrap + AppState + ws + ipc + supervisor
  ;; ══════════════════════════════════════════════════════════
  (section process-transport
    :desc "守护进程生命周期 + IPC/WS 传输 + 全局状态 + 监督"
    :v1-cross-ref "intent-pillar-transport-bootstrap.lisp"

    (component bootstrap
      :desc "main.rs 启动序列 — 6 phase 严格依赖顺序"
      :target "crates/missiond-daemon/src/main.rs"
      :primary-ownership-here "本 pillar (worker v0.3 是 cross-pillar reader)"
      :phases 6
      :phase-list
        ((p1 infrastructure       "DB pool + embed_model + event_bus")
         (p1.5 project-registry   "ProjectRegistry 从 PG 加载 (commit e18d0bf, 必须早于 slot_manager)")
         (p2 core-modules         "PTYManager + SlotManager + MissionControl")
         (p3 gateways             "LLM gateways: sonnet / gemini / codex / (minimax optional) + xjp_router_client embedding")
         (p4 pipelines            "Context pipeline + WorkerRegistry + ControlTree")
         (p5 workers-spawn        "17 BackgroundWorker spawn (main.rs L1007-1385)")
         (p6 engines-and-io       "autopilot + ipc-handler + ws-server"))
      :invariant "每阶段依赖前一阶段; ProjectRegistry 必须早于 message_handler; event_bus 必须早于任何 handler (防事件丢失)"
      :depends-graph
        ((project_registry (db)
            :note "store.list_projects() → ProjectRegistry::new(projects) → SharedProjectRegistry")
         (pty_manager     (event_bus))
         (slot_manager    (db pty_manager event_bus))
         (mission_control (db slot_manager event_bus))
         (gemini_gateway  (db))
         (sonnet_gateway  (slot_manager))
         (llm_gateway     (gemini_gateway sonnet_gateway))
         (context_pipeline (db slot_manager))
         (autopilot       (db slot_manager event_bus llm_gateway context_pipeline))))

    (component app-state
      :desc "全局共享状态 — Arc<RwLock<...>> 贯穿所有 handler"
      :target "crates/missiond-daemon/src/state.rs"
      :fields
        ["db pool"
         "event_bus"
         "slot_manager"
         "llm_gateway"
         "context_pipeline"
         "project_registry (SharedProjectRegistry, commit e18d0bf)"
         "4 MPSC senders (embedding_tx / ast_sync_tx 等)"]
      :invariant "只读访问 (RwLock 只 read); 启动后不再 write — 权威状态在 DB + event_bus"
      :added-field-history "project_registry 字段 commit e18d0bf: path → project_id 解析 + 项目元数据缓存")

    (component ws-server
      :desc "WebSocket 服务器 — 前端订阅端"
      :target "crates/missiond-core/src/ws/server.rs"
      :sub-components
        [(screenshot-broker :target "crates/missiond-core/src/ws/screenshot_broker.rs"
                            :role "异步截屏流分发")
         (jarvis-trace      :target "crates/missiond-core/src/ws/jarvis_trace.rs"
                            :role "trace span 分发给客户端")]
      :consumers "board-frontend Next.js + 其他前端")

    (component ipc
      :desc "mcp ↔ daemon 双向通信 (Unix socket / TCP)"
      :target "crates/missiond-core/src/ipc/mod.rs"
      :sub-components
        [(ipc-handler :target "crates/missiond-daemon/src/infra/ipc_handler.rs"
                      :role "JSON-RPC endpoint (MCP proxy)")])

    (component supervisor
      :desc "worker 健康监控 + 重启"
      :target "crates/missiond-daemon/src/supervisor.rs")

    (path daemon-bootstrap
      :lifecycle-style bootstrap
      (ingress
        :source "binary main 启动"
        :entry-components ["crates/missiond-daemon/src/main.rs"
                           "crates/missiond-daemon/src/state.rs"])
      (logic-core
        (step s1 "Phase 1 — infrastructure: db pool → embed_model → event_bus")
        (step s2 "Phase 1.5 — ProjectRegistry: store.list_projects() → SharedProjectRegistry")
        (step s3 "Phase 2 — core modules: pty_manager → slot_manager → mission_control")
        (step s4 "Phase 3 — gateways: gemini → sonnet → llm_gateway")
        (step s5 "Phase 4 — pipelines: context_pipeline → worker_registry → control_tree")
        (step s6 "Phase 5 — workers: 17 BackgroundWorker spawn")
        (step s7 "Phase 6 — engines & io: autopilot → ipc_handler → ws_server"))
      (egress
        :writes []
        :reads ["projects"]
        :via-bus []
        :memory-cross-ref ["project-management"]
        :returns "booted runtime topology"
        :invariant "依赖前序 + 无逆序"
        :cross-pillar-reader "worker pillar :: daemon-bootstrap-spawn-order"))

    (path app-state-distribution
      :lifecycle-style runtime
      (ingress
        :source "handler / worker / subscriber 需要共享运行时依赖"
        :entry-components ["crates/missiond-daemon/src/state.rs"])
      (logic-core
        (step s1 "封装 db / event_bus / slot_manager / llm_gateway / context_pipeline / project_registry 进 AppState")
        (step s2 "以 Arc<RwLock<...>> 或等价共享方式下发")
        (step s3 "启动后尽量只读访问, 权威状态留 DB + event_bus")
        (step s4 "减少大范围 mutable global state")
        (step s5 "供 tool / worker / infra 统一读取"))
      (egress
        :writes []
        :reads []
        :returns "shared application runtime"))

    (path ipc-transport
      :lifecycle-style runtime
      (ingress
        :source "mcp ↔ daemon 通信请求"
        :entry-components ["crates/missiond-core/src/ipc/mod.rs"
                           "crates/missiond-daemon/src/infra/ipc_handler.rs"])
      (logic-core
        (step s1 "建立 Unix socket / TCP 连接")
        (step s2 "封装请求与响应消息")
        (step s3 "daemon 端解析并路由到 handler")
        (step s4 "等待处理完成或超时")
        (step s5 "把结果回送到请求方"))
      (egress
        :writes []
        :reads []
        :returns "IPC response"))

    (path websocket-stream
      :lifecycle-style runtime
      (ingress
        :source "前端订阅 board / trace / screenshot / timeline streams"
        :entry-components ["crates/missiond-core/src/ws/server.rs"
                           "crates/missiond-core/src/ws/screenshot_broker.rs"
                           "crates/missiond-core/src/ws/jarvis_trace.rs"])
      (logic-core
        (step s1 "客户端建立 WS 连接")
        (step s2 "server 注册订阅")
        (step s3 "上游 runtime 把 screenshot / trace / frontend event 推给 broker")
        (step s4 "server 序列化并广播")
        (step s5 "前端消费更新"))
      (egress
        :writes []
        :reads []
        :returns "frontend stream payload")))

  ;; ══════════════════════════════════════════════════════════
  ;; 6.3 Infra Modules — 7 基础设施文件
  ;; ══════════════════════════════════════════════════════════
  (section infra-modules
    :desc "basic infrastructure — aiops / daemon_stats / ipc_handler / mcp_client / ingestion_router / message_handler / session_util"
    :target-dir "crates/missiond-daemon/src/infra/"
    :file-count 7

    (module-owned-fully
      (aiops
        :target "crates/missiond-daemon/src/infra/aiops.rs"
        :role "AIOps 健康扫描 + 事件桥接 — 300s 扫描 incident"
        :writes "memory :: system-support (incidents 表)"
        :emits "IncidentEvent (via event-bus)"
        :interval "300s"
        :memory-cross-ref ["system-support"])

      (daemon-stats
        :target "crates/missiond-daemon/src/infra/daemon_stats.rs"
        :role "DB 执行时间 + worker 计数器 + observability"
        :consumers "mission_sys_logs / mission_sys_config")

      (ipc-handler
        :target "crates/missiond-daemon/src/infra/ipc_handler.rs"
        :role "JSON-RPC endpoint (MCP proxy)"
        :commit-note "commit ec269d7: 加了对 slot auto-memory 路径的修正指令 (让 slot 用 mission_conversation_query 读历史)")

      (mcp-client
        :target "crates/missiond-daemon/src/infra/mcp_client.rs"
        :role "xjp-mcp-config.json 进程客户端 — daemon 反向调 MCP 工具"
        :consumers "flow-engine-v2 的 McpTool node 类型"))

    (module-owned-but-worker-穿越
      :note "以下 3 文件归本 pillar 但 worker pillar 数据面穿越 — worker v0.3 :: cross-pillar-notes::system-infra 已声明"

      (ingestion-router
        :target "crates/missiond-daemon/src/infra/ingestion_router.rs"
        :role "message classification → worker route"
        :routes ["Conv → conversation_logger"
                 "Codex → codex_ingestion"
                 "PTY → pty_event_worker"
                 "Gemini → gemini_reconcile / gemini_logger"]
        :worker-crossref "worker pillar :: section worker-cluster :: worker-local :: functional-group cli-ingestion")

      (message-handler
        :target "crates/missiond-daemon/src/infra/message_handler.rs"
        :role "JSONL/message normalize → DB write SSOT"
        :commit-e18d0bf "project_id 经 state.project_registry.read().resolve(cwd) 自动填充"
        :writes "memory :: conversation-logs (conversations/conversation_messages)"
        :worker-crossref "worker pillar :: worker-local :: conversation-logger / codex-ingestion / gemini-reconcile / reconcile (通过本文件写入)")

      (session-util
        :target "crates/missiond-daemon/src/infra/session_util.rs"
        :role "PTY session UUID + project_registry 辅助"
        :worker-crossref "worker pillar :: section pty :: subsection pty-transport + claude-slot-dispatch (调用此)"))

    (deleted-modules
      (git-watcher
        :old-target "crates/missiond-daemon/src/infra/git_watcher.rs"
        :deleted-at "commit 65c8b59"
        :replaced-by "ContextualCommitDetected 事件 + worker::tagger_chunker 的 commit detection (commit 1ea1838 吸收 EventAnalyzerWorker)"))

    (path infra-aiops-tick
      :lifecycle-style spawned
      :interval "300s"
      (ingress
        :source "daemon Phase 6 spawn 后持续运行"
        :entry-components ["infra/aiops.rs"])
      (logic-core
        (step s1 "扫描 infra servers (SSH reachability / GPU temp / disk / docker health)")
        (step s2 "检测异常 → 产 incidents")
        (step s3 "发射 IncidentEvent via event-bus")
        (step s4 "写 incidents 表 (memory::system-support)"))
      (egress
        :writes ["incidents"]
        :reads []
        :via-bus ["IncidentEvent"]
        :memory-cross-ref ["system-support"]
        :returns "incident count / health state"
        :tools-surface "mission_incident (sysinfra domain)")))

  ;; ══════════════════════════════════════════════════════════
  ;; 6.4 RPC Gateway — JSON-RPC 传输层
  ;; ══════════════════════════════════════════════════════════
  (section rpc-gateway
    :desc "JSON-RPC 服务器 — stdio(MCP 协议) + IPC(daemon) 双传输"
    :role-boundary "schema 归 tools pillar; transport + routing + error code 归本 pillar"
    :targets
      ["crates/missiond-mcp/src/bin/mission-mcp.rs (binary)"
       "crates/missiond-mcp/src/server.rs (JSON-RPC loop)"
       "crates/missiond-mcp/src/gateway_impl.rs (dispatch)"
       "crates/missiond-mcp/src/gen_gateway.rs (Forge-generated routing table)"
       "crates/missiond-mcp/src/protocol.rs (MCP protocol types)"
       "crates/missiond-mcp/src/lib.rs"]
    :v1-cross-ref ".missiond/intent-rpc-gateway.lisp"

    (methods
      ["initialize"
       "notifications/initialized"
       "tools/list"
       "tools/call"
       "ping"])

    (dispatch-mechanism
      :rule "数据驱动: tool_name → handler 映射, 非硬编码 match"
      :scope "79 tools × 4 domain (schema 归 tools pillar, handler 散各 pillar)"
      :forge-role "gen_gateway.rs 由 Forge 冲压, 源在 intent-mcp-defs.lisp + intent-pillar-mcp-dispatch.lisp")

    (error-codes
      :归一化-in "本 pillar"
      :list
        ["UNKNOWN_TOOL"
         "UNKNOWN_ACTION"
         "MISSING_PARAM"
         "INVALID_PARAM"
         "NOT_FOUND"
         "PERMISSION_DENIED"
         "IPC_TIMEOUT"
         "SPAWN_FAILED"
         "DB_ERROR"])

    (path json-rpc-gateway
      :lifecycle-style runtime
      (ingress
        :source "stdio JSON-RPC / daemon IPC JSON-RPC"
        :entry-components ["gen_gateway.rs" "missiond-mcp gateway_impl.rs" "server.rs"])
      (logic-core
        (step s1 "解析 initialize / notifications/initialized / tools/list / tools/call / ping")
        (step s2 "按数据驱动路由表映射 tool_name → handler_fn")
        (step s3 "参数校验 + action 分派 + 默认值补齐 (schema 来自 tools pillar)")
        (step s4 "生成统一错误码 (UNKNOWN_TOOL / INVALID_PARAM / DB_ERROR 等)")
        (step s5 "通过 mcp_client / ipc / 直接 handler 调用进入 daemon (转给 tools pillar)")
        (step s6 "收集返回文本 / 结构化 JSON / error, 回包给客户端"))
      (egress
        :writes []
        :reads []
        :returns "JSON-RPC response"
        :writes-audit "tools pillar :: tool-call-log (tool_calls 表)")))

  ;; ══════════════════════════════════════════════════════════
  ;; 6.5 Pure Utils — 确定性算法
  ;; ══════════════════════════════════════════════════════════
  (section pure-utils
    :desc "无 I/O 无状态的确定性工具函数 — 横向复用"
    :v1-cross-ref ".missiond/intent-pure-utility.lisp"

    (component semantic-parsing-helpers
      :desc "PTY extractor 的纯 parser helper"
      :target "crates/missiond-core/src/semantic/gen_parsing.rs"
      :functions
        ["is_spinner_char"
         "split_args"
         "extract_phase_from_parens"
         "sanitize_line"
         "has_activity_timer"
         "is_idle_prompt"]
      :consumer "worker pillar :: section pty :: subsection semantic-parser + extractor pipeline")

    (component string-safety
      :desc "UTF-8 安全截断 — CJK 多字节字符不断开"
      :target "crates/missiond-core/src/util/gen_string_helpers.rs"
      :functions
        ["safe_byte_truncate"
         "safe_char_truncate"]
      :rationale "替换代码库里所有 &s[..N] 危险切片")

    (component token-budget
      :desc "context 窗口规划 — token 估算 + 预算分配"
      :target "crates/missiond-daemon/src/context/gen_budget.rs (或 pure_budget/*)"
      :functions
        ["estimate_tokens (英文 /4, 中文 /2)"
         "allocate_budget (N 源 + 边际递减)"]
      :consumer "worker pillar :: section context-assembly :: context-bundle-assembly")

    (path pure-utility-usage
      :lifecycle-style "compile-time library"
      (ingress
        :source "任何上层模块需要安全截断 / token 估算 / parsing 纯逻辑"
        :entry-components ["gen_parsing.rs" "gen_string_helpers.rs" "gen_budget.rs"])
      (logic-core
        (step s1 "safe_byte_truncate / safe_char_truncate 保 UTF-8 安全")
        (step s2 "estimate_tokens 粗估 token 开销")
        (step s3 "allocate_budget 多源边际分配")
        (step s4 "is_spinner_char / sanitize_line 等 PTY helper 纯函数")
        (step s5 "测试样例锁定行为, 避免业务层重复实现"))
      (egress
        :writes []
        :reads []
        :returns "safe truncation / token budget plan / parsing result")))

  ;; ══════════════════════════════════════════════════════════
  ;; 6.6 State Machines Overview — 5 FSM 分布指路
  ;; ══════════════════════════════════════════════════════════
  (section state-machines-overview
    :desc "5 个核心 FSM 清单 + ownership 指路 — 具体 states/transitions 在各 pillar 详述"
    :v1-cross-ref ".missiond/intent-pillar-state-machines.lisp"
    :本-pillar-角色 "只提供 overview index, 不复制每 FSM 的 transitions"

    (fsm-index
      (pty-session
        :states-count 8
        :transitions-count 14
        :owner-pillar "worker"
        :owner-location "worker pillar :: section pty :: subsection pty-state-machine"
        :target-code "semantic-terminal crate (external/Forge) — src/types.rs"
        :summary "Starting/Idle/SlashMenu/Thinking/Responding/ToolRunning/Confirming/Error")

      (board-task
        :states-count 5
        :owner-pillar "memory"
        :owner-location "memory pillar :: module board (enum BoardTaskStatus)"
        :target-code "crates/missiond-core/src/types/board.rs"
        :summary "Open/Running/Done/Failed/Blocked")

      (engineering-phase
        :states-count 5
        :owner-pillar "intent-layer"
        :owner-location "intent-layer pillar :: section state-machines-owned :: engineering-phase + section flow-engine-v1-project-lifecycle"
        :target-code "crates/missiond-core/src/types/board.rs"
        :summary "Investigate/Consult/Plan/Execute/Finalize/Done"
        :consumed-by "flow-engine v1 :: board-phase-engine / mission_submit_phase_result")

      (task
        :states-count 4
        :owner-pillar "memory"
        :owner-location "memory pillar :: module system-support"
        :target-code "crates/missiond-core/src/types/task.rs"
        :summary "Queued/Running/Completed/Failed")

      (question
        :states-count 3
        :owner-pillar "memory"
        :owner-location "memory pillar :: module system-support"
        :target-code "crates/missiond-core/src/types/question.rs"
        :summary "Pending/Answered/Dismissed")

      (extraction-phase
        :states-count 4
        :owner-pillar "intent-layer"
        :owner-location "intent-layer pillar :: section state-machines-owned :: extraction-phase"
        :target-code "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
        :summary "Idle/Sending/WaitingForIdleness/Complete"))

    (fsm-type-source-lisp
      :note "各 FSM 的 enum 定义源 Lisp 在 .missiond/intent-types.lisp, 由 Forge 冲压到对应 gen_types.rs"))

  ;; ══════════════════════════════════════════════════════════
  ;; Need-more-ground-truth (SL-T001 … — phase-B scanned 2026-04-21)
  ;; ══════════════════════════════════════════════════════════
  (need-more-ground-truth
    (SL-T001 :status RESOLVED :resolved-at "2026-04-21"
             :finding "13 enum + 20 struct (v0.1 估 11+12 偏差). 详见 phase-B-scan-findings-2026-04-21.md § C.1. 头部 enums-shared + structs-shared 已更新准确数字")
    (SL-T002 :status RESOLVED :resolved-at "2026-04-21"
             :finding "详见 § C.2. 拓扑: [external MCP client] →stdio→ [missiond-mcp bin] →Unix socket→ [daemon ipc_handler] | daemon 反向 [mcp_client] →child process stdio→ [xjp-mcp] (max 200 calls/recycle, 30s timeout)")
    (SL-T003 :status RESOLVED :resolved-at "2026-04-21"
             :finding "详见 § C.3. bus/ws_bridge.rs 是 event-bus v2 → 前端桥 (100ms 轮询 event_log, v2→v1 byte-compat); ws/server.rs 是通用 WS 多路复用. 互补非重叠")
    (SL-T004 :status RESOLVED :resolved-at "2026-04-21"
             :finding "详见 § C.4. 300s 确认. Pre-check connectivitycheck.gstatic.com 防假警. 扫所有 health_endpoint servers, HTTP GET 5s timeout. 自动 remediation: 恢复→自动 close Board task + aiops author note; 失败→建 Board task + incident (state-based dedup); PtySlot incident → Claude Code (Opus) slot")
    (SL-T005 :status RESOLVED :resolved-at "2026-04-21"
             :finding "详见 § C.5. util/ 仅 string_helpers/ (mod + custom + generated), 无其他文件. 注: semantic-parsing-helpers 实际在 missiond-core/src/semantic/gen_parsing.rs (非 util/)")
    (SL-T006 :status RESOLVED :resolved-at "2026-04-21"
             :finding "详见 § C.6. 14 手写 types 文件 (含 mod/async_job/board/conversation/directive/dynamic_slot/incident/infra/knowledge/project/question/skill/slot/task). 模式: gen_types.rs = struct 定义 + derives, 手写 = impl + helper + trait")
    (SL-T007 :status RESOLVED :resolved-at "2026-04-21"
             :finding "详见 § C.7. 599 行. ExtractionPhase enum + ExtractionState struct + 15% graceful / 3% emergency 阈值. Restart 策略: graceful(Idle 时 restart) / emergency(强 kill 立即) / recovery(requeue+release+sleep 3s+respawn). 独立于 ControlTree")
    (SL-T008 :status RESOLVED :resolved-at "2026-04-21"
             :finding "详见 § C.8. 25+ env vars 全列 (MISSIOND_HOME/FORGE_BIN/MISSION_IPC_SOCKET/MISSION_PG_URL/OLLAMA_HOST 等). xjp-router endpoint/auth 已在 worker I006 代码对齐批次补齐为 MISSION_XJP_ROUTER_ENDPOINT / MISSION_XJP_ROUTER_AUTH_TOKEN"))
)
