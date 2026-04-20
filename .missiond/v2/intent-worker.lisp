;; ═════════════════════════════════════════════════════════════
;; MissionD — Worker Pillar (phase-A draft v0.2)
;; 目标: 按“入点 / 逻辑核心 / 出点”把 worker pillar 升到 ground-truth ontology
;; 底稿: README.md → 00-BRIEF.md → 01 v0.1 → drift-audit / execution / memory frozen
;; ═════════════════════════════════════════════════════════════

(pillar worker
  :version "v0.2"
  :status "phase-A integrated 2026-04-21 — gptpro design + 主 Claude fs-inference fixes"
  :predecessor "v0.1 2026-04-21"
  :target-path ".missiond/v2/intent-worker.lisp"
  :integration-notes
    ["gptpro 原始 v0.2 归档于 .missiond/v2/drafts/gptpro/intent-worker-v0.2.lisp"
     "主 Claude 2026-04-21 修正 3 处 gptpro 推断性 fs inference:"
     "  (a) semantic-parser-files: 8 个 semantic-terminal/src/* 不存在, 已合入 missiond-core/src/semantic_parsing/ 3 文件 + semantic-terminal-napi 壳"
     "  (b) retrieval-fusion entry-components: context/retrieval.rs 不存在, 检索融合由 context_pipeline + code_prefetch + handlers/knowledge/kb 协作"
     "  (c) slot_manager/ 目录不存在, 已合入 slot_orchestrator/"
     "need-more-ground-truth 对应同步"
     "phase-A 完成, phase-B-scan 启动"]
  :actual-state-sources
    [".missiond/v2/drift-audit-2026-04-21.md"
     ".missiond/v2/worker-pillar-execution.lisp"
     ".missiond/v2/intent-pillar-source-index.lisp"]
  :historical-footprint-sources
    ["intent-pillar-event-workers.lisp"
     "intent-pillar-llm-context.lisp"
     "intent-pillar-engines.lisp"
     "intent-pillar-semantic-parser.lisp"
     "intent-domain-engines.lisp"
     "intent-pillar-transport-bootstrap.lisp"
     "intent-pillar-mcp-dispatch.lisp"
     "intent-flows.lisp"]
  :design-correction-sources
    [".missiond/v2/intent.lisp :: pillar worker"
     ".missiond/v2/intent-memory.lisp v0.5.1 frozen"
     ".missiond/workflows/pillar-refactor.lisp"]

  (phase-A-decisions
    (Q1
      :related-pre-deviation P-D001
      :decision accept
      :answer "worker-cluster 改成按 WorkerKind 四分: worker-sonnet / worker-codex / worker-gemini / worker-local。active-roster 降级为每个 kind 子节里的派生清单, 不再充当 ontology。"
      :effect "把目录契约 / BackgroundWorker::KIND / ControlTree provider 注入重新抬升为骨架级语义")

    (Q2
      :related-pre-deviation P-D004
      :decision accept
      :answer "engine-cluster 独立为顶级 section, 与 worker-cluster 并列。worker-cluster = 可被治理的计算租户; engine-cluster = 编排 / 调度 / 分析内核。"
      :effect "避免把 engine 混成 worker 的附录, 也避免把 orchestration 误写成只有 autopilot")

    (Q3
      :related-pre-deviation P-D004
      :decision accept
      :answer "learning_engine 按业务 group 分三层: decision / extraction / analysis。decision=decision_engine+decision_harvest; extraction=extraction+historical_scanner+idle_explorer; analysis=intent_analyst+timeline_analyst。gen_engine.rs 作为 support shell 单列。"
      :effect "让 learning 家族从‘漏掉的 8 个文件’变成有结构的子系统")

    (Q4
      :related-pre-deviation P-D005
      :decision accept-with-clarification
      :answer "Gemini 采用 1 条 path `gemini-unified-gateway`, 但 entry-components 列 5 个具体文件: gemini_driver.rs / gemini_cli.rs / gemini_client.rs / gemini_pty.rs / gemini_file_api.rs。brief 说 4 件套, v0.2 采用‘4 个角色 + 1 个 CLI 适配器’的写法。"
      :effect "既保留统一入口, 又不把 CLI/PTY/File API 的真实分层抹平")

    (Q5
      :related-pre-deviation P-D006
      :decision accept
      :answer "infra cross-pillar-notes 采用‘独立块 + 相关 path step 双写’。pillar-egress 里集中声明 ingestion_router / message_handler / session_util 是 worker data-plane 的穿行点; 在 conversation-jsonl-ingestion / pty-event-worker 等 path 里再次点名。"
      :effect "grep 友好, 且不会把 infra 吞进 worker pillar")

    (Q6
      :related-pre-deviation P-D008
      :decision accept
      :answer "R/W table 以每条 path 的 egress 为主, 不等 phase-E 才补。v0.2 同时在每个 WorkerKind 子节末尾给一个 contract-summary, 但 summary 只是便于审阅, 不替代 path-level contract。"
      :effect "worker → memory 的契约能逐 path 对齐 frozen memory pillar")

    (Q7
      :related-pre-deviation P-D009
      :decision accept
      :answer "context 独立为 `(section context-assembly)`, 与 llm-gateways / worker-cluster / engine-cluster 并列。原因: claude_md_sync.rs 与 topology_map.rs 明显不是 LLM gateway 子层。"
      :effect "把‘LLM 前置’与‘上下文治理’分离, 边界更接近真实代码")

    (Q8
      :related-pre-deviation P-D010
      :decision accept
      :answer "active 定义为 spawned ∪ on-demand-call。并引入 `:lifecycle-style` 四分: spawned / on-demand / planned / zombie-deleted。code_prefetch=on-demand active; experience_harvester=planned 非 active; briefing_worker / step_narrator=zombie-deleted。"
      :effect "活跃清单终于与磁盘 / spawn / runtime 调用三者同时对齐")

    (Q9
      :related-pre-deviation "global"
      :decision accept-with-adjustment
      :answer "v0.2 目标行数采用 1000-1400 行区间。理由: 本版把 19 workers、engine-cluster、PTY/LLM/context、orchestration、cross-pillar contract 都展开到 path 粒度, 500-700 已明显不够。"
      :effect "优先保证结构完整与契约精度, 不为压行数牺牲真实分层")

    (Q10
      :related-pre-deviation "global"
      :decision accept
      :answer "保留 `:actual-state-sources` 顶部元信息, 但以 `.missiond/v2/drift-audit-2026-04-21.md` + `.missiond/v2/intent-pillar-source-index.lisp` + `.missiond/v2/worker-pillar-execution.lisp` 为主。旧地图退到 historical-footprint-sources。"
      :effect "读者能立刻分清‘当前 phase-A 判真依据’与‘历史足迹来源’")

    (pre-deviation-dispositions
      (P-D001
        :disposition accept
        :how "worker-cluster 改为 4 个 WorkerKind 子节, 每个子节带 :kind / :provider-dep / :active-roster / 多条 path。")
      (P-D002
        :disposition accept
        :how "local 采用 `12 files on disk / 10 spawned / 1 on-demand / 1 planned` 的精确记账; 不再沿用旧的模糊 active=12 写法。")
      (P-D003
        :disposition accept
        :how "sonnet 5 个 worker 全部分拆成独立 path: embedding / translation / arch-maintenance / retro / lisp-survey。")
      (P-D004
        :disposition accept
        :how "新增独立顶级 section `engine-cluster`, 补齐 intent_engine 4 条真实 path + learning_engine 7 条 path + flow-engine 3 条 path, gen_engine 壳文件单列。")
      (P-D005
        :disposition accept
        :how "llm-gateways 改成 routing / sonnet / gemini / codex / minimax / prompts 六条 path, entry-components 具体到文件名。")
      (P-D006
        :disposition accept
        :how "pillar-egress 新增 cross-pillar-notes::system-infra, 相关 path 的 logic-core 里也显式写穿越 ingestion_router / message_handler / session_util。")
      (P-D007
        :disposition accept
        :how "PTY 与 slot_orchestrator 全改成具体文件 entry-components, 不再用 glob 代替模块归属。slot_manager 因 phase-A 材料无精确文件清单, 仅保留 runtime authority 注释并标 `:need-more-ground-truth`。")
      (P-D008
        :disposition accept
        :how "所有 worker path 的 egress 均显式列 `:writes / :reads / :via-bus`, 并补 memory-cross-ref 模块归属。")
      (P-D009
        :disposition accept
        :how "context-assembly 独立成节, 把 claude_md_sync / topology_map 从 llm-gateways 下移出。")
      (P-D010
        :disposition partial
        :how "code_prefetch 明确升级为 on-demand active; experience_harvester 定义为 planned 非 active, 但保留 path 设计骨架, 因磁盘文件仍在且 phase-B 仍需确认是否完全废弃。")))

  (purpose "系统如何把计算派出去 — PTY / LLM / context / worker / engine / retrieval / forge 的统一执行层")

  (pillar-ingress
    (entry-1 "来自工具与外部入口的任务: mission_pty_* / mission_compute_slot / mission_worker / mission_flow_run / mission_forge_* / board task")
    (entry-2 "来自 PTY 介质的实时信号: terminal frame / screenshot / semantic state / JSONL / confirm dialog")
    (entry-3 "来自 event-bus 的订阅触发: Message / Session / Slot / System / Llm 事件")
    (entry-4 "来自定时器或 sweep 的触发: autopilot tick / reconcile interval / briefing interval / historical scan")
    (entry-5 "来自 on-demand context 请求的检索: context_pipeline::execute / code_prefetch / hybrid retrieval")
    (entry-6 "来自文件系统与项目变化的触发: ContextualCommitDetected / git diff / project intent 更新"))

  (pillar-core
    (core-1 "WorkerKind 是 ontology; active-roster 只是实例清单")
    (core-2 "worker-cluster 与 engine-cluster 并列: worker = 被治理的计算租户; engine = 编排 / 调度 / 分析内核")
    (core-3 "PTY / LLM / context 是 ingress adapter, 负责把外界输入变成可执行计算, 不是 memory owner")
    (core-4 "worker 对 memory 的所有 durable 读写必须 path-level 显式列出, 不再只写‘写回 conversations / KB / board’这种口号")
    (core-5 "infra/ 仍归 system pillar, 但 worker data-plane 穿越点必须显式声明而非暗耦合")
    (core-6 "lifecycle-style 四分: spawned / on-demand / planned / zombie-deleted, 用于统一理解磁盘、spawn 与 runtime 调用")
    (core-7 "search / retrieval / forge 属于 computation, 因此留在 worker pillar, 不回流到 memory pillar"))

  (pillar-egress
    (egress-1 "把 durable state 写回 memory pillar 的 conversation-logs / board / kb-manager / slot-support / system-support / llm-support 模块")
    (egress-2 "向 event-bus 发射或消费 domain events, 但 event_log 的 SSOT ownership 始终在 pillar event-bus")
    (egress-3 "返回 session handle / dispatch receipt / model output / retrieval result / workflow result 给上游调用者")
    (egress-4 "驱动后续 worker / engine / flow 节点 / slot execution 的下一跳")

    (cross-pillar-notes
      (memory
        :principle "worker 负责计算与时机, memory 负责 schema / trait / durable ownership"
        :table-cross-ref
          (conversation-logs ["conversations" "conversation_messages" "compaction_fragments" "message_labels" "message_translations" "tool_calls" "turns" "turn_topics" "retrospectives"])
          (board            ["board_tasks"])
          (kb-manager       ["kb_entries" "kb_embeddings" "beacon_nodes" "ast_files" "ast_nodes" "ast_embeddings" "ast_search_hits"])
          (project-management ["projects"])
          (slot-support     ["slot_sessions" "slot_tasks"])
          (system-support   ["incidents" "inbox_messages" "daemon_state" "reconcile_watermarks" "deep_analysis" "deep_analysis_checkpoint" "image_descriptions"])
          (llm-support      ["gemini_requests"]))
      (system-infra
        :principle "infra/ 归 system pillar, 但 worker data-plane 明穿其中 3 个文件"
        (data-plane-through
          (ingestion-router "crates/missiond-daemon/src/infra/ingestion_router.rs — message classification → worker route")
          (message-handler  "crates/missiond-daemon/src/infra/message_handler.rs — JSONL / message normalize → DB write SSOT")
          (session-util     "crates/missiond-daemon/src/infra/session_util.rs — PTY session UUID + project registry helper"))
        (not-owned-here
          ["crates/missiond-daemon/src/infra/aiops.rs"
           "crates/missiond-daemon/src/infra/daemon_stats.rs"
           "crates/missiond-daemon/src/infra/ipc_handler.rs"
           "crates/missiond-daemon/src/infra/mcp_client.rs"]))
      (event-bus
        :principle "worker 多数是 subscriber / emitter; append-only bus 与 replay contract 留在 pillar event-bus"
        :common-consumed-events ["MessageEvent::*" "SessionCompleted" "SystemEvent::ContextualCommitDetected" "LlmEvent::*" "ManagerEvent::*"]
        :common-emitted-events   ["SessionEvent::Organized" "SlotBecameIdle" "SlotStuck" "Worker progress / memory-side follow-up events"])
      (intent-layer
        :principle "lisp-survey / arch-maintenance / forge 会触达 intent / flow / generated code, 但 lisp schema ownership 不在 worker pillar")
      (flow
        :principle "flow 定义与 narrative 归 flow pillar; flow 的 runtime execution engine 归 worker pillar")))

  ;; ─────────────────────────────────────────────────────
  ;; 2.1 PTY 介质层
  ;; ─────────────────────────────────────────────────────
  (section pty
    :transport-files
      ["crates/missiond-pty/src/lib.rs"
       "crates/missiond-pty/src/manager.rs"
       "crates/missiond-pty/src/session.rs"
       "crates/missiond-pty/src/extractor.rs"
       "crates/missiond-pty/src/anomaly.rs"
       "crates/missiond-pty/src/screenshot.rs"]
    :semantic-parser-files
      ["crates/missiond-core/src/semantic_parsing/generated.rs"
       "crates/missiond-core/src/semantic_parsing/custom.rs"
       "crates/missiond-core/src/semantic_parsing/mod.rs"
       "crates/semantic-terminal-napi/src/lib.rs"]
    :semantic-parser-legacy-footprint
      ["旧 semantic-terminal crate 的 patterns/state/gemini_state/fingerprint/confirm/tool/status/title"
       "已经经 Forge 冲压合入 missiond-core/src/semantic_parsing/{generated,custom}.rs"
       "历史地图 intent-pillar-semantic-parser.lisp 里仍可见这 8 个原始文件名"]
    :orchestrator-files
      ["crates/missiond-daemon/src/slot_orchestrator/mod.rs"
       "crates/missiond-daemon/src/slot_orchestrator/agent.rs"
       "crates/missiond-daemon/src/slot_orchestrator/types.rs"
       "crates/missiond-daemon/src/slot_orchestrator/controller.rs"
       "crates/missiond-daemon/src/slot_orchestrator/claude_code.rs"
       "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs"
       "crates/missiond-daemon/src/slot_orchestrator/gemini_cli.rs"
       "crates/missiond-daemon/src/slot_orchestrator/gemini_controller.rs"
       "crates/missiond-daemon/src/slot_orchestrator/spawner.rs"
       "crates/missiond-daemon/src/slot_orchestrator/perm_injector.rs"
       "crates/missiond-daemon/src/slot_orchestrator/gen_engine.rs"]
    :bridge-files
      ["crates/missiond-daemon/src/events_sync.rs"
       "crates/missiond-daemon/src/infra/ingestion_router.rs"
       "crates/missiond-daemon/src/infra/message_handler.rs"
       "crates/missiond-daemon/src/infra/session_util.rs"]
    :need-more-ground-truth
      ["crates/missiond-daemon/src/slot_manager/ 目录实际不存在, 已合入 slot_orchestrator/; phase-B 需确认任何残留 slot_manager 命名引用的清理状态"]

    (path pty-session-lifecycle
      :lifecycle-style "on-demand / long-lived"
      (ingress
        :source "mission_pty_* / slot dispatch / flow SlotTask / mission_compute_slot"
        :entry-components
          ["crates/missiond-pty/src/lib.rs"
           "crates/missiond-pty/src/manager.rs"
           "crates/missiond-pty/src/session.rs"])
      (logic-core
        (step s1 "PTYManager 创建、附着或恢复 PTY session, 维护 session_id → runtime handle 的权威索引")
        (step s2 "PTYSession 接管读写循环, 把 CLI 进程、terminal grid 与 semantic stack 绑定成一个交互单元")
        (step s3 "semantic-terminal 外部 crate 负责把原始 screen state 解释成 idle / thinking / responding / confirm / tool / title 等结构化状态")
        (step s4 "session 生命周期变化以 ManagerEvent 形式广播给下游 worker/slot orchestration")
        (step s5 "崩溃、退出、state change 等状态统一回到 runtime authority, 供 slot 层做补偿或回收"))
      (egress
        :writes []
        :reads []
        :via-bus ["ManagerEvent::TextComplete" "ManagerEvent::Exited" "ManagerEvent::StateChange" "ManagerEvent::ConfirmRequired"]
        :returns "session handle / runtime status"))

    (path pty-signal-extraction
      :lifecycle-style "streaming"
      (ingress
        :source "PTYSession 的 frame / scrollback / terminal grid"
        :entry-components
          ["crates/missiond-pty/src/extractor.rs"
           "crates/missiond-pty/src/anomaly.rs"
           "crates/missiond-pty/src/screenshot.rs"])
      (logic-core
        (step s1 "extractor.rs 做 frame-by-frame 增量提取, 过滤 spinner / 状态栏 / 非稳定噪声")
        (step s2 "semantic-terminal 的 state / confirm / tool / title / fingerprint parser 消费这些增量文本, 产出结构化 terminal state")
        (step s3 "anomaly.rs 被动监控 stuck / parser 信心 / anchor 缺失, 形成异常信号")
        (step s4 "screenshot.rs 在需要时把 terminal grid 渲染为 PNG, 供人工或 vision 路径消费")
        (step s5 "上述产物回流给 pty_event_worker / slot orchestration / tool 调用者, 不直接拥有 durable storage"))
      (egress
        :writes []
        :reads []
        :via-bus ["ManagerEvent::*"]
        :file-writes ["terminal screenshot PNG (ephemeral artifact)"]
        :returns "visible text delta / anomaly signal / screenshot artifact"))

    (path conversation-jsonl-ingestion
      :lifecycle-style "event-driven"
      (ingress
        :source "Claude / Gemini / Codex CLI 产生的 JSONL / local state artifacts"
        :entry-components
          ["crates/missiond-daemon/src/events_sync.rs"
           "crates/missiond-daemon/src/infra/ingestion_router.rs"
           "crates/missiond-daemon/src/infra/message_handler.rs"
           "crates/missiond-daemon/src/workers/local/conversation_logger.rs"])
      (logic-core
        (step s1 "events_sync.rs 读取 JSONL 增量或启动时 backfill, 做原始条目切片与 helper 级预处理")
        (step s2 "显式穿越 infra::ingestion_router.rs, 按消息类型把原始事件分类为 conversation / codex / pty 等摄取分支")
        (step s3 "显式穿越 infra::message_handler.rs, 把 JSONL / message payload 归一成可持久化的记录")
        (step s4 "conversation_logger 等 local worker 消费归一化结果, 决定如何写 conversations / messages / follow-up hooks")
        (step s5 "下游 organizer / tagger / reconcile / embedding 继续消费摄入后的事件与表状态"))
      (egress
        :writes ["conversations" "conversation_messages" "board_tasks"]
        :reads ["conversations" "board_tasks" "compaction_fragments"]
        :via-bus ["WatcherEvent::NewMessages" "WatcherEvent::SessionInactive"]
        :memory-cross-ref ["conversation-logs" "board"]
        :returns "persisted conversation delta / follow-up ingestion progress"))

    (path claude-slot-dispatch
      :lifecycle-style "on-demand"
      (ingress
        :source "autopilot / flow-engine / mission_compute_slot / mission_agent / slot task dispatch"
        :entry-components
          ["crates/missiond-daemon/src/slot_orchestrator/mod.rs"
           "crates/missiond-daemon/src/slot_orchestrator/agent.rs"
           "crates/missiond-daemon/src/slot_orchestrator/types.rs"
           "crates/missiond-daemon/src/slot_orchestrator/controller.rs"
           "crates/missiond-daemon/src/slot_orchestrator/claude_code.rs"
           "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs"
           "crates/missiond-daemon/src/slot_orchestrator/spawner.rs"
           "crates/missiond-daemon/src/slot_orchestrator/perm_injector.rs"
           "crates/missiond-daemon/src/slot_orchestrator/gen_engine.rs"
           "crates/missiond-daemon/src/infra/session_util.rs"])
      (logic-core
        (step s1 "agent.rs 依据 task_type / engine / lifecycle 选定 Claude Code 子管理器与 SlotTaskConfig")
        (step s2 "types.rs 与 controller.rs 提供统一 request / timeout / clear / ask 协议, 抹平上层调用方式")
        (step s3 "claude_code.rs 治理 persistent Mutex 与 ephemeral 并发额度, 决定是复用现有 slot 还是经 spawner.rs 拉起新 slot")
        (step s4 "spawner.rs 经过 perm_injector.rs 注入 learned permissions, 再把真实 spawn 统一收束到 sole-spawn bottleneck")
        (step s5 "cc_controller.rs 把 prompt 写入 PTY, 绑定 JSONL session, 等待 TextComplete / idle 等事件回流")
        (step s6 "session_util.rs 负责 session UUID / project registry 侧的辅助解析, 但 ownership 仍不在 worker pillar"))
      (egress
        :writes []
        :reads []
        :via-bus ["ManagerEvent::TextComplete" "ManagerEvent::Exited" "SlotBecameIdle" "SlotStuck"]
        :returns "slot dispatch receipt / session binding result"))

    (path gemini-slot-dispatch
      :lifecycle-style "on-demand"
      (ingress
        :source "strategy / gemini_router / mission_compute_slot / flow SlotTask"
        :entry-components
          ["crates/missiond-daemon/src/slot_orchestrator/mod.rs"
           "crates/missiond-daemon/src/slot_orchestrator/agent.rs"
           "crates/missiond-daemon/src/slot_orchestrator/types.rs"
           "crates/missiond-daemon/src/slot_orchestrator/controller.rs"
           "crates/missiond-daemon/src/slot_orchestrator/gemini_cli.rs"
           "crates/missiond-daemon/src/slot_orchestrator/gemini_controller.rs"
           "crates/missiond-daemon/src/slot_orchestrator/spawner.rs"
           "crates/missiond-daemon/src/slot_orchestrator/perm_injector.rs"
           "crates/missiond-daemon/src/slot_orchestrator/gen_engine.rs"
           "crates/missiond-daemon/src/infra/session_util.rs"])
      (logic-core
        (step s1 "agent.rs 选择 GeminiCliSlotManager 路径, 复用与 Claude 路径同一套 task_type / config 路由协议")
        (step s2 "gemini_cli.rs 管理 persistent / ephemeral 生命周期, 处理 Gemini slot 的并发门禁")
        (step s3 "gemini_controller.rs 委托 Gemini driver / PTY 传输层执行 `/clear + send` 等会话动作, 并为某些路径生成 synthetic session_id")
        (step s4 "spawner.rs / perm_injector.rs 仍是统一 spawn bottleneck, 保证权限注入与 session UUID capture 一致")
        (step s5 "执行结果与运行态通过 PTY 事件和 slot receipt 回流给 worker / engine / caller"))
      (egress
        :writes []
        :reads []
        :via-bus ["ManagerEvent::*" "SlotBecameIdle" "SlotStuck"]
        :returns "gemini slot dispatch receipt / session binding result")))

  ;; ─────────────────────────────────────────────────────
  ;; 2.2 LLM Gateway 层
  ;; ─────────────────────────────────────────────────────
  (section llm-gateways
    :targets
      ["crates/missiond-daemon/src/llm/mod.rs"
       "crates/missiond-daemon/src/llm/llm_gateway.rs"
       "crates/missiond-daemon/src/llm/llm_gate.rs"
       "crates/missiond-daemon/src/llm/sonnet_gateway.rs"
       "crates/missiond-daemon/src/llm/gemini_driver.rs"
       "crates/missiond-daemon/src/llm/gemini_cli.rs"
       "crates/missiond-daemon/src/llm/gemini_client.rs"
       "crates/missiond-daemon/src/llm/gemini_pty.rs"
       "crates/missiond-daemon/src/llm/gemini_file_api.rs"
       "crates/missiond-daemon/src/llm/codex_cli.rs"
       "crates/missiond-daemon/src/llm/minimax_gateway.rs"
       "crates/missiond-daemon/src/llm/minimax_client.rs"
       "crates/missiond-daemon/src/llm/prompts.rs"
       "crates/missiond-daemon/src/llm/gen_engine.rs"]

    (path llm-request-routing
      :lifecycle-style "on-demand"
      (ingress
        :source "worker / engine / handler / flow node 的 LLM 请求"
        :entry-components
          ["crates/missiond-daemon/src/llm/mod.rs"
           "crates/missiond-daemon/src/llm/llm_gateway.rs"
           "crates/missiond-daemon/src/llm/llm_gate.rs"
           "crates/missiond-daemon/src/llm/gen_engine.rs"])
      (logic-core
        (step s1 "llm_gateway.rs 作为顶层路由门面, 按 model/provider 把调用分发到 Sonnet / Gemini / Codex / MiniMax 路径")
        (step s2 "llm_gate.rs 在热路径上执行 kill-switch / provider disable / rate-limit / backoff 判定")
        (step s3 "interactive caller 可按 gate 规则拿到豁免, 后台 worker 默认受 gate 治理")
        (step s4 "provider path 返回文本 / 向量 / 错误, 顶层门面再把失败语义和观测信息回传上游"))
      (egress
        :writes []
        :reads []
        :via-bus ["LlmEvent::*"]
        :returns "model output / error / provider routing result"))

    (path sonnet-priority-gateway
      :lifecycle-style "on-demand"
      (ingress
        :source "embedding_worker / translation_worker / retro_worker / arch_maintenance_worker / lisp_survey / direct sonnet caller"
        :entry-components
          ["crates/missiond-daemon/src/llm/sonnet_gateway.rs"])
      (logic-core
        (step s1 "sonnet_gateway.rs 维持独立优先级 actor 与 30 RPM 级别的 provider 治理")
        (step s2 "chat 与 embedding 共享同一 gateway, 但 embedding 仍保持唯一 provider 约束, 禁止 fallback")
        (step s3 "失败、限流、成功路径都回到顶层 llm_gateway, 由 caller 决定是否重试或降级"))
      (egress
        :writes []
        :reads []
        :via-bus ["LlmEvent::*"]
        :returns "sonnet chat result / embedding vector"))

    (path gemini-unified-gateway
      :lifecycle-style "on-demand"
      (ingress
        :source "strategy_worker / gemini slot path / multimodal file request / direct Gemini API caller"
        :entry-components
          ["crates/missiond-daemon/src/llm/gemini_driver.rs"
           "crates/missiond-daemon/src/llm/gemini_cli.rs"
           "crates/missiond-daemon/src/llm/gemini_client.rs"
           "crates/missiond-daemon/src/llm/gemini_pty.rs"
           "crates/missiond-daemon/src/llm/gemini_file_api.rs"])
      (logic-core
        (step s1 "gemini_client.rs 作为统一 client, 在 HTTP / CLI 模式之间切换, 负责速率限制与重试壳")
        (step s2 "gemini_driver.rs 把 Gemini PTY 会话统一成高层 driver, 管 `/clear` 隔离、@file 上传与事件机制")
        (step s3 "gemini_cli.rs 管理 stream-json CLI 子进程, 兼顾 tool 执行扩展超时")
        (step s4 "gemini_pty.rs 是底层 PTY 传输包装, 保证 clear+send 的原子时序")
        (step s5 "gemini_file_api.rs 负责多模态文件上传与缓存去重, 让 PDF/视频等资产走 file API 而非裸 prompt"))
      (egress
        :writes []
        :reads []
        :via-bus ["LlmEvent::*"]
        :returns "Gemini text / multimodal result / PTY driver result"))

    (path codex-cli-gateway
      :lifecycle-style "on-demand"
      (ingress
        :source "vision_worker / image-aware codex caller"
        :entry-components
          ["crates/missiond-daemon/src/llm/codex_cli.rs"])
      (logic-core
        (step s1 "codex_cli.rs 包装 Codex / Claude Code 风格 CLI 子进程, 支持 vision/image 输入")
        (step s2 "调用前把图片或 multimodal payload 归一成 CLI 可消费格式")
        (step s3 "调用后解析 JSONL / stdout 事件, 把模型输出交回上游 worker"))
      (egress
        :writes []
        :reads []
        :via-bus ["LlmEvent::*"]
        :returns "codex cli result / parsed JSONL events"))

    (path minimax-legacy-gateway
      :lifecycle-style "legacy"
      (ingress
        :source "legacy briefing / compatibility caller"
        :entry-components
          ["crates/missiond-daemon/src/llm/minimax_gateway.rs"
           "crates/missiond-daemon/src/llm/minimax_client.rs"])
      (logic-core
        (step s1 "minimax_gateway.rs 维持旧的优先级 actor / 配额跟踪 / 四通道路由")
        (step s2 "minimax_client.rs 执行轻量 HTTP 调用")
        (step s3 "phase-A 把它保留成兼容路径, 但不再把它当主执行面"))
      (egress
        :writes []
        :reads []
        :via-bus ["LlmEvent::*"]
        :returns "legacy minimax result"))

    (path prompt-template-resolution
      :lifecycle-style "on-demand"
      (ingress
        :source "任意 LLM request 进入 provider 前的模板装配"
        :entry-components
          ["crates/missiond-daemon/src/llm/prompts.rs"])
      (logic-core
        (step s1 "prompts.rs 读取中央 prompt 模板与文件 override")
        (step s2 "按场景选择 Tier2 / Tier3 模板, 与 caller 提供的 prompt bundle 合成")
        (step s3 "把模板解析结果交回 llm_gateway / worker / flow handler"))
      (egress
        :writes []
        :reads []
        :via-bus []
        :returns "resolved prompt template bundle")))

  ;; ─────────────────────────────────────────────────────
  ;; 2.3 Context Assembly (独立于 LLM gateway)
  ;; ─────────────────────────────────────────────────────
  (section context-assembly
    :targets
      ["crates/missiond-daemon/src/context/mod.rs"
       "crates/missiond-daemon/src/context/claude_md_sync.rs"
       "crates/missiond-daemon/src/context/context_budget.rs"
       "crates/missiond-daemon/src/context/context_pipeline.rs"
       "crates/missiond-daemon/src/context/slot_env.rs"
       "crates/missiond-daemon/src/context/topology_map.rs"
       "crates/missiond-daemon/src/context/pure_budget/generated.rs"
       "crates/missiond-daemon/src/context/pure_budget/custom.rs"
       "crates/missiond-daemon/src/context/pure_budget/mod.rs"]

    (path slot-env-build
      :lifecycle-style "on-demand"
      (ingress
        :source "slot 激活 / LLM 调用前的 runtime context build"
        :entry-components
          ["crates/missiond-daemon/src/context/slot_env.rs"])
      (logic-core
        (step s1 "slot_env.rs 收集 role / cwd / project / session tracking file / secret resolve 等 runtime 输入")
        (step s2 "把这些输入归一成 slot 能直接消费的环境变量与 prompt 前置元信息")
        (step s3 "将 build 结果返回给 context_pipeline, 不直接做 provider 调用"))
      (egress
        :writes []
        :reads []
        :via-bus []
        :returns "slot-scoped environment bundle"))

    (path claude-md-managed-sync
      :lifecycle-style "on-demand / sync"
      (ingress
        :source "context assemble 需要项目托管段 / hot topics / preferences 时"
        :entry-components
          ["crates/missiond-daemon/src/context/claude_md_sync.rs"])
      (logic-core
        (step s1 "claude_md_sync.rs 从 KB / project memory 抽取适合托管进 CLAUDE.md 的 preferences 与 hot topics")
        (step s2 "把托管段同步到项目 CLAUDE.md 的约定位置, 避免人工维护与运行态记忆脱节")
        (step s3 "把同步后的摘要回给 context pipeline 作为一个 source, 而不是把它伪装成 LLM gateway 职责"))
      (egress
        :writes []
        :reads ["kb_entries"]
        :via-bus []
        :file-writes ["<project>/CLAUDE.md managed sections"]
        :memory-cross-ref ["kb-manager"]
        :returns "managed CLAUDE.md fragments / sync result"))

    (path topology-map-resolution
      :lifecycle-style "on-demand"
      (ingress
        :source "context pipeline 需要模块导航 / AST+KB 聚合 / 降级查询时"
        :entry-components
          ["crates/missiond-daemon/src/context/topology_map.rs"])
      (logic-core
        (step s1 "topology_map.rs 组合 AST 结构、KB 片段与模块导航信息")
        (step s2 "优先走结构化导航; 缺失时做降级查询或回退到更粗粒度的项目拓扑")
        (step s3 "把路径级 / 模块级导航结果回给 context_pipeline 参与 source ranking"))
      (egress
        :writes []
        :reads ["ast_nodes" "kb_entries"]
        :via-bus []
        :memory-cross-ref ["kb-manager"]
        :returns "topology map / module navigation result"))

    (path context-bundle-assembly
      :lifecycle-style "on-demand"
      (ingress
        :source "任何一次即将发起的 LLM / slot 请求"
        :entry-components
          ["crates/missiond-daemon/src/context/mod.rs"
           "crates/missiond-daemon/src/context/context_pipeline.rs"
           "crates/missiond-daemon/src/context/context_budget.rs"
           "crates/missiond-daemon/src/context/pure_budget/generated.rs"
           "crates/missiond-daemon/src/context/pure_budget/custom.rs"
           "crates/missiond-daemon/src/context/pure_budget/mod.rs"])
      (logic-core
        (step s1 "context_pipeline.rs 按优先级拼 sources: slot-env → skill-context → kb → conversation-history → topology-map → claude-md")
        (step s2 "需要代码检索时直接 on-demand 调用 workers/local/code_prefetch.rs, 不强行要求它走 BackgroundWorker 生命周期")
        (step s3 "context_budget.rs 与 pure_budget/* 估 token / byte budget, 处理 6MB 载荷上限、源间分配与裁剪衰减")
        (step s4 "必要时并发发起 KB / history / topology retrieval, 再按 source trace 汇总成 assembled bundle")
        (step s5 "最终 bundle 交给 llm_gateway 或 slot dispatch, 自身不拥有 provider routing 权限"))
      (egress
        :writes []
        :reads ["kb_entries" "conversations" "conversation_messages" "ast_nodes" "beacon_nodes" "ast_search_hits"]
        :via-bus []
        :memory-cross-ref ["kb-manager" "conversation-logs"]
        :returns "assembled prompt bundle + source trace"
        :cross-ref "worker-local/code-prefetch = on-demand retrieval dependency")))

  ;; ─────────────────────────────────────────────────────
  ;; 2.4 Worker Cluster — 按 WorkerKind 四分
  ;; ─────────────────────────────────────────────────────
  (section worker-cluster
    :active-definition "spawned ∪ on-demand-call"
    :disk-footprint-summary "19 worker files on disk; 17 spawned; 1 on-demand active (code_prefetch); 1 planned non-active (experience_harvester)"

    (zombie-ledger
      (code-prefetch         :lifecycle-style on-demand       :status "disk-present / runtime-called / not spawned")
      (experience-harvester  :lifecycle-style planned         :status "disk-present / not spawned / plan-or-prototype")
      (briefing-worker       :lifecycle-style zombie-deleted  :status "removed from sonnet/ in v1.3.0")
      (step-narrator         :lifecycle-style zombie-deleted  :status "removed from codex/ in v0.4.23"))

    ;; ── WorkerKind::Sonnet ───────────────────────────────
    (subsection worker-sonnet
      :kind "WorkerKind::Sonnet"
      :provider-dep ["Dependency::Provider(Sonnet)"]
      :disk-count 5
      :spawned-count 5
      :active-roster ["embedding_worker" "translation_worker" "arch_maintenance_worker" "retro_worker" "lisp_survey_worker"]
      :targets
        ["crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs"
         "crates/missiond-daemon/src/workers/sonnet/translation_worker.rs"
         "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
         "crates/missiond-daemon/src/workers/sonnet/retro_worker.rs"
         "crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs"]

      (path embedding-worker-loop
        :lifecycle-style spawned
        (ingress
          :source "EmbeddingTask MPSC channel from ast_sync_worker / backfill path"
          :entry-components
            ["crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs"
             "crates/missiond-daemon/src/llm/sonnet_gateway.rs"])
        (logic-core
          (step s1 "消费 embedding_rx / embedding_tx 通道上的 EmbeddingTask, 把任务分派成 conversation / KB / AST 等不同 embedding 子路径")
          (step s2 "读取 conversations / ast_nodes / kb_entries / compaction_fragments 与既有 embedding 表, 决定是增量 upsert 还是 backfill")
          (step s3 "经 sonnet_gateway 调用唯一 embedding provider, 失败直接上抛, 不做 fallback")
          (step s4 "把向量写回 kb_embeddings / ast_embeddings / turn_topics, 并避免重复计算")
          (step s5 "必要时记录进度并释放后续检索可见性"))
        (egress
          :writes ["kb_embeddings" "ast_embeddings" "turn_topics"]
          :reads ["conversations" "ast_nodes" "kb_entries" "compaction_fragments" "ast_embeddings" "kb_embeddings"]
          :via-bus ["EmbeddingTask (MPSC from ast_sync_worker)"]
          :memory-cross-ref ["kb-manager" "conversation-logs"]
          :returns "embedding progress / vector upsert summary"))

      (path translation-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "MessageEvent::thinking_message"
          :entry-components
            ["crates/missiond-daemon/src/workers/sonnet/translation_worker.rs"
             "crates/missiond-daemon/src/llm/sonnet_gateway.rs"])
        (logic-core
          (step s1 "订阅 thinking_message 类 message 事件, 过滤出需要翻译或标准化的文本")
          (step s2 "读取 conversation_messages 原文与必要元数据, 生成 translation prompt")
          (step s3 "经 sonnet_gateway 获取译文")
          (step s4 "把译文写回 message_translations, 供检索、UI 或后续分析消费"))
        (egress
          :writes ["message_translations"]
          :reads ["conversation_messages"]
          :via-bus ["MessageEvent::thinking_message"]
          :memory-cross-ref ["conversation-logs"]
          :returns "translation result / skipped-or-written status"))

      (path arch-maintenance-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "SystemEvent::ContextualCommitDetected"
          :entry-components
            ["crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
             "crates/missiond-daemon/src/slot_orchestrator/agent.rs"
             "crates/missiond-daemon/src/slot_orchestrator/claude_code.rs"
             "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs"])
        (logic-core
          (step s1 "消费带 conversation/session/slot 上下文的 ContextualCommitDetected 事件, 不再走粗糙的定时 git-log 轮询")
          (step s2 "把 commit diff / project / context 归一成 architecture maintenance prompt")
          (step s3 "通过 SlotManager.execute 风格路径把请求派到受管 AI slot, 而不是直接在 worker 内部写文件")
          (step s4 "slot 侧执行 Edit / file mutation, 产生 architecture manifest 或 flow side artifact")
          (step s5 "worker 只持有调度权, 实际文件写入发生在 slot 执行侧"))
        (egress
          :writes []
          :reads []
          :via-bus ["SystemEvent::ContextualCommitDetected"]
          :file-writes ["~/.missiond/flows/... (indirect via SlotManager.execute)" "project architecture manifest files (indirect)"]
          :returns "slot dispatch result / architecture maintenance side effect"
          :write-style "indirect via slot execution"))

      (path retro-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "SessionCompleted"
          :entry-components
            ["crates/missiond-daemon/src/workers/sonnet/retro_worker.rs"
             "crates/missiond-daemon/src/llm/sonnet_gateway.rs"])
        (logic-core
          (step s1 "订阅 SessionCompleted, 选取需要复盘的 conversation window")
          (step s2 "读取 conversations 与相关统计信息, 组装 retrospective prompt")
          (step s3 "经 sonnet_gateway 生成 session retro / deep analysis")
          (step s4 "把复盘结果写入 deep_analysis 与 retrospectives, 供之后的记忆和策略层消费"))
        (egress
          :writes ["deep_analysis" "retrospectives"]
          :reads ["conversations"]
          :via-bus ["SessionCompleted"]
          :memory-cross-ref ["system-support" "conversation-logs"]
          :returns "retro analysis result"))

      (path lisp-survey-update
        :lifecycle-style spawned
        (ingress
          :source "SystemEvent::ContextualCommitDetected"
          :entry-components
            ["crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs"
             "crates/missiond-daemon/src/slot_orchestrator/agent.rs"
             "crates/missiond-daemon/src/slot_orchestrator/claude_code.rs"
             "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs"])
        (logic-core
          (step s1 "先做 self-trigger 过滤: slot_id == lisp-surveyor 的 commit 不再递归 survey")
          (step s2 "ProjectRegistry 解析 project_id 与 intent_path; 没配置 intent.lisp 的项目直接跳过")
          (step s3 "按 project 做 60s debounce, 防止连续 commit 把 survey slot 打爆")
          (step s4 "把 diff + intent_path 组装成 survey prompt, 交给 persistent survey slot 执行")
          (step s5 "解析返回: NO_CHANGE 则结束; 否则让 slot 的 Edit 工具更新 intent.lisp"))
        (egress
          :writes []
          :reads []
          :via-bus ["SystemEvent::ContextualCommitDetected"]
          :file-writes ["<project>/.missiond/intent.lisp"]
          :returns "survey result / intent file update"
          :write-style "indirect via slot execution"))

      (contract-summary
        :writes ["kb_embeddings" "ast_embeddings" "turn_topics" "message_translations" "deep_analysis" "retrospectives"]
        :reads  ["conversations" "conversation_messages" "ast_nodes" "kb_entries" "compaction_fragments" "ast_embeddings" "kb_embeddings"]
        :file-writes ["~/.missiond/flows/..." "<project>/.missiond/intent.lisp"]))

    ;; ── WorkerKind::Codex ────────────────────────────────
    (subsection worker-codex
      :kind "WorkerKind::Codex"
      :provider-dep ["Dependency::Provider(Codex)"]
      :disk-count 1
      :spawned-count 1
      :active-roster ["vision_worker"]
      :targets
        ["crates/missiond-daemon/src/workers/codex/vision_worker.rs"]

      (path vision-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "MessageEvent::vision tasks"
          :entry-components
            ["crates/missiond-daemon/src/workers/codex/vision_worker.rs"
             "crates/missiond-daemon/src/llm/codex_cli.rs"])
        (logic-core
          (step s1 "订阅 vision task 事件, 定位需要图像理解的 conversation message")
          (step s2 "读取 conversation_messages.raw_content, 提取图片引用或附件上下文")
          (step s3 "经 codex_cli.rs 发起图像/多模态调用")
          (step s4 "把结果写回 image_descriptions, 供 UI / recall / strategy 等后续路径消费"))
        (egress
          :writes ["image_descriptions"]
          :reads ["conversation_messages"]
          :via-bus ["MessageEvent::vision tasks"]
          :memory-cross-ref ["system-support" "conversation-logs"]
          :returns "vision description result"))

      (contract-summary
        :writes ["image_descriptions"]
        :reads  ["conversation_messages"]))

    ;; ── WorkerKind::Gemini ───────────────────────────────
    (subsection worker-gemini
      :kind "WorkerKind::Gemini"
      :provider-dep ["Dependency::Provider(Gemini)"]
      :disk-count 1
      :spawned-count 1
      :active-roster ["strategy_worker"]
      :targets
        ["crates/missiond-daemon/src/workers/gemini/strategy_worker.rs"]

      (path strategy-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "SessionCompleted"
          :entry-components
            ["crates/missiond-daemon/src/workers/gemini/strategy_worker.rs"
             "crates/missiond-daemon/src/llm/gemini_driver.rs"
             "crates/missiond-daemon/src/llm/gemini_cli.rs"
             "crates/missiond-daemon/src/llm/gemini_client.rs"])
        (logic-core
          (step s1 "订阅 SessionCompleted, 决定是否为该会话生成 strategy analysis")
          (step s2 "读取 conversations、战略状态类 kb_entries 与 daemon_state, 组装策略 prompt")
          (step s3 "走 Gemini driver / CLI / client 路径执行策略分析")
          (step s4 "把结果分别沉淀到 inbox_messages、kb_entries(strategic-state) 与 deep_analysis"))
        (egress
          :writes ["inbox_messages" "kb_entries" "deep_analysis"]
          :reads ["conversations" "kb_entries" "daemon_state"]
          :via-bus ["SessionCompleted"]
          :memory-cross-ref ["system-support" "kb-manager" "conversation-logs"]
          :returns "strategy analysis result"))

      (contract-summary
        :writes ["inbox_messages" "kb_entries" "deep_analysis"]
        :reads  ["conversations" "kb_entries" "daemon_state"]))

    ;; ── WorkerKind::Local ────────────────────────────────
    (subsection worker-local
      :kind "WorkerKind::Local"
      :provider-dep []
      :disk-count 12
      :spawned-count 10
      :on-demand-count 1
      :planned-count 1
      :active-roster
        ["ast_sync_worker"
         "code_prefetch"
         "codex_ingestion_worker"
         "conversation_logger"
         "conversation_organizer"
         "gemini_logger"
         "gemini_reconcile_worker"
         "pty_event_worker"
         "reconcile_worker"
         "tagger_chunker"
         "xjpcode_briefing_worker"]
      :planned-roster ["experience_harvester"]
      :targets
        ["crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
         "crates/missiond-daemon/src/workers/local/code_prefetch.rs"
         "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"
         "crates/missiond-daemon/src/workers/local/conversation_logger.rs"
         "crates/missiond-daemon/src/workers/local/conversation_organizer.rs"
         "crates/missiond-daemon/src/workers/local/experience_harvester.rs"
         "crates/missiond-daemon/src/workers/local/gemini_logger.rs"
         "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
         "crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
         "crates/missiond-daemon/src/workers/local/reconcile_worker.rs"
         "crates/missiond-daemon/src/workers/local/tagger_chunker.rs"
         "crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs"]

      (path ast-sync-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "增量 git diff / changed file sweep / ast_sync_rx MPSC"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"])
        (logic-core
          (step s1 "扫描变更文件或消费 ast_sync 触发, 决定哪些源码需要重新解析")
          (step s2 "读取 ast_files 当前记录做增量判断, 避免全量重建")
          (step s3 "生成 AST 结构与 beacon 相关节点, upsert 到 ast_files / ast_nodes / beacon_nodes")
          (step s4 "为需要向量化的内容组装 EmbeddingTask 并发到 embedding_tx channel")
          (step s5 "把更新后的代码索引暴露给 context / search / code_prefetch"))
        (egress
          :writes ["ast_files" "ast_nodes" "beacon_nodes"]
          :reads ["ast_files"]
          :via-bus ["EmbeddingTask -> embedding_tx"]
          :memory-cross-ref ["kb-manager"]
          :returns "ast sync delta / embedding tasks"))

      (path code-prefetch-on-demand-retrieval
        :lifecycle-style on-demand
        (ingress
          :source "context_pipeline::execute() / retrieval call sites"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/code_prefetch.rs"
             "crates/missiond-daemon/src/context/context_pipeline.rs"])
        (logic-core
          (step s1 "接收 on-demand 查询, 不通过 main.rs spawn, 直接由 context pipeline 调用")
          (step s2 "在 beacon_nodes / ast_nodes 上跑 FTS5 或结构化代码检索, 并结合 ast_search_hits / kb_entries 取补充上下文")
          (step s3 "做 hybrid merge / rank, 返回 snippet 与命中解释")
          (step s4 "仅返回结果, 不负责持久化也不宣称自己是标准 BackgroundWorker"))
        (egress
          :writes []
          :reads ["beacon_nodes" "ast_nodes" "ast_search_hits" "kb_entries"]
          :via-bus []
          :memory-cross-ref ["kb-manager"]
          :returns "hybrid code retrieval result"))

      (path codex-ingestion-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "poll ~/.codex/state_5.sqlite"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"])
        (logic-core
          (step s1 "周期性轮询 Codex SQLite, 读取操作历史与 JSONL-like state")
          (step s2 "解析文本消息与 tool call 记录, 把 Codex CLI 历史转成 MissionD 可消费的会话片段")
          (step s3 "upsert conversations / conversation_messages / tool_calls, 保持与其他 CLI 引擎统一")
          (step s4 "把对账后的会话状态交给下游 organizer / tagger / recall 系统"))
        (egress
          :writes ["conversations" "conversation_messages" "tool_calls"]
          :reads []
          :via-bus []
          :memory-cross-ref ["conversation-logs"]
          :returns "codex ingestion progress"))

      (path conversation-logger-cycle
        :lifecycle-style spawned
        (ingress
          :source "WatcherEvent::NewMessages / WatcherEvent::SessionInactive"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/conversation_logger.rs"
             "crates/missiond-daemon/src/events_sync.rs"
             "crates/missiond-daemon/src/infra/ingestion_router.rs"
             "crates/missiond-daemon/src/infra/message_handler.rs"])
        (logic-core
          (step s1 "收到 watcher 信号后, tail JSONL 或做一次 session backfill")
          (step s2 "显式穿越 infra::ingestion_router.rs 做消息分类")
          (step s3 "显式穿越 infra::message_handler.rs 把消息归一成可落库形态")
          (step s4 "写 conversations / conversation_messages, 并按需要创建 board_tasks 形式的 memory hook")
          (step s5 "把已落地 session 暴露给 organizer / tagger / reconcile 等后续 worker"))
        (egress
          :writes ["conversations" "conversation_messages" "board_tasks"]
          :reads ["conversations" "board_tasks" "compaction_fragments"]
          :via-bus ["WatcherEvent::NewMessages" "WatcherEvent::SessionInactive"]
          :memory-cross-ref ["conversation-logs" "board"]
          :returns "persisted conversation delta / memory hook submission"))

      (path conversation-organizer-cycle
        :lifecycle-style spawned
        (ingress
          :source "MessageEvent::Logged → SessionEvent::Organized"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/conversation_organizer.rs"])
        (logic-core
          (step s1 "消费新落地 message 的 session 脏集合")
          (step s2 "对 agent session 做 compaction link 修复与 orphan parent link 修补")
          (step s3 "对所有 session 统一做 debounce, 然后发出 SessionOrganized 供下游认知管道消费")
          (step s4 "broadcast lagged 场景只警告不 panic, 让 tagger_chunker 的 reconcile tick 兜底"))
        (egress
          :writes ["conversations"]
          :reads ["conversations"]
          :via-bus ["MessageEvent::Logged" "SessionEvent::Organized"]
          :memory-cross-ref ["conversation-logs"]
          :returns "organized session ids / follow-up signal"))

      (path experience-harvester-prototype
        :lifecycle-style planned
        (ingress
          :source "暂无稳定 subscription; 预期为 interval / exploration trigger"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/experience_harvester.rs"])
        (logic-core
          (step s1 "读取 ast_nodes / conversations / tool_calls 等多源证据, 寻找高价值经验或信标候选")
          (step s2 "把候选经验归纳成可写 beacon / board follow-up")
          (step s3 "当前 phase-A 仅保留行为骨架; phase-B 需确认它究竟是未来计划功能还是未接线原型"))
        (egress
          :writes ["beacon_nodes" "board_tasks"]
          :reads ["ast_nodes" "conversations" "tool_calls"]
          :via-bus []
          :memory-cross-ref ["kb-manager" "board" "conversation-logs"]
          :returns "planned harvester result"
          :status "disk-present / not spawned / non-active"))

      (path gemini-logger-cycle
        :lifecycle-style spawned
        (ingress
          :source "LlmEvent::RequestStarted / LlmEvent::ResponseCompleted"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/gemini_logger.rs"])
        (logic-core
          (step s1 "监听 Gemini 请求生命周期事件")
          (step s2 "归一记录 request / response / latency / status 等日志")
          (step s3 "把 Gemini 调用审计写入 gemini_requests"))
        (egress
          :writes ["gemini_requests"]
          :reads []
          :via-bus ["LlmEvent::RequestStarted" "LlmEvent::ResponseCompleted"]
          :memory-cross-ref ["llm-support"]
          :returns "gemini request log status"))

      (path gemini-reconcile-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "poll ~/.gemini/tmp"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"])
        (logic-core
          (step s1 "周期性扫描 ~/.gemini/tmp 与当前 MissionD conversation state 的差异")
          (step s2 "根据 reconcile_watermarks 决定从哪里继续对账")
          (step s3 "补写 conversations / conversation_messages, 并推进 reconcile_watermarks")
          (step s4 "保证 Gemini 外部 state 与 MissionD 内部会话记录最终一致"))
        (egress
          :writes ["conversations" "conversation_messages" "reconcile_watermarks"]
          :reads ["reconcile_watermarks" "conversations"]
          :via-bus []
          :memory-cross-ref ["conversation-logs" "system-support"]
          :returns "gemini reconcile progress"))

      (path pty-event-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "ManagerEvent::TextComplete / Exited / StateChange / ConfirmRequired"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
             "crates/missiond-daemon/src/infra/session_util.rs"])
        (logic-core
          (step s1 "消费 PTY runtime 广播出的 ManagerEvent, 识别 idle / stuck / confirm-required 等状态")
          (step s2 "显式穿越 infra::session_util.rs 解析 session UUID、project_id 与 slot 归属辅助信息")
          (step s3 "遇到已知确认弹窗时执行 auto-approve, 并把 learned permission 路径交给更底层权限模块")
          (step s4 "更新 conversations / slot_sessions / message_labels / incidents / deep_analysis_checkpoint 等运行期表")
          (step s5 "必要时发射 SlotBecameIdle / SlotStuck, 驱动上层 orchestration 继续推进"))
        (egress
          :writes ["conversations" "slot_sessions" "deep_analysis_checkpoint" "message_labels" "incidents"]
          :reads ["conversations" "slot_sessions" "board_tasks"]
          :via-bus ["ManagerEvent::TextComplete" "ManagerEvent::Exited" "ManagerEvent::StateChange" "ManagerEvent::ConfirmRequired"]
          :memory-cross-ref ["conversation-logs" "slot-support" "system-support" "board"]
          :returns "slot runtime side effects / slot events"))

      (path reconcile-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "periodic poll ~/.claude/projects"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/reconcile_worker.rs"])
        (logic-core
          (step s1 "扫描 Claude 项目目录中的 JSONL 与已持久化状态之间的 gap")
          (step s2 "依据 reconcile_watermarks 决定需要补入的增量区间")
          (step s3 "补写 conversation_messages 并推进 reconcile_watermarks, 把遗漏消息补齐")
          (step s4 "不依赖事件总线, 以 sweep 形式承担最终一致性修补"))
        (egress
          :writes ["conversation_messages" "reconcile_watermarks"]
          :reads ["reconcile_watermarks"]
          :via-bus []
          :memory-cross-ref ["conversation-logs" "system-support"]
          :returns "reconcile progress"))

      (path tagger-chunker-cycle
        :lifecycle-style spawned
        (ingress
          :source "SessionEvent::Organized"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/tagger_chunker.rs"])
        (logic-core
          (step s1 "消费已 organize 的 session, 先执行 analyze_messages(stage2): 噪声标签 / tool 分类 / commit detection")
          (step s2 "再执行 chunk_turns(stage3): turn 抽取 / tail trim / 持久化")
          (step s3 "从 conversation_messages 生成 message_labels 与 turns, 活跃 session 可返回 0 副作用")
          (step s4 "lagged / missed session 由 reconcile_tick(60s) 做补处理, 防止永久漏 turn"))
        (egress
          :writes ["message_labels" "turns"]
          :reads ["conversation_messages"]
          :via-bus ["SessionEvent::Organized"]
          :memory-cross-ref ["conversation-logs"]
          :returns "tagging / chunking result"))

      (path xjpcode-briefing-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "interval 60s (15s initial delay, MissedTickBehavior::Skip)"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs"])
        (logic-core
          (step s1 "周期性读取 running+failed 的 board_tasks、近期 incidents 与 projects")
          (step s2 "把这些状态汇总成 xjpcode CLI 启动时可直接消费的 briefing markdown")
          (step s3 "以原子写文件方式落到 ~/.xjpcode/xjpcode.md, 自身不写 DB")
          (step s4 "被 ControlTree pause/resume 治理, 但 durable ownership 仍在读取到的各 memory 模块"))
        (egress
          :writes []
          :reads ["board_tasks" "incidents" "projects"]
          :via-bus []
          :file-writes ["~/.xjpcode/xjpcode.md"]
          :memory-cross-ref ["board" "system-support" "project-management"]
          :returns "xjpcode briefing markdown"))

      (contract-summary
        :writes
          ["ast_files" "ast_nodes" "beacon_nodes"
           "conversations" "conversation_messages" "board_tasks"
           "tool_calls" "gemini_requests" "reconcile_watermarks"
           "slot_sessions" "deep_analysis_checkpoint" "message_labels"
           "incidents" "turns"]
        :reads
          ["ast_files" "beacon_nodes" "ast_nodes" "ast_search_hits" "kb_entries"
           "conversations" "board_tasks" "compaction_fragments" "tool_calls"
           "reconcile_watermarks" "slot_sessions" "conversation_messages"
           "incidents" "projects"]
        :file-writes ["~/.xjpcode/xjpcode.md"])))

  ;; ─────────────────────────────────────────────────────
  ;; 2.5 Engine Cluster — 与 worker-cluster 并列
  ;; ─────────────────────────────────────────────────────
  (section engine-cluster
    :relationship-to-worker-cluster "worker-cluster = concrete compute tenants; engine-cluster = orchestration / scheduling / analysis kernels"

    (subsection intent-engine
      :targets
        ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
         "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
         "crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs"
         "crates/missiond-daemon/src/engine/intent_engine/workflow_executor.rs"
         "crates/missiond-daemon/src/engine/intent_engine/gen_engine.rs"]
      :support-files ["crates/missiond-daemon/src/engine/intent_engine/gen_engine.rs — Forge shell, zero business logic"]

      (path autopilot-tick
        :lifecycle-style spawned
        (ingress
          :source "autopilot timer"
          :entry-components
            ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"])
        (logic-core
          (step s1 "memory-scheduler: 扫内部待推进 / reminder / pending queue")
          (step s2 "extraction-check: 检查 extraction / learning 相关执行态")
          (step s3 "board-task-dispatch: list open tasks → CAS claim → 选择 slot / worker")
          (step s4 "flow-progression: 推动 board_tasks 上挂的 flow 状态")
          (step s5 "supervision-check: lease recovery / stale task / zombie slot 回收"))
        (egress
          :writes ["board_tasks" "prompt_snapshots"]
          :reads ["board_tasks"]
          :via-bus ["BoardEvent::*" "WorkerEvent::*"]
          :memory-cross-ref ["board"]
          :returns "tick summary / dispatch actions"))

      (path board-phase-engine
        :lifecycle-style spawned
        (ingress
          :source "autopilot claim 后的 board task phase progression"
          :entry-components
            ["crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"])
        (logic-core
          (step s1 "按旧的 board lifecycle phase 把任务从 Investigate / Plan / Execute / Finalize 等阶段推进")
          (step s2 "在需要 slot 时请求 slot runtime, 在需要工具时委托 workflow / tool path")
          (step s3 "把阶段推进结果与失败状态回写 board_tasks"))
        (egress
          :writes ["board_tasks"]
          :reads ["board_tasks"]
          :via-bus ["BoardEvent::*"]
          :memory-cross-ref ["board"]
          :returns "board phase progression result"))

      (path memory-scheduler-queue
        :lifecycle-style spawned
        (ingress
          :source "autopilot tick / internal memory work queue"
          :entry-components
            ["crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs"])
        (logic-core
          (step s1 "扫描待执行 memory-side 任务与 reminder/queue state")
          (step s2 "决定哪些任务应被推进、延后或交给其他 worker / slot")
          (step s3 "把候选结果回给 autopilot 或 workflow 层, 自身偏调度而非数据 ownership"))
        (egress
          :writes []
          :reads ["board_tasks"]
          :via-bus []
          :memory-cross-ref ["board"]
          :returns "scheduled memory work candidates"
          :need-more-ground-truth "phase-A 材料只确认它读 board_tasks; 精确写路径待 phase-B 扫描"))

      (path workflow-executor-runtime
        :lifecycle-style on-demand
        (ingress
          :source "skill workflow step / engine-side tool dispatch"
          :entry-components
            ["crates/missiond-daemon/src/engine/intent_engine/workflow_executor.rs"])
        (logic-core
          (step s1 "接收 workflow step 定义或 skill execution 请求")
          (step s2 "按步骤调用 MCP 工具、slot 或其他 runtime primitive")
          (step s3 "把执行结果交回上层 engine / handler, 自身更像 runtime bridge 而不是 durable writer"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "workflow step result"
          :need-more-ground-truth "phase-A 材料未给出 workflow_executor 的精确表交互")))

    (subsection learning-engine
      :targets
        ["crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs"
         "crates/missiond-daemon/src/engine/learning_engine/decision_harvest.rs"
         "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
         "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"
         "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"
         "crates/missiond-daemon/src/engine/learning_engine/intent_analyst.rs"
         "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"
         "crates/missiond-daemon/src/engine/learning_engine/gen_engine.rs"]
      :support-files ["crates/missiond-daemon/src/engine/learning_engine/gen_engine.rs — domain shell"]

      (subsection decision
        (path decision-cascade
          :lifecycle-style spawned
          (ingress
            :source "agent question / decision need / escalation path"
            :entry-components
              ["crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs"])
          (logic-core
            (step s1 "识别一个问题是否可由 KB 直接解、需要 LLM 辅助、还是应升级给人类")
            (step s2 "按 kb-lookup → gemini-consult → decision-slot → human-escalation 的 cascade 做分流")
            (step s3 "把决策结论回给调用方或上层调度器"))
          (egress
            :writes []
            :reads []
            :via-bus []
            :returns "decision result"
            :need-more-ground-truth "phase-A 材料未给出 decision_engine 精确落表"))

        (path decision-harvest-generalization
          :lifecycle-style spawned
          (ingress
            :source "已发生的 decision / outcome 样本"
            :entry-components
              ["crates/missiond-daemon/src/engine/learning_engine/decision_harvest.rs"])
          (logic-core
            (step s1 "回看历史决策与结果")
            (step s2 "抽取可复用模式或 policy 候选")
            (step s3 "将模式化结果回送给知识 / 策略侧使用"))
          (egress
            :writes []
            :reads []
            :via-bus []
            :returns "harvested decision patterns"
            :need-more-ground-truth "decision_harvest 的 durable sink 未在 phase-A 材料中显式给出")))

      (subsection extraction
        (path extraction-pipeline
          :lifecycle-style spawned
          (ingress
            :source "session/event 已落地后的知识提取触发"
            :entry-components
              ["crates/missiond-daemon/src/engine/learning_engine/extraction.rs"])
          (logic-core
            (step s1 "从 conversation / event material 中抽取候选知识")
            (step s2 "执行快速提取与深度提取两阶段")
            (step s3 "把知识候选返回给上层消费链路"))
          (egress
            :writes []
            :reads []
            :via-bus []
            :returns "knowledge extraction candidates"
            :need-more-ground-truth "extraction.rs 的精确表目标需 phase-B 扫描"))

        (path historical-scan-backfill
          :lifecycle-style spawned
          (ingress
            :source "回溯扫描请求 / backlog catch-up"
            :entry-components
              ["crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"])
          (logic-core
            (step s1 "按时间窗口或 session backlog 扫历史会话")
            (step s2 "把旧数据补进 extraction / analysis 队列")
            (step s3 "将扫描结果交给后续提取器, 自身更偏 backfill driver"))
          (egress
            :writes []
            :reads []
            :via-bus []
            :returns "historical scan result"
            :need-more-ground-truth "historical_scanner 的 precise read/write contract 待 phase-B"))

        (path idle-explore-trigger
          :lifecycle-style spawned
          (ingress
            :source "空闲期触发"
            :entry-components
              ["crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"])
          (logic-core
            (step s1 "检测系统或 slot 的空闲窗口")
            (step s2 "在空闲窗口内触发 exploration / extraction / backfill 任务")
            (step s3 "把待探索对象交给后续 path, 自己只做时机治理"))
          (egress
            :writes []
            :reads []
            :via-bus []
            :returns "idle exploration triggers"
            :need-more-ground-truth "idle_explorer 的 downstream target 待 phase-B 扫描")))

      (subsection analysis
        (path intent-analysis
          :lifecycle-style spawned
          (ingress
            :source "session / turn 分析触发"
            :entry-components
              ["crates/missiond-daemon/src/engine/learning_engine/intent_analyst.rs"])
          (logic-core
            (step s1 "按 session 扫 conversation turns")
            (step s2 "用分析模型为一组 turns 识别 intent group")
            (step s3 "把 intent group 与 turn back-reference 写回 conversation 视角"))
          (egress
            :writes ["user_intents" "conversation_turns.intent_group_id"]
            :reads ["conversation_turns"]
            :via-bus []
            :memory-cross-ref ["conversation-logs"]
            :returns "intent analysis result"
            :note "本条来自 frozen memory pillar 的 cross-ref, 非本 brief 新发现"))

        (path timeline-analysis
          :lifecycle-style spawned
          (ingress
            :source "时间轴 / long-range sequence 分析触发"
            :entry-components
              ["crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"])
          (logic-core
            (step s1 "按时间顺序聚合会话 / 事件 / 任务序列")
            (step s2 "寻找长期模式、关键转折点与上下文压缩机会")
            (step s3 "把分析结果回给策略、记忆或展示侧"))
          (egress
            :writes []
            :reads []
            :via-bus []
            :returns "timeline analysis result"
            :need-more-ground-truth "timeline_analyst 的 precise table contract 待 phase-B"))))

    (subsection flow-engine
      :targets
        ["crates/missiond-daemon/src/engine/flow/loader.rs"
         "crates/missiond-daemon/src/engine/flow/runner.rs"
         "crates/missiond-daemon/src/engine/flow/handlers.rs"]
      :samples ["crates/missiond-daemon/src/engine/flow/examples/"]

      (path flow-definition-load
        :lifecycle-style on-demand
        (ingress
          :source "mission_flow_run(list/run/status) / board task with flow_template"
          :entry-components
            ["crates/missiond-daemon/src/engine/flow/loader.rs"])
        (logic-core
          (step s1 "从 $MISSIOND_HOME/flows/{flow_id}.yaml 载入 FlowDefinition")
          (step s2 "校验 YAML 能否解析成 node-based flow 定义")
          (step s3 "把定义交给 runner, 或在 list/status 场景直接返回元数据"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "FlowDefinition / available flows"))

      (path flow-node-handler-dispatch
        :lifecycle-style on-demand
        (ingress
          :source "runner 逐节点执行到某个具体 node 时"
          :entry-components
            ["crates/missiond-daemon/src/engine/flow/handlers.rs"])
        (logic-core
          (step s1 "按 node type 分派到 LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks")
          (step s2 "LlmCall 走 llm_gateway; SlotTask 走 running slot; McpTool 委托 dispatch_tool; DaemonAction 走内建 action")
          (step s3 "ParallelSlotTasks 负责 bounded parallelism + gather strategy, 保持 per-task receipt 的顺序")
          (step s4 "未知 provider / 未运行 slot / 未知 action 直接 fail-fast, 不做隐式 fallback"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "per-node execution result"))

      (path flow-runner-persist
        :lifecycle-style on-demand
        (ingress
          :source "FlowDefinition 已加载, runner 开始执行"
          :entry-components
            ["crates/missiond-daemon/src/engine/flow/runner.rs"])
        (logic-core
          (step s1 "runner 按 node 顺序执行, 做变量插值与 completed_nodes 追踪")
          (step s2 "每个 node 完成后 persist_context 到 board_tasks.flow_context")
          (step s3 "遇错时把 error 写进 ctx.last_error 并持久化, 由 error policy 决定 stop / skip / retry")
          (step s4 "flow 完成后由上层 handler 把 flow_phase/status 更新为 completed 或 failed"))
        (egress
          :writes ["board_tasks.flow_context" "board_tasks.flow_phase" "board_tasks.status"]
          :reads ["board_tasks"]
          :via-bus []
          :memory-cross-ref ["board"]
          :returns "flow runtime result / persisted context"))))

  ;; ─────────────────────────────────────────────────────
  ;; 2.6 Orchestration / Governance
  ;; ─────────────────────────────────────────────────────
  (section orchestration-governance
    :targets
      ["crates/missiond-daemon/src/workers/registry.rs"
       "crates/missiond-daemon/src/control_tree.rs"
       "crates/missiond-daemon/src/main.rs"
       "crates/missiond-daemon/src/state.rs"]

    (path worker-kind-registration
      :lifecycle-style bootstrap
      (ingress
        :source "daemon bootstrap / worker registry construction"
        :entry-components
          ["crates/missiond-daemon/src/workers/registry.rs"])
      (logic-core
        (step s1 "BackgroundWorker trait 定义 KIND / name / extra_deps / run, 把 worker 可治理化")
        (step s2 "spawn_worker 依据 KIND 自动注入 provider dependency, 强化目录结构就是 contract 的原则")
        (step s3 "注册表产出 WorkerHandle / WorkerInfo / WorkerContext, 供 runtime 和工具面查询")
        (step s4 "后续 worker lifecycle 均经此处纳入统一治理, 而不是各自裸 tokio::spawn"))
      (egress
        :writes []
        :reads []
        :via-bus []
        :returns "worker registry / handles / provider dependency injection"))

    (path pause-resume-cascade
      :lifecycle-style runtime-control
      (ingress
        :source "mission_worker / control mutation / debug override / bootstrap restore"
        :entry-components
          ["crates/missiond-daemon/src/control_tree.rs"])
      (logic-core
        (step s1 "ControlTree 保存 global / provider / domain / worker / slot_role / project 六层控制状态")
        (step s2 "is_effectively_paused() 以 worker override > global > provider/domain 的优先级生效")
        (step s3 "ControlManager 用 tokio::watch::channel 推送更新, worker 在 select! 里零轮询响应")
        (step s4 "变更写入 control_tree.json, 供崩溃恢复和审计查看"))
      (egress
        :writes []
        :reads []
        :via-bus []
        :file-writes ["control_tree.json"]
        :returns "pause/resume status summary"))

    (path daemon-bootstrap-spawn-order
      :lifecycle-style bootstrap
      (ingress
        :source "daemon startup"
        :entry-components
          ["crates/missiond-daemon/src/main.rs"
           "crates/missiond-daemon/src/state.rs"])
      (logic-core
        (step s1 "按 infrastructure → project_registry → pty_manager → slot_manager → gateways → context_pipeline → worker_registry/control_tree → workers → engines → ipc/ws 的顺序启动")
        (step s2 "state.rs 把 project_registry 等共享态装进 AppState, 为 message handler / slot / context 提供共用依赖")
        (step s3 "main.rs 实际 spawn 17 个 worker + event-bus subscribers + autopilot 等 runtime 组件")
        (step s4 "phase-A 只锁定拓扑与顺序, phase-B 再逐个 binds-to 校验 spawn 点"))
      (egress
        :writes []
        :reads ["projects"]
        :via-bus []
        :memory-cross-ref ["project-management"]
        :returns "booted runtime topology"
        :need-more-ground-truth "worker 数 / spawn 位点随 main.rs 演化, phase-B 需再扫一遍")))

  ;; ─────────────────────────────────────────────────────
  ;; 2.7 Worker-side Computation — retrieval / forge
  ;; ─────────────────────────────────────────────────────
  (section worker-side-computation
    (path retrieval-fusion
      :lifecycle-style on-demand
      (ingress
        :source "mission_kb_search / mission_memory / mission_code_search / context assembly"
        :entry-components
          ["crates/missiond-daemon/src/context/context_pipeline.rs"
           "crates/missiond-daemon/src/handlers/knowledge/kb.rs"
           "crates/missiond-daemon/src/workers/local/code_prefetch.rs"])
      (logic-core
        (step s1 "并发跑 vector-hnsw / fulltext-gin / fuzzy-trigram / tag-exact 四路检索")
        (step s2 "叠加 project_id OR NULL 的作用域过滤")
        (step s3 "把代码检索(code_prefetch) 与 KB / history 检索结果做融合打分")
        (step s4 "向调用方返回 ranked candidates / snippet / source trace, 不自己拥有 durable write"))
      (egress
        :writes []
        :reads ["kb_entries" "ast_nodes" "beacon_nodes" "ast_search_hits" "conversations" "conversation_messages"]
        :via-bus []
        :memory-cross-ref ["kb-manager" "conversation-logs"]
        :returns "ranked retrieval results"
        :need-more-ground-truth "实际 fusion ranker 的具体实现路径需 phase-B file scan; context/retrieval.rs 并不存在, 检索融合由 context_pipeline + code_prefetch + handlers/knowledge/kb 三件协作"))

    (path forge-build-bridge
      :lifecycle-style on-demand
      (ingress
        :source "mission_forge_build / mission_forge_lint"
        :entry-components
          ["crates/missiond-daemon/src/handlers/compute/forge.rs (inferred from handler namespace)"
           "external jarvis-forge CLI"])
      (logic-core
        (step s1 "按 project_id 通过 ProjectRegistry 定位项目根目录")
        (step s2 "shell out 到 forge build 或 forge lint, 支持 FORGE_BIN env override")
        (step s3 "捕获 stdout / stderr / exit_code / violations_raw")
        (step s4 "把结果作为 compute tool response 返回, 必要时触发 survey / intent-layer follow-up"))
      (egress
        :writes []
        :reads ["projects"]
        :via-bus []
        :memory-cross-ref ["project-management"]
        :returns "forge execution result"
        :need-more-ground-truth "phase-B 需确认 compute::forge handler 的真实文件路径是否与 namespace 推断一致")))

  (need-more-ground-truth
    (item 1 "slot_manager/ 目录已不存在, 实际合入 slot_orchestrator/; phase-B 需确认任何残留引用的清理状态")
    (item 2 "workflow_executor.rs 的表级 R/W 契约")
    (item 3 "learning_engine 除 intent_analyst 外的 precise table contract")
    (item 4 "retrieval-fusion 的真实文件绑定 (context/retrieval.rs 并不存在, 由 context_pipeline + code_prefetch + handlers/knowledge/kb 协作)")
    (item 5 "experience_harvester.rs 是 planned 功能还是将被删除的 prototype"))
)