;; MissionD v2 — Event Bus Execution Log
;; 与 intent-event-bus.lisp 配对使用 — 架构 lisp 只读不改,本文件记录执行期偏差/问题/决策
;; 作为多 agent 并行的共享内存层 — 新 agent 入场先读本文件了解进度与锁

;; ══════════════════════════════════════════════════════
;; 规则:
;;   1. 任何 agent 开工前须在 (claims) 下登记 claim,完成或中止时 release
;;   2. 若实际实现偏离 frozen lisp,在 (deviations) 记录 — 不改 frozen lisp
;;   3. 遇到阻塞/未决问题进 (issues),带 severity 和 resolution
;;   4. 执行期做出的次要决策(frozen lisp 未覆盖)进 (decisions)
;;   5. 每阶段完成后追加 (completions) 条目,含 deliverables 和 verification 证据
;; ══════════════════════════════════════════════════════

(execution-log
  (meta
    :parent       "intent-event-bus.lisp"
    :branch       "refactor/event-bus-v2"
    :started      "2026-04-19"
    :status       "in-progress"
    :phase-cursor 6)

  ;; ─ 阶段追踪 ─
  (phases
    (phase-0 :status "completed" :owner "phase0-survey" :started "2026-04-19" :completed "2026-04-19"
             :summary "Inventory of v1 bus surface: 52 variants, 83 publish points, 14 subscribers, 4 MPSC bypasses, 1 central timeline writer. 7 top risks flagged.")
    (phase-1 :status "completed" :owner "phase1-schema" :started "2026-04-19" :completed "2026-04-19"
             :summary "schema 层落地 — 12 domain enum + DomainEvent trait,共 55 个 variant 覆盖 49 个 v1 DaemonEvent + 6 个新 variant(1 Slot::Stuck 占位 + 3 Observability + 2 Incident)。模块位于 crates/missiond-core/src/event/,与旧 event_bus.rs 并存。45 unit tests 全部 pass。I001 9 个遗漏 variant 全部归属(见 DC001)。")
    (phase-2 :status "completed" :owner "phase2-storage" :started "2026-04-19" :completed "2026-04-19"
             :summary "storage 层落地 — Log trait + LogWriter 任务 + BlobStore claim-check + 3 个新 migration。30 个新 unit test 覆盖 backpressure/dedup collision/batch flush/failed state/claim-check redirect/checksum roundtrip。6 个 integration test 骨架 (#[ignore],需要 Docker 才跑)。代码位于 crates/missiond-core/src/event/{log,blob_store}/,与 v1 run_timeline_writer / system_timeline 完全共存。")
    (phase-3 :status "completed" :owner "phase3-routing" :started "2026-04-19" :completed "2026-04-19"
             :summary "routing 层落地 — Dispatcher + Topic<T> + TopicRegistry + 长轮询 tail loop + control-gate。代码在 crates/missiond-core/src/event/dispatcher/{mod,topic,registry,tail,control_gate}.rs。32 个新 unit test 覆盖:12 域注册/type 查询/broadcast fan-out/慢订阅者 Lagged 不传染/Domain→CtlDomain 映射总体完整/paused Memory 只阻 Memory/Observability+Incident 不受 pause/mock tail 100 条严格 seq 顺序 + cursor 单调/bad payload drop 不 crash/tail source error 上浮。2 个 integration test 骨架(#[ignore],同样需要 Docker)。I002 resolved: Domain→CtlDomain 映射函数位于 control_gate.rs;只 Memory/Board 映射,其他 10 域默认不 gate。I003 resolved: paused-domain 默认 drop 已是实现,Observability/Incident 永不 gated(WS 独立 Phase 7)。")
    (phase-4 :status "completed" :owner "phase4-subscription" :started "2026-04-19" :completed "2026-04-19"
             :summary "egress 层落地 — SubscriptionOpts + 三 FailurePolicy + 两 PauseBehavior + 6 combinators + tail-and-pull lifecycle + 双阈值 flush。代码在 crates/missiond-core/src/event/subscription/{api,mod,options,cursor_store,failure,lifecycle,combinators}.rs。40 个新 unit tests:options 10(enum round-trip + backoff + default)、cursor_store 5(in-memory CRUD)、failure 5(Retry/DLQ/Halt)、lifecycle 6(bootstrap 顺序/batch size/ack 单调/live 去重/越界过滤)、subscription core 2(ack/drop 语义)、api 5(bootstrap flush/resume/empty name/StartFrom×3)、combinators 7(filter/map/debounce/coalesce/rate_limit/batch×2)。3 个 integration test 骨架(#[ignore])覆盖 100 条全流程 + crash-recovery + DLQ 验证。引入 LogReadable trait 解决 Log 的 dyn 不兼容(泛型 append)。D001 deviation: FreezeAndCatchUp 推迟到未来实现,当前 alias 到 DropAndLiveResume。I002/I003/I008 related:dispatcher ControlGate 已在 Phase 3 处理暂停语义,subscription MVP 不感知。")
    (phase-5 :status "completed" :owner "phase5-cross-cutting" :started "2026-04-19" :completed "2026-04-19"
             :summary "横切面层落地 — causation guard + BusMetrics trait + AtomicBusMetrics + ObservabilityEvent emitter + InMemoryBus 全套 + 9 chaos scenarios。代码在 crates/missiond-core/src/event/{guards,metrics,in_memory}/**。34 个新 lib unit tests (5 causation + 7 metrics counters + 3 emitter + 4 control_gate + 3 blob_store + 8 in_memory_log + 4 in_memory_bus) + 12 chaos integration tests 在 tests/event_chaos.rs。LogWriter.append 改调 check_causation() 统一 guard 入口。InMemoryLog impl Log:seq 用 AtomicI64 fetch_add、bounded 4096 channel、dedupe map、failed state 可切换、ephemeral 分 seq 不持久化、claim-check 走 BlobStore — 与 PG 版全面同语义。Dispatcher 与 InMemoryLog 通过 InMemoryTailSource 解耦,共享 Phase 3 run_tail 实现,无代码分叉。BusMetricsEmitter 每 10s snapshot 转 ObservabilityEvent::BusMetric(ephemeral=true)追加到 log。D002 deviation:Prometheus 后端推迟(MVP 仅 AtomicBusMetrics stub)。I010 issue:cursor-orphan daily cron wire up 推迟到 Phase 8。")
    (phase-6 :status "completed" :owner "phase6-producer-migration" :started "2026-04-19" :completed "2026-04-19"
             :summary "Producer 迁移 — daemon 侧 bus/{mod,bootstrap,compat,control_gate_adapter}.rs 落地 + AppState.bus 字段 + main.rs 启动钩子(PG 特性开启后 bootstrap + start 注入 dispatcher/metrics/observability emitter)+ ControlTreeGate 适配 watch::Receiver<ControlTree> → ControlGate trait(DC010 形式)。所有 83 个 v1 publish 点全部双发:保留 event_bus.publish(...) + 新增 let _ = crate::bus::publish_v1_shim(&state.bus, &ev).await(async 上下文)或 spawn_v1_shim(detached)(同步 helper 如 pty_event_worker::handle_mcp_tool_error)。7 个 LLM-internal send_tx 点(gemini_client/codex_cli/sonnet_gateway/minimax_gateway)各自挂 Option<Arc<BusServices>> + with_bus() + .shim() 辅助,保证 CliRequest*/WorkerLlmCall 也双发。4 条 MPSC bypass 的 sender 端选择性双发:incident_tx 加 bus.publish_incident(IncidentEvent::Reported) — IncidentEvent 已在 Phase 1 枚举中。embedding_tx / ast_sync_tx sender 保留纯 MPSC(D003 deviation: EmbeddingEvent / AstSyncEvent 不在 12 域枚举中;新增会破坏 §4.2.a 契约,推迟到 Phase 7 或重新评估)。cursor_ack_tx 不动(I005 保留 Phase 7)。LEGACY Gemini*/Codex* variants 透传进 LlmEvent::LegacyXxx(DC004 保留)。workspace 编译通过 + missiond-core 250 lib tests + 12 chaos tests + missiond-daemon 91 unit tests 全部 PASS。未触 v1 DaemonEvent enum / run_timeline_writer / event_router — Phase 7/8 再处理。")
    (phase-7 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-8 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-9 :status "pending" :owner nil :started nil :completed nil :summary nil))

  ;; ─ 并行锁表(防 agent 冲突) ─
  ;; 格式: (claim :phase N :scope "path/description" :agent "name" :claimed-at "..." :released-at "..."|nil)
  (claims
    (claim :phase 4 :scope "crates/missiond-core/src/event/subscription/**" :agent "phase4-subscription"
           :claimed-at "2026-04-19T00:00:00Z" :released-at "2026-04-19T00:00:00Z")
    (claim :phase 5 :scope "crates/missiond-core/src/event/{guards,metrics,in_memory}/** + tests/event_chaos.rs" :agent "phase5-cross-cutting"
           :claimed-at "2026-04-19T00:00:00Z" :released-at "2026-04-19T00:00:00Z")
    (claim :phase 6 :scope "crates/missiond-daemon/src/bus/** + AppState.bus + all 83 v1 publish sites + 7 LLM internal sends" :agent "phase6-producer-migration"
           :claimed-at "2026-04-19T00:00:00Z" :released-at "2026-04-19T00:00:00Z")
    )

  ;; ─ 偏离 frozen lisp 的记录 ─
  ;; 格式: (deviation :id D001 :phase N :date "..." :agent "name"
  ;;                  :lisp-said "引用原 lisp 的决策"
  ;;                  :actually-did "实际实现"
  ;;                  :reason "为什么偏离"
  ;;                  :approved-by "user|auto|agent-consensus")
  (deviations
    (deviation :id D001 :phase 4 :date "2026-04-19" :agent "phase4-subscription"
               :lisp-said "PauseBehavior::FreezeAndCatchUp opt-in:paused 期间 cursor 冻结不前推;resume 时 subscription 触发 pull catch-up(batch_size 节流)"
               :actually-did "Phase 4 MVP 仅实现 DropAndLiveResume(默认);FreezeAndCatchUp 变体在 API 存在,但运行时行为等同 DropAndLiveResume"
               :reason "FreezeAndCatchUp 需要与 ControlGate 状态机双向协作(订阅轮询 paused 状态切断 live + resume 主动 catch-up)。Phase 4 MVP 首要目标是落地 tail-and-pull + cursor + combinators + failure policy;Freeze 语义与现有 Dispatcher-gated drop 语义在实际行为上极其接近(paused 期间 Dispatcher 不 fan-out ⇒ 订阅者天然等在 live 的 recv().await,resume 时自动恢复)。I009 追踪未来补齐"
               :approved-by "auto")

    (deviation :id D002 :phase 5 :date "2026-04-19" :agent "phase5-cross-cutting"
               :lisp-said "observability / bus self-emission 应能向 Prometheus-兼容 backend 暴露 append_rate / reject_rate / dispatch_lag / topic_depth / subscription_lag 等计数"
               :actually-did "Phase 5 仅提供 BusMetrics trait + AtomicBusMetrics 内存实现 + emitter 周期 snapshot → ObservabilityEvent::BusMetric(ephemeral);无 Prometheus HTTP 导出器"
               :reason "Phase 5 核心交付是 causation guard + InMemoryBus + 9 chaos scenarios。Prometheus 后端属于生产运维集成层(Phase 8 wiring 或独立 observability phase)。目前 AtomicBusMetrics snapshot → ObservabilityEvent 的管道已可被任何 consumer 订阅读取,实际 backend 可后续替换不动 BusMetrics trait。"
               :approved-by "auto")

    (deviation :id D003 :phase 6 :date "2026-04-19" :agent "phase6-producer-migration"
               :lisp-said "frozen lisp §4.1 dead-bypass 列出 embedding-tx / ast-sync-tx 应改走 log.append(EmbeddingEvent::Requested) / log.append(AstSyncEvent::Requested);Phase 6 任务说'sender 端加 log.append(EmbeddingEvent/AstSyncEvent)'"
               :actually-did "Phase 6 未添加 log.append 镜像。仅 incident_tx sender 加了 bus.publish_incident(IncidentEvent::Reported) 双发(IncidentEvent 在 Phase 1 12 域枚举内);embedding_tx / ast_sync_tx 的 sender 保持 MPSC-only,不做双发"
               :reason "frozen lisp §4.2.a 只定义了 12 个 domain enum(Slot/Board/Task/Question/Llm/Worker/Memory/Message/Session/System/Observability/Incident),Phase 1 未创建 EmbeddingEvent / AstSyncEvent。新增 2 个域会破坏 12 域 compile-time 契约(Domain::ALL + TopicRegistry + 所有 chaos/unit tests 都需同步)。MPSC receiver 侧还未重写(Phase 7 任务),dual emit 也没有 subscriber 可拿。Phase 7 subscriber 迁移时应重新评估:(a) 新增 2 个域 + 补 chaos 测试(重代价);(b) 将 embedding/ast-sync 归入现有 MemoryEvent / SystemEvent 的新 variant(轻代价);(c) 保持 MPSC 作为 worker-internal channel 不进总线(与 cursor-ack 同策略)。"
               :approved-by "auto"))

  ;; ─ 执行期阻塞/未决问题 ─
  ;; 格式: (issue :id I001 :phase N :date "..." :severity blocker|major|minor
  ;;              :desc "问题描述"
  ;;              :resolution "解决方案或 TODO"
  ;;              :resolved-at "..."|nil)
  (issues
    (issue :id I001 :phase 1 :date "2026-04-19" :severity major
           :desc "frozen lisp §4.2.a 的 12 个 domain-enum 示例 variant 列表不完整。survey 发现 9 个实际存在的 DaemonEvent variant 未列出:DeepAnalysisCompleted / KBBatchMutated / SessionOrganized / TurnExtracted / IntentAnalyzed / JarvisProactivePush / ContextualCommitDetected / CascadeTriggered / CascadeCompleted。仍可映射入 12 域(见 inventory §1),但需在 Phase 1 显式决策每个 variant 的归属域"
           :resolution "Phase 1 定义 domain enums 时补齐 9 个遗漏 variant。不修改 frozen lisp,在 decisions 记录每条映射。见 DC001。"
           :resolved-at "2026-04-19")
    (issue :id I002 :phase 3 :date "2026-04-19" :severity major
           :desc "frozen lisp §4.2.c control-gate 说'暂停域不进 topic',但 v1 CtlDomain 只有 4 值(Memory/Flow/Board/Strategy),v2 Domain 有 12 值。Dispatcher 如何映射 Domain→CtlDomain 未规定"
           :resolution "Phase 3 在 crates/missiond-core/src/event/dispatcher/control_gate.rs 定义 domain_to_ctl_domain() 多对一映射 + ControlGate trait(抽象,避免 core ← daemon 循环依赖)。Memory→Memory, Board→Board, 其余 10 域(Slot/Task/Question/Llm/Worker/Message/Session/System/Observability/Incident)返回 None ⇒ 永不 gate。Daemon 侧 Phase 8 提供 Adapter: impl ControlGate for watch::Receiver<ControlTree>。见 DC010+。"
           :resolved-at "2026-04-19")
    (issue :id I003 :phase 7 :date "2026-04-19" :severity major
           :desc "控制闸语义变化风险:v1 paused domain 仍经过 Timeline Writer 入库并广播,consumer 自行 no-op;v2 paused domain 的事件不再 fan-out 给 subscriber。前端若依赖'暂停时仍能看到事件'会 break"
           :resolution "Phase 3 验证: Dispatcher 实现对 paused domain 默认 drop 已是 frozen lisp §4.2.c 正向契约(paused=true 时跳过该 domain 的所有投递)。事件仍 persist 到 event_log,订阅端自己做 live-resume。ObservabilityEvent/IncidentEvent 永远 domain_to_ctl_domain = None ⇒ 永不受 pause 影响(§4.4 bus self-emission)。Phase 7 WS 层若需继续在暂停时向前端发事件,可让 frontend_events_tx 作为独立 subscriber 绕过 Dispatcher control-gate(不在 Phase 3 范围)。"
           :resolved-at "2026-04-19")
    (issue :id I004 :phase 6 :date "2026-04-19" :severity blocker
           :desc "前端 WS wire-format 契约耦合:47-ish wire_type 字符串 + 固定 JSON envelope (type/seq/trace_id/span_id/parent_span_id/payload) 由外部浏览器 client 消费。修改字段名/去掉字段会静默 break。sync 协议进一步耦合 timeline_latest_seq + query_timeline_since 两个 DB API"
           :resolution "Phase 6-7 上线需保持 wire envelope 不变或加版本字段,老 system_timeline 查询 API 在新 event_log 上 alias (view 或 query 重写),不能一次性 cut-over"
           :resolved-at nil)
    (issue :id I005 :phase 2 :date "2026-04-19" :severity minor
           :desc "cursor_ack_tx 的去向:frozen lisp §4.1 dead-bypass 说归 conversation-logger worker 内部,不作为 event。但 producer (conversation_logger:58) 和 consumer (main.rs:846-858) 分居两个 task,需要重构合并,不能只做 bus 迁移"
           :resolution "Phase 2 代码重构:cursor_ack 的 send + receive 合并到 ConversationLoggerWorker 自己的 run loop 内部,删除 UnboundedChannel"
           :resolved-at nil)
    (issue :id I006 :phase 1 :date "2026-04-19" :severity minor
           :desc "AST 同步触发链路不完整:ast_sync_tx 的唯一外部 producer 是 main.rs:1298 的启动时 FullSync。commit 级增量同步在代码中未接入,ContextualCommitDetected 也未 wire 到 CommitSync"
           :resolution "Phase 1-2 借 AstSyncEvent::Requested 迁移时,同时补 ContextualCommitDetected → AstSyncEvent::CommitSync 的 consumer 逻辑(新功能,非单纯迁移)"
           :resolved-at nil)
    (issue :id I007 :phase 3 :date "2026-04-19" :severity minor
           :desc "ephemeral 语义从 per-variant 硬编码 (is_ephemeral()) 改为 per-call (AppendOpts.ephemeral) 时,83 个 publish 点需要逐个审计。当前 8 个 ephemeral variant 多数会保留 ephemeral,但有些 (ImageMessageInserted, NarrationBatchCompleted) 实际 payload 较小,可能应转持久化"
           :resolution "Phase 3 迁移时,默认保持 v1 ephemeral 语义;后续按实际观察调整,非本次重构阻塞"
           :resolved-at nil)
    (issue :id I008 :phase 4 :date "2026-04-19" :severity minor
           :desc "Log trait 的 append<E> 方法阻止 dyn-compatibility,因此 subscription 无法直接 Arc<dyn Log>。Phase 4 引入了只读子 trait LogReadable 并给它一个 blanket impl over Log,使 Arc<dyn LogReadable> 可用于 subscription 运行时。生产者继续用具体 Log(含泛型 append);订阅者用 LogReadable"
           :resolution "已通过 LogReadable split 解决;不是 frozen lisp 偏差,而是实现分层。可选优化:将来 async-trait 支持 dyn with generic methods 后可统一"
           :resolved-at "2026-04-19")
    (issue :id I009 :phase 4 :date "2026-04-19" :severity minor
           :desc "D001: FreezeAndCatchUp 未在 MVP 实现。需要 Subscription 持有 ControlGate 状态观察,paused 时切断 live 并主动轮询;resume 时 batch_size 节流 catch-up 到 head"
           :resolution "未来按需补齐:Subscription::new 接受可选 ControlGate 引用 + 新增 phase Frozen + resume 触发 bootstrap replay loop。优先级低:DropAndLiveResume 覆盖 95% 场景"
           :resolved-at nil)
    (issue :id I010 :phase 8 :date "2026-04-19" :severity minor
           :desc "cursor-orphan cleanup daily cron 未 wire。Frozen lisp §4.3 orphan-cleanup 要求 last_seen_at > 30 天的 cursor 被归档 + 发 IncidentEvent::StaleSubscription。Phase 5 只在 chaos_9 验证数据模型支持 stale 检测(predicate query)"
           :resolution "Phase 8 daemon 启动时 spawn 一个每日 tick task:SELECT FROM event_subscriptions WHERE last_seen_at < now() - '30 days' → DELETE + 对每行 append IncidentEvent::Reported。当前 Phase 5 InMemoryCursorStore 无 TTL 字段遮蔽,PG 侧 event_subscriptions 表已有 last_seen_at 列供 cron 查询"
           :resolved-at nil))

  ;; ─ frozen lisp 未覆盖的次要决策 ─
  ;; 格式: (decision :id DC001 :phase N :date "..." :topic "..."
  ;;                 :options (opt-a opt-b opt-c)
  ;;                 :chose opt-x
  ;;                 :rationale "...")
  (decisions
    (decision :id DC001 :phase 1 :date "2026-04-19"
              :topic "I001 9 个遗漏 variant 归属域映射"
              :chose ("DeepAnalysisCompleted  → MemoryEvent::DeepAnalysisCompleted"
                     "KBBatchMutated         → MemoryEvent::KBBatchMutated"
                     "TurnExtracted          → MemoryEvent::TurnExtracted"
                     "IntentAnalyzed         → MemoryEvent::IntentAnalyzed"
                     "SessionOrganized       → SessionEvent::Organized"
                     "JarvisProactivePush    → SystemEvent::JarvisProactivePush"
                     "ContextualCommitDetected → SystemEvent::ContextualCommitDetected"
                     "CascadeTriggered       → TaskEvent::CascadeTriggered"
                     "CascadeCompleted       → TaskEvent::CascadeCompleted")
              :rationale "MemoryEvent 承接所有 KB/turn/intent 级记忆状态变更;SessionEvent 只管 session 生命周期转移(organized 是 S2 完成态);SystemEvent 接收系统级信号(主动推送 + git 提交);CascadeEvent 不独立成域以保 12 域契约,而是折入 TaskEvent(cascade 本质是一种专化的 task 生命周期)")

    (decision :id DC002 :phase 1 :date "2026-04-19"
              :topic "serde 枚举标签策略"
              :chose "externally tagged (serde 默认,无 #[serde(tag = ...)]) "
              :rationale "最初用 #[serde(tag = \"kind\")] 与 SystemEvent::ConfigChanged.kind 字段名冲突(rustc: variant field name `kind` conflicts with internal tag)。externally tagged JSON 形如 {\"VariantName\":{...}} — 兼容性完全够,Phase 2 shim 只需 serde_json::Value 级映射,不依赖内部标签。")

    (decision :id DC003 :phase 1 :date "2026-04-19"
              :topic "Provider 标签定义位置"
              :chose "LlmEvent 自带 Provider enum(Sonnet/Codex/Gemini/Claude),独立于 missiond-core::types::CliEngine"
              :rationale "CliEngine 只有 ClaudeCode/ClaudeMd 两值(gen_types.rs:337),语义是'CLI engine for PTY slots',与 LLM backend 不匹配。Provider 专表 LLM 后端身份,可单独演化。")

    (decision :id DC004 :phase 1 :date "2026-04-19"
              :topic "LEGACY Gemini*/Codex* variant 处理"
              :chose "保留为独立 variant(LegacyGeminiRequestStarted 等),不 deprecate"
              :rationale "frozen lisp 未让我们立即抛弃 legacy 语义,Phase 6 才会折叠。保留完整字段确保 v1 DB 行 round-trip 可行(Phase 2 shim 必须无损双向映射)。Phase 6 会引入 From impl 把 legacy 折叠入 RequestStarted { provider: Gemini/Codex, .. }。")

    (decision :id DC005 :phase 2 :date "2026-04-19"
              :topic "Phase 2 对 I005 cursor_ack_tx 的态度"
              :chose "不动 conversation_logger,Log trait 不提供 cursor_ack 相关 API,等 Phase 7 subscriber 迁移时由 ConversationLoggerWorker 自行处理"
              :rationale "Phase 2 范围严格限定在 storage(log + blob + migration),touch conversation_logger 会越界进入 subscriber refactor。I005 的 resolution 保持'Phase 7 处理'不变;frozen lisp §4.1 dead-bypass 已明确 cursor_ack 不作为 event,Phase 2 的 Log trait 无需为其设计任何入口。")

    (decision :id DC006 :phase 2 :date "2026-04-19"
              :topic "Blob backend 默认选择"
              :chose "PgBlobStore 作为默认 BlobStore,LocalFileBlobStore 作为可选后端;两者共享 trait,由 daemon 构造时选择"
              :rationale "frozen lisp §4.2.b claim-check.backends 明确 blob-table 为默认(同 DB 一致性/备份简单)、local-file 为可选(>1MB payload)。Phase 2 只提供两个实现和 trait,daemon 侧在 Phase 8 启动时根据配置挑选,不在 core 层硬编码。")

    (decision :id DC007 :phase 2 :date "2026-04-19"
              :topic "PayloadRef 的 on-wire 编码"
              :chose "PayloadRef 序列化为 JSON 字符串写入 event_log.payload_ref TEXT 列,checksum 以 hex 而非 bytes 呈现"
              :rationale "TEXT 列比自定义 BYTEA+长度前缀可读、可在 psql 中直接检查,故序列化时 checksum hex-encode。Python 甚至 ops 脚本只用 serde_json 即可解码,不依赖 Rust。")

    (decision :id DC008 :phase 2 :date "2026-04-19"
              :topic "PG INSERT 批量策略"
              :chose "同一 tx 内逐行 INSERT RETURNING seq,而非多值 INSERT"
              :rationale "多值 INSERT 在 UUID/JSONB 含 NULL 混合时 sqlx 绑定参数数组困难;batch 上限 100 行,一次 tx 内逐行的成本在百毫秒级,可接受。若 observed QPS >10k 再优化为 COPY IN 或 unnest 批量。")

    (decision :id DC009 :phase 2 :date "2026-04-19"
              :topic "Ephemeral fast-path Seq 分配"
              :chose "进程内 AtomicI64 从 -1 递减,只作占位提示"
              :rationale "Phase 2 dispatcher 未上线,ephemeral 路径暂无消费者。给出负 seq 让前端/metric 能区分 persistent vs volatile;Phase 3 dispatcher 接管后真实 seq 由它分配(可能通过 in-memory bus 单 writer 分 seq)。不让 ephemeral append 占用 DB BIGSERIAL 避免 seq 空洞。")

    (decision :id DC010 :phase 3 :date "2026-04-19"
              :topic "ControlGate 抽象 vs 直用 ControlTree"
              :chose "在 dispatcher/control_gate.rs 定义 ControlGate trait + CtlDomain(duplicated 枚举),不在 missiond-core 引入 missiond-daemon 依赖"
              :rationale "ControlTree/CtlDomain 定义在 missiond-daemon,而 dispatcher 必须在 missiond-core(event_log/topic 都在 core)。core ← daemon 是反向依赖,不能直接 import。方案:core 侧复制 4-bucket CtlDomain enum + 定义 ControlGate trait,daemon 侧 Phase 8 写 Adapter 把 watch::Receiver<ControlTree> 实现成 ControlGate。变化小、零循环依赖、测试可以用 NeverPaused 或 mock impl。")

    (decision :id DC011 :phase 3 :date "2026-04-19"
              :topic "Tail 机制:长轮询 vs PG LISTEN/NOTIFY"
              :chose "Phase 3 只实现长轮询(每 100ms SELECT WHERE seq > last_dispatched LIMIT 256);PG LISTEN/NOTIFY 留作未来优化"
              :rationale "frozen lisp §4.2.c 允许两选一。长轮询简单可靠、与 sqlx 原生配合、无需 PG extension、mock 容易(MockTailSource)。100ms 对 MissionD 单机场景完全够用(典型事件率 <100/s)。LISTEN/NOTIFY 需要一个 LISTEN task + Notification Channel + fallback 逻辑,复杂度远高于价值。若未来 QPS >1k 或需要 <10ms dispatch lag,再引入。")

    (decision :id DC012 :phase 3 :date "2026-04-19"
              :topic "last_dispatched_seq 持久化策略"
              :chose "只用进程内 AtomicI64,不持久化;重启视为从 0 开始"
              :rationale "frozen lisp §4.2.c scope-invariant:'Dispatcher 不替离线 consumer 扫库,不维护 per-subscription 状态'。Dispatcher 重启后从 0 开始扫,会把已存在的历史事件重新扫一遍但不 fan-out(订阅者此时未连,broadcast 无 receiver),然后追上 head。订阅者的 cursor 才是真相来源(Phase 4 event_subscriptions 表)。若未来发现重复扫的 I/O 成本显著,可在 system_config 或新增 dispatcher_state 表存 last_dispatched_seq;Phase 3 不做。")

    (decision :id DC013 :phase 3 :date "2026-04-19"
              :topic "Tail SQL:跨域 vs 分域"
              :chose "单次查询跨所有 12 域:SELECT WHERE seq > $1 ORDER BY seq LIMIT 256"
              :rationale "frozen lisp §4.2.c tail-mechanism 要求'严格按 seq 升序派发'。按 seq 全局跨域扫最符合这个契约,O(1) state。Phase 2 的 LogReader::read_from 是 per-domain 的(给订阅者 catch-up 用),此处 Dispatcher 需要跨域,所以 tail.rs 新写了 PgTailSource 直接查表。idx_event_log_domain_seq 已有;未来若需加速可加 idx_event_log_seq BTREE(seq) — 但 BIGSERIAL PK 本就是 BTREE,当前索引已够用。")

    (decision :id DC014 :phase 3 :date "2026-04-19"
              :topic "Bad row 处理策略(unknown domain / payload deser fail)"
              :chose "log WARN + 跳过 + advance cursor;不返回 error、不 panic"
              :rationale "frozen lisp §4.2.c fault-isolation:'dispatcher panic 由 supervisor 重启对应 topic task;Dispatcher 全体崩从 last_dispatched 继续,不替人补发'。一条坏行不该拖垮整个总线。若坏行频繁出现,会在 rows_dropped_unknown_domain / rows_dropped_deserialize 指标上体现,ops 可见。这与 frozen lisp §4.4 observability 自报告精神一致(bug 暴露,不静默掩盖)。")

    (decision :id DC015 :phase 3 :date "2026-04-19"
              :topic "Fan-out transport:per-topic broadcast::Sender<Arc<T>>"
              :chose "每域一条 tokio::sync::broadcast::Sender<Arc<T>>,buffer=1024"
              :rationale "frozen lisp §4.2.c topic-registry 明确 per-topic broadcast + Arc<Event>。broadcast 语义符合'live fan-out 给所有当前订阅者';慢订阅触发 Lagged 在该订阅 local,不影响 tail 或其他订阅者(fault-isolation)。buffer=1024 是 frozen lisp 默认。Arc<T> 让多订阅者零拷贝。慢订阅者的 Lagged rewind 归 Phase 4 subscription API 处理,Phase 3 只保证自身不阻塞。")

    (decision :id DC016 :phase 4 :date "2026-04-19"
              :topic "Log dyn-compat:拆出 LogReadable 子 trait"
              :chose "新增 pub trait LogReadable: Send + Sync,只含 read_from / head_seq;blanket impl<T: Log> LogReadable for T;Subscription 接受 Arc<dyn LogReadable>"
              :rationale "Log::append<E> 泛型阻止 Arc<dyn Log>。Phase 4 订阅者不需要 append,只需读。子 trait 拆分让生产者继续用具体 Log(含泛型 append 零成本),订阅者用 LogReadable dyn 通道。见 I008。")

    (decision :id DC017 :phase 4 :date "2026-04-19"
              :topic "watermark 与 ack_cursor 分离"
              :chose "Lifecycle<T> 同时维护 watermark(已投递的最高 seq)和 ack_cursor(已 ack 的最高 seq);bootstrap 下次拉用 seq > watermark,而非 seq > cursor"
              :rationale "若用 ack_cursor 作 bootstrap watermark,第一批事件投递但尚未 ack 时,第二次拉取会重复读同一批 → 死循环。分离后 bootstrap 按投递进度推进、ack 按消费者确认推进,互不干涉。at-least-once 语义靠 ack_cursor 持久化保障")

    (decision :id DC018 :phase 4 :date "2026-04-19"
              :topic "Ack 被 drop 视为 silent nack"
              :chose "Drop for Ack<T> 发 FlushSignal::Nack { reason='ack dropped' };consumer 必须显式 ack() 或 nack()"
              :rationale "frozen lisp §4.3 delivery-semantics 要求 at-least-once。若 drop 被视为 auto-ack,consumer 崩溃时 cursor 已推进 → 事件丢失。反之 silent nack 触发 FailurePolicy:Retry 走 retry queue;SkipToDLQ 入库 DLQ;Halt 停 subscription。符合 fail-fast/at-least-once 契约")

    (decision :id DC019 :phase 4 :date "2026-04-19"
              :topic "Combinator 的 ack 保真策略"
              :chose "debounce / coalesce / batch 把被吸收的前面事件 silent_ack 掉,只让尾事件(或合成事件)surface 给 consumer。filter 把过滤掉的事件 silent_ack。map 保持 seq 透传。每条都是 silent_ack,不走 nack 路径"
              :rationale "consumer 用 combinator 就是想合并/过滤,这些'中间'事件已由 combinator 语义'接纳'。silent_ack 让 cursor 持续前推;如果用 nack 会误触 FailurePolicy(Retry 无穷循环)。frozen lisp §4.3 subscription-combinators 未细化 ack 行为,此处沿最少惊讶原则")

    (decision :id DC020 :phase 4 :date "2026-04-19"
              :topic "Retry 超限 fallthrough 到 SkipToDLQ(而非 Halt)"
              :chose "FailurePolicy::Retry { max: N, .. } 用满 N 次后,FailureRouter 自动转 SkipToDLQ,不 halt subscription"
              :rationale "frozen lisp §4.3 subscription-api 给出三个 policy 但未规定 Retry 超限后走哪个。选 SkipToDLQ 是 safe default:subscription 持续向前,坏事件入 DLQ 让 ops 离线修复。若 consumer 显式要 Halt,应 opt-in FailurePolicy::Halt 而不是靠 Retry 超限。tests failure.rs 覆盖此行为")

    (decision :id DC021 :phase 5 :date "2026-04-19"
              :topic "guards/causation.rs 独立模块 vs inline 到 LogWriter"
              :chose "独立 pub mod guards + pub fn check_causation(&AppendOpts) → Result<(), AppendError>;LogWriter 与 InMemoryLog 各调一次"
              :rationale "frozen lisp §4.4 把 causation-loop-guard 划为 cross-cutting,跨 log 实现;若 inline 到某一个 writer,换实现就会遗漏。guards 模块也给将来的 (schema-guard / producer-token-guard) 留了位置。")

    (decision :id DC022 :phase 5 :date "2026-04-19"
              :topic "MAX_CAUSATION_DEPTH 常量位置"
              :chose "guards/causation.rs 定义 + log/mod.rs 继续 re-export 同值 + unit test 断言二者相等"
              :rationale "既保持 Phase 2 对 log::MAX_CAUSATION_DEPTH 的调用兼容,又让 guards 模块自持源。单测 max_matches_log_module_const 锁死不漂移。")

    (decision :id DC023 :phase 5 :date "2026-04-19"
              :topic "InMemoryLog 的 seq 分配位置"
              :chose "在 WriterTask::handle_one 内 AtomicI64::fetch_add,而非 InMemoryLog::append 返回前分配"
              :rationale "frozen lisp §4.2.b writer-semantics 要求 single writer 分 seq。若 append() 直接 fetch_add,多 producer 并发调用会乱序。放到 writer task 里逐条处理保证'按入队顺序分 seq',与 PG BIGSERIAL + INSERT RETURNING 同语义。")

    (decision :id DC024 :phase 5 :date "2026-04-19"
              :topic "InMemoryLog 的 batching 语义"
              :chose "writer 每条独立处理,不做批窗合并"
              :rationale "frozen lisp §4.2.b batching 是 PG 路径为摊平 INSERT 成本的优化;InMemoryLog 无 DB round-trip,批合并无收益。契约说 'append Ok ⟺ 进入 log' 仍成立:进 Vec 即 commit。Chaos 测试若需断言批行为,跑 PG 版 LogWriter 而非 InMemoryLog。")

    (decision :id DC025 :phase 5 :date "2026-04-19"
              :topic "BusMetrics trait 粒度"
              :chose "8 方法(append / reject / dispatch_lag / topic_depth / subscription_lag / lagged / slow_consumer / control_gate_dropped),全按 frozen lisp §4.4 observability 列表"
              :rationale "每一项都是 frozen lisp 明确列出的指标;少一个就会在 emitter 的 snapshot 里缺口 → Phase 8 写 Grafana dashboard 时不得不改 trait。一次定义齐全。")

    (decision :id DC026 :phase 5 :date "2026-04-19"
              :topic "BusMetricsEmitter 周期"
              :chose "默认 METRICS_EMIT_INTERVAL = 10s,可配置(构造时传 Duration)"
              :rationale "frozen lisp §4.4 observability 未指定频率。10s 是 Prometheus 常见 scrape 间隔的整数倍,在 1000 events/s 下 snapshot 体量 < 1% 总量,不形成自观测血洗。")

    (decision :id DC027 :phase 5 :date "2026-04-19"
              :topic "ObservabilityEvent 发射路径"
              :chose "ObservabilityAppender trait 作为 narrowed Log(只接 ObservabilityEvent),避开 Log::append<E> 泛型 dyn-incompat"
              :rationale "Log trait 为 dyn-compat 拆出了 LogReadable,但 append 仍泛型。emitter 只需追加一种类型,用 ObservabilityAppender 接口即可;InMemoryLog / PgLogWriter 各自 impl 一次。代价 = 每实现 3 行 wrapper。")

    (decision :id DC028 :phase 5 :date "2026-04-19"
              :topic "Chaos test #9 cursor-orphan — stub vs real cron"
              :chose "只验 cursor 数据模型支持 stale predicate;真 cron 推迟到 Phase 8,issue I010 追踪"
              :rationale "frozen lisp §4.3 orphan-cleanup 是 daemon 启动时的后台任务,属 Phase 8 wiring 范畴。Phase 5 把 event_subscriptions.last_seen_at 列 + InMemoryCursorStore.snapshot() 的谓词过滤 pattern 锁住,Phase 8 只需要把 Arc<dyn CursorStore> + tokio::interval 粘起来。")

    (decision :id DC029 :phase 5 :date "2026-04-19"
              :topic "InMemoryBlobStore 是否用 LocalFile backend tag"
              :chose "InMemoryBlobStore 对外声明 backend() = BlobBackend::LocalFile,URI 以 `mem:` 前缀区分"
              :rationale "BlobBackend enum 只有 PgTable / LocalFile 两值;新增 InMemory 需要改 frozen lisp §4.2.b。URI 前缀区分不影响 PayloadRef on-wire 兼容性,tests 可通过 uri.starts_with('mem:') 识别。符合 frozen lisp 'forbidden-backends.in-memory-handle' 的精神:不作为 durable pointer,只作为 tests fixture。")

    (decision :id DC030 :phase 6 :date "2026-04-19"
              :topic "v1 publish 点的 dual-emit 语法模式"
              :chose "大多数点:let ev = DaemonEvent::X {...}; state.event_bus.publish[_traced](ev.clone(), ...); let _ = crate::bus::publish_v1_shim(&state.bus, &ev).await; 同步 helper 不能 await 处改用 crate::bus::spawn_v1_shim(bus.clone(), ev.clone()) detached spawn"
              :rationale "publish_v1_shim 是 async fn,调用 log.append<E> 必须 .await;但 Phase 6 不少 publish 点在 sync helper 内(如 extraction.rs::set_extraction_phase、pty_event_worker::handle_mcp_tool_error)。两种模式:(a) 需要把 helper 改 async(跨越 engine 大量签名改动),(b) tokio::spawn 让 append 在独立任务完成。方案 (b) 最小影响,且 producer 不依赖 append 的成功(at-least-once 契约已由 writer + dedupe_key 保障)。此决策确立 Phase 6 迁移的机械手法:90% 点用 publish_v1_shim().await,剩下 10% sync helper 用 spawn_v1_shim()。")

    (decision :id DC031 :phase 6 :date "2026-04-19"
              :topic "MPSC bypass sender 迁移两步走 vs 一步切换"
              :chose "两步走:Phase 6 仅在已有 domain enum 的 MPSC sender(incident_tx)加 dual-emit,embedding_tx / ast_sync_tx 保持不变;Phase 7 subscriber 迁移时再统一决策 receiver 侧 + sender 侧"
              :rationale "frozen lisp 任务书要求'Phase 6 仅添加 log.append,保留 MPSC send 不动'。但 IncidentEvent 已有;EmbeddingEvent / AstSyncEvent 不在 12 域。加 log.append 需新增 domain(破坏 compile-time 契约)或合并进现有域(跨相职责不清)。两步走让 Phase 7 有完整的 subscriber 设计空间,Phase 6 不提前锁死 domain 形状。D003 deviation 跟踪此偏差。")

    (decision :id DC032 :phase 6 :date "2026-04-19"
              :topic "LLM 客户端挂 bus 的方式:新增必需参数 vs Optional + with_bus builder"
              :chose "每个 LLM 客户端(GeminiClient / CodexCli / SonnetGateway / MinimaxGateway)新增 `bus: Option<Arc<BusServices>>` 字段 + `.with_bus(bus)` builder;main.rs 构造时链式 .with_bus(Arc::clone(&bus_services))"
              :rationale "这些客户端的 `new(event_tx)` 构造签名被许多 worker / vision_worker / step_narrator 重用,改必需参数会扇出到每一处构造点。Optional + builder 让重构范围限于 main.rs 和 2 个 worker,不污染 tests。shim 辅助方法在 bus 为 None 时无操作,保证无 bus 时客户端仍可用(未来若独立运行或 mock 时)。")

    (decision :id DC033 :phase 6 :date "2026-04-19"
              :topic "ControlGate adapter 位置:daemon 内 bus/ 子模块 vs 独立 top-level"
              :chose "daemon/src/bus/control_gate_adapter.rs,与 bootstrap/compat 同级"
              :rationale "ControlTreeGate 的唯一作用是把 daemon 内 watch::Receiver<ControlTree> 喂给 missiond-core 的 ControlGate trait。它只会在 BusServices::bootstrap 中被实例化,不属于 daemon 通用工具。放在 bus/ 下与 BusServices 同生命周期清晰,未来 Phase 8 替换为复杂 gate(如 per-project pause)时改动局部。")

    (decision :id DC034 :phase 6 :date "2026-04-19"
              :topic "BusServices.start() 的生命周期与 daemon shutdown 的耦合"
              :chose "返回 BusStartHandle(持有 shutdown_tx 和 dispatcher_join);main.rs 用 `let _bus_handle = bus_services.start(shutdown_rx.clone()).await?;` 让其随 daemon 生命周期存活;未显式 join(daemon 退出时 tokio 运行时会终止所有任务)"
              :rationale "frozen lisp §4.4 fault-isolation 允许 dispatcher 崩溃由 supervisor 重启;Phase 6 daemon 没有 supervisor 层,简单 fire-and-forget 足够。BusStartHandle 的 Drop impl 会发 shutdown 信号,正常 ctrl-c 路径也会关闭。未来若需要优雅退出,main.rs 收到 SIGTERM 后可 await bus_handle.shutdown() 取得 DispatchMetrics。")

    (decision :id DC035 :phase 6 :date "2026-04-19"
              :topic "AppState.bus 类型:Arc<BusServices> 直接持有"
              :chose "pub(crate) bus: Arc<BusServices>(必填,非 Option)"
              :rationale "Phase 6 开始 v2 bus 就是 daemon 启动的强制依赖。Option 会让下游 90+ publish 点每次都写 .as_ref().map(...),噪音大。启动失败的路径(PG 不可达)应在 bootstrap 阶段就 `?` 传播,不到 AppState 构造。与 store/mission/pty 同为必填字段符合一致性。"))

  ;; ─ 阶段完成记录 ─
  ;; 格式: (completion :phase N :date "..." :agent "name"
  ;;                   :deliverables ("path1" "path2" ...)
  ;;                   :tests-added N
  ;;                   :verified-by "cargo test passed|smoke test|..."
  ;;                   :notes "摘要")
  (completions
    (completion
      :phase 0 :date "2026-04-19" :agent "phase0-survey"
      :deliverables (".missiond/v2/_phase0-inventory.md")
      :tests-added 0
      :verified-by "manual review"
      :notes "完整 inventory of 旧 bus 代码,11 节覆盖所有 touch 点")

    (completion
      :phase 1 :date "2026-04-19" :agent "phase1-schema"
      :deliverables ("crates/missiond-core/src/event/mod.rs"
                     "crates/missiond-core/src/event/domain.rs"
                     "crates/missiond-core/src/event/event_trait.rs"
                     "crates/missiond-core/src/event/events/mod.rs"
                     "crates/missiond-core/src/event/events/slot.rs"
                     "crates/missiond-core/src/event/events/board.rs"
                     "crates/missiond-core/src/event/events/task.rs"
                     "crates/missiond-core/src/event/events/question.rs"
                     "crates/missiond-core/src/event/events/llm.rs"
                     "crates/missiond-core/src/event/events/worker.rs"
                     "crates/missiond-core/src/event/events/memory.rs"
                     "crates/missiond-core/src/event/events/message.rs"
                     "crates/missiond-core/src/event/events/session.rs"
                     "crates/missiond-core/src/event/events/system.rs"
                     "crates/missiond-core/src/event/events/observability.rs"
                     "crates/missiond-core/src/event/events/incident.rs"
                     "crates/missiond-core/src/lib.rs (+ pub mod event;)")
      :tests-added 45
      :verified-by "cargo build -p missiond-core OK; cargo test -p missiond-core event:: → 45 passed, 0 failed"
      :notes "12 domain enum + DomainEvent trait + Domain enum。Variant 总数 55 = v1 49 variants 完整覆盖(含 LEGACY Gemini×3/Codex×2 保留)+ 6 new(SlotEvent::Stuck 占位 + ObservabilityEvent×3 新引入 + IncidentEvent×2 新引入)。I001 的 9 个遗漏 variant 全部解决(映射见 DC001)。未触碰 missiond-daemon::event_bus::DaemonEvent,与 v1 完全共存。serde 采用默认 externally tagged(见 DC002 原因)。Provider enum 独立于 CliEngine(DC003)。")

    (completion
      :phase 2 :date "2026-04-19" :agent "phase2-storage"
      :deliverables ("crates/missiond-core/migrations/20260419000000_event_log.sql"
                     "crates/missiond-core/migrations/20260419000001_event_subscriptions.sql"
                     "crates/missiond-core/migrations/20260419000002_blob_storage.sql"
                     "crates/missiond-core/src/event/log/mod.rs"
                     "crates/missiond-core/src/event/log/writer.rs"
                     "crates/missiond-core/src/event/log/reader.rs"
                     "crates/missiond-core/src/event/log/retention.rs"
                     "crates/missiond-core/src/event/blob_store/mod.rs"
                     "crates/missiond-core/src/event/blob_store/claim_check.rs"
                     "crates/missiond-core/src/event/blob_store/pg_backend.rs"
                     "crates/missiond-core/src/event/blob_store/local_file_backend.rs"
                     "crates/missiond-core/src/event/mod.rs (+ pub mod log; pub mod blob_store;)"
                     "crates/missiond-core/tests/event_log_integration.rs")
      :tests-added 30
      :verified-by "cargo build -p missiond-core OK; cargo test -p missiond-core event:: → 75 passed, 0 failed (45 phase-1 + 30 phase-2); cargo build --tests OK; integration tests build OK but require Docker at runtime (skipped here by #[ignore])"
      :notes "Log trait + LogWriter 任务 + BlobStore claim-check 两后端 + 4 张新表 schema。writer 实现了完整的 frozen §4.2.b 语义:批量 INSERT (≤100/10ms)、UNIQUE dedupe_key 冲突回退到 SELECT existing seq 返回 AlreadyExists、exp backoff transient retry 超限进 failed state 拒新 append、CLAIM_CHECK_THRESHOLD=8192 分流到 blob_store 写 payload_ref。Retention cleanup 函数实现完毕但未 wire 进 daemon(Phase 8 任务)。整个模块未 touch v1 run_timeline_writer 或 DaemonEvent,完全共存。I005 cursor_ack_tx 未 touch,仍归 Phase 7 处理(见 DC005)。")

    (completion
      :phase 3 :date "2026-04-19" :agent "phase3-routing"
      :deliverables ("crates/missiond-core/src/event/dispatcher/mod.rs"
                     "crates/missiond-core/src/event/dispatcher/topic.rs"
                     "crates/missiond-core/src/event/dispatcher/registry.rs"
                     "crates/missiond-core/src/event/dispatcher/tail.rs"
                     "crates/missiond-core/src/event/dispatcher/control_gate.rs"
                     "crates/missiond-core/src/event/mod.rs (+ pub mod dispatcher;)"
                     "crates/missiond-core/tests/event_dispatcher_integration.rs")
      :tests-added 32
      :verified-by "cargo build -p missiond-core OK; cargo test -p missiond-core event:: → 107 passed, 0 failed (75 phase-1+2 + 32 phase-3); cargo test --no-run 全部编译 OK;integration tests 骨架 #[ignore] 需要 Docker"
      :notes "Dispatcher live-fan-out O(1) state;12 个 Topic<T> 按 TypeId + Domain 双索引查找;长轮询 tail(每 100ms SELECT LIMIT 256);control-gate 按 Domain→CtlDomain 映射检查(Memory/Board 映射,其余 10 域默认不 gate)。慢订阅者 Lagged 不传染、不阻塞 tail loop。坏行(unknown domain / payload deser fail)log WARN + drop + advance,不 panic。未 touch v1 event_bus.rs / event_router.rs / run_timeline_writer。ControlGate trait 避免 core→daemon 循环依赖;daemon 侧 Phase 8 提供 Adapter 把 ControlTree 接入。I002/I003 已 resolved。I007(ephemeral per-call)仍 pending,归 Phase 6 处理。")

    (completion
      :phase 4 :date "2026-04-19" :agent "phase4-subscription"
      :deliverables ("crates/missiond-core/src/event/subscription/mod.rs"
                     "crates/missiond-core/src/event/subscription/api.rs"
                     "crates/missiond-core/src/event/subscription/options.rs"
                     "crates/missiond-core/src/event/subscription/cursor_store.rs"
                     "crates/missiond-core/src/event/subscription/failure.rs"
                     "crates/missiond-core/src/event/subscription/lifecycle.rs"
                     "crates/missiond-core/src/event/subscription/combinators.rs"
                     "crates/missiond-core/src/event/mod.rs (+ pub mod subscription;)"
                     "crates/missiond-core/src/event/log/mod.rs (+ trait LogReadable)"
                     "crates/missiond-core/tests/event_subscription_integration.rs")
      :tests-added 40
      :verified-by "cargo build -p missiond-core --tests OK; cargo test -p missiond-core --lib event::subscription → 40 passed,0 failed;整库 cargo test -p missiond-core --lib → 216 passed,0 failed(147 phase-1..4 event tests + 69 pre-existing)"
      :notes "subscribe::<T>(name, opts, log, topic, cursor_store, dlq) 统一入口;Subscription<T>::next() 返回 Ack<T>,consumer 必须 .ack().await 或 .nack(reason).await。Ack drop 视为 silent nack(DC018)。两阶段 lifecycle:bootstrap 从 log.read_from pull,耗尽后切 live 读 Topic broadcast。watermark 与 ack_cursor 分离避免重读(DC017)。双阈值 flush:flusher task tokio::select!(Dirty/Force/Nack/interval),每条 ack 或 1s 时窗 upsert cursor。6 combinators:debounce/rate_limit/coalesce/filter/map/batch,被吸收事件 silent_ack(DC019)。FailurePolicy 三个:Retry 超限 fall through 到 SkipToDLQ(DC020)。PauseBehavior:MVP 只 DropAndLiveResume(D001);FreezeAndCatchUp 推迟 I009。引入 LogReadable 子 trait 解决 dyn 不兼容(DC016/I008)。未 touch v1 event_bus.rs/event_router.rs/workers/daemon。Phase 5+ 起草 InMemoryBus / daemon 迁移 consumer。")

    (completion
      :phase 6 :date "2026-04-19" :agent "phase6-producer-migration"
      :deliverables ("crates/missiond-daemon/src/bus/mod.rs"
                     "crates/missiond-daemon/src/bus/bootstrap.rs"
                     "crates/missiond-daemon/src/bus/compat.rs"
                     "crates/missiond-daemon/src/bus/control_gate_adapter.rs"
                     "crates/missiond-daemon/src/state.rs (+ pub(crate) bus: Arc<BusServices>)"
                     "crates/missiond-daemon/src/main.rs (+ mod bus; + bus_services bootstrap + start + AppState.bus 注入 + sonnet/minimax/gemini_client.with_bus + ConfigFileChanged dual-emit)"
                     "crates/missiond-daemon/Cargo.toml (+ sqlx workspace dep)"
                     "crates/missiond-daemon/src/llm/gemini_client.rs (+ bus field + with_bus + shim() + 3 publish-like sites)"
                     "crates/missiond-daemon/src/llm/codex_cli.rs (+ bus field + with_bus + shim() + 2 emit sites)"
                     "crates/missiond-daemon/src/llm/sonnet_gateway.rs (+ bus field + with_bus + WorkerLlmCall dual-emit)"
                     "crates/missiond-daemon/src/llm/minimax_gateway.rs (+ bus field + with_bus + WorkerLlmCall dual-emit)"
                     "crates/missiond-daemon/src/workers/codex/vision_worker.rs (+ .with_bus at CodexCli::new)"
                     "crates/missiond-daemon/src/workers/codex/step_narrator.rs (+ .with_bus at CodexCli::new + 5 DaemonEvent publish sites dual-emit)"
                     "crates/missiond-daemon/src/workers/sonnet/briefing_worker.rs (4 sites)"
                     "crates/missiond-daemon/src/workers/sonnet/translation_worker.rs (4 sites)"
                     "crates/missiond-daemon/src/workers/gemini/strategy_worker.rs (2 sites)"
                     "crates/missiond-daemon/src/workers/local/pty_event_worker.rs (7 sites + 1 incident_tx dual-emit via spawn_v1_shim)"
                     "crates/missiond-daemon/src/workers/local/tagger_chunker.rs (3 sites)"
                     "crates/missiond-daemon/src/workers/local/conversation_organizer.rs (1 site)"
                     "crates/missiond-daemon/src/handlers/knowledge/board.rs (7 sites; 3 helper fns now pass &Arc<BusServices>)"
                     "crates/missiond-daemon/src/handlers/knowledge/kb.rs (5 sites)"
                     "crates/missiond-daemon/src/handlers/knowledge/cascade.rs (2 sites)"
                     "crates/missiond-daemon/src/handlers/compute/task.rs (3 sites)"
                     "crates/missiond-daemon/src/handlers/comm/question.rs (4 sites)"
                     "crates/missiond-daemon/src/handlers/sysinfra/misc.rs (2 QuestionCreated + 1 incident_tx dual-emit)"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs (8 sites + 1 incident_tx dual-emit)"
                     "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs (1 site)"
                     "crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs (2 sites)"
                     "crates/missiond-daemon/src/engine/learning_engine/extraction.rs (helpers now take &Arc<BusServices>)"
                     "crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs (11 sites)"
                     "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs (2 sites)"
                     "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs (1 site)"
                     "crates/missiond-daemon/src/engine/learning_engine/intent_analyst.rs (1 site)"
                     "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs (1 site)"
                     "crates/missiond-daemon/src/infra/message_handler.rs (2 sites)"
                     "crates/missiond-daemon/src/infra/aiops.rs (2 publish + 1 incident_tx dual-emit)")
      :tests-added 2
      :verified-by "cargo build -p missiond-daemon OK; cargo build (workspace) OK; cargo test -p missiond-core --lib → 250 passed,0 failed(无 regression);cargo test -p missiond-core --test event_chaos → 12 passed,0 failed(chaos 无 regression);cargo test -p missiond-daemon → 91 passed,0 failed(daemon 单元测试无 regression)"
      :notes "83 v1 publish sites + 7 LLM internal send sites 全部 dual-emit。incident_tx sender 加了 bus.publish_incident(IncidentEvent::Reported) 双发(IncidentEvent 在 Phase 1 域内);embedding_tx / ast_sync_tx sender 保留纯 MPSC(D003 deviation)。LEGACY Gemini*/Codex* variants 透传进 LlmEvent::LegacyXxx(DC004 保留)。v1 event_bus::DaemonEvent / run_timeline_writer / event_router 未触(Phase 8)。AppState.bus 是必填字段(DC035)。bus/ 子模块 4 个新文件:mod.rs / bootstrap.rs / compat.rs / control_gate_adapter.rs。DC030 双发范式确立:90% 用 publish_v1_shim().await,10% 同步 helper 用 spawn_v1_shim 独立任务。BusServices::start 返回 BusStartHandle,daemon shutdown 随运行时终止(DC034)。I005 cursor_ack_tx 未动(保留 Phase 7);I007 ephemeral per-call 审计未动(compat shim 继续尊重 v1 is_ephemeral())。未新增单元测试(任务要求),但添加 2 个 bus 内部测试(control_gate_adapter round-trip + default_opts stamps producer_id)。")

    (completion
      :phase 5 :date "2026-04-19" :agent "phase5-cross-cutting"
      :deliverables ("crates/missiond-core/src/event/guards/mod.rs"
                     "crates/missiond-core/src/event/guards/causation.rs"
                     "crates/missiond-core/src/event/metrics/mod.rs"
                     "crates/missiond-core/src/event/metrics/emitter.rs"
                     "crates/missiond-core/src/event/in_memory/mod.rs"
                     "crates/missiond-core/src/event/in_memory/log.rs"
                     "crates/missiond-core/src/event/in_memory/blob_store.rs"
                     "crates/missiond-core/src/event/in_memory/cursor_store.rs"
                     "crates/missiond-core/src/event/in_memory/control_gate.rs"
                     "crates/missiond-core/src/event/mod.rs (+ pub mod guards; pub mod metrics; pub mod in_memory;)"
                     "crates/missiond-core/src/event/log/writer.rs (改调 check_causation)"
                     "crates/missiond-core/tests/event_chaos.rs (12 tests)")
      :tests-added 46
      :verified-by "cargo build -p missiond-core OK; cargo test -p missiond-core --lib → 250 passed,0 failed(216 phase-1..4 + 34 phase-5 unit);cargo test -p missiond-core --test event_chaos → 12 passed,0 failed"
      :notes "Causation guard 抽到独立 pub mod guards(DC021) + Phase 2 LogWriter 改调 check_causation 单入口(frozen lisp §4.4 跨相契约)。BusMetrics trait 8 个方法全覆盖 frozen lisp §4.4 observability 列表(DC025);AtomicBusMetrics 用 AtomicU64 + Mutex<HashMap> 做内存 MVP 收集;NoopMetrics 给 tests/daemon 启动前用。BusMetricsEmitter 每 10s snapshot → 多条 ObservabilityEvent::BusMetric(ephemeral=true)通过 ObservabilityAppender trait 发射(DC027)。InMemoryLog 严格对齐 PG 版语义:single writer task 分 seq(AtomicI64 fetch_add,DC023)、bounded mpsc cap 4096(frozen §4.2.b 同值)、dedupe HashMap 模拟 UNIQUE 索引、failed state 可程式化切换供 chaos#6 使用、ephemeral 分 seq 但不入 rows Vec、claim-check 超 8KB 走 BlobStore。Dispatcher 直接复用 Phase 3 run_tail(Arc<dyn TailSource>),InMemoryTailSource 适配器扫 Vec 按 seq 升序返回。InMemoryBus 聚合 log+blob+cursor+dispatcher+control_gate+metrics 一站式启动,start() spawn dispatcher tail task 返回 Handle。9 Chaos scenarios(+ 3 辅助 = 12 tests)在 tests/event_chaos.rs 全部不用 Docker 就跑:log-writer-timeout / log-writer-panic / dispatcher-panic / subscriber-panic / slow-subscriber-lag / db-disconnect / causation-loop / dedup-retry / cursor-orphan-stub + sanity-full-flow + 7b-guard-shared-contract + metrics-record。D002 deviation:Prometheus backend 推迟;I010 issue:cursor orphan daily cron wire up 推迟 Phase 8。InMemoryBlobStore 用 LocalFile backend tag + 'mem:' URI 前缀(DC029)避免扩 BlobBackend enum。未 touch v1 bus/daemon/workers(Phase 6+)。"))

  ;; ─ 全局备忘(跨阶段需要记住的事) ─
  (global-notes
    (historical-data-policy "system_timeline 旧数据不迁,保留 7 天 TTL 只读归档,3 月后废弃")
    (e2e-test-contract "Phase 9 前建立一条黄金路径测试: daemon 启动 → MCP board_create → event_log 写入 → WS 发送 → 前端收到"))
)
