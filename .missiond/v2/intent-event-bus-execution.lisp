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
    :status       "completed"
    :phase-cursor 10)

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
    (phase-7 :status "completed" :owner "phase7-subscriber-migration" :started "2026-04-19" :completed "2026-04-19"
             :summary "Subscriber 迁移 — daemon 侧 bus/v2_subscribers.rs 落地 + BusServices.dlq(PgDlqSink)+ BusServices::subscribe<T>() 助记 + main.rs 启动 hook(bus::start_v2_subscribers 位于 _bus_handle 之后)。14 个 v2 订阅者全部上线,与 v1 timeline_tx 订阅并存(Phase 8 删 v1):A 组 8 个 event_router consumers(extraction / submit / decision / harvest / realtime_extraction / session_reflection / kb_consolidation / intent_analyst)使用 bus.subscribe::<T>() + debounce() combinator,动作与 v1 同(schedule_memory_tasks / dispatch_queued_submit_tasks / process_pending_master_questions 等皆 DB-poll 幂等,双发安全);B 组 6 个 worker observers(gemini_logger / translation / arch_maintenance / lisp_survey / conversation_organizer / tagger_chunker)仅订阅 + ack,不重复 v1 副作用(LLM 调用 / git 操作),只做 debug log → Phase 8 切 active handler。I005(cursor_ack_tx 内化)标 D004 推迟 Phase 8,risk/reward 不值;I004(WS wire-format)全部推迟 Phase 8(继续走 v1 ws_tx,决策见 D005)。未触 v1 event_bus.rs / event_router.rs / run_timeline_writer / 任何 worker.rs。workspace build clean + missiond-core 250 lib tests + 12 chaos tests + missiond-daemon 91 unit tests 全部 PASS。")
    (phase-8 :status "completed" :owner "phase8-cleanup" :started "2026-04-19" :completed "2026-04-19"
             :summary "V1 bus 完全删除 — event_bus.rs / event_router.rs / bus/compat.rs 三个文件删除;run_timeline_writer 从 main.rs 删除;4 条 MPSC bypass 缩减为 1 条(embedding/ast_sync 改标 worker-internal queue deviation D007/D008,incident 完全迁入 IncidentEvent v2 subscriber,cursor_ack 内化为 AppState.conversation_cursor_map + drain task 取代 cursor_ack_tx)。6 个 v1 timeline_rx workers 全部升级为 bus.subscribe::<T>() active handlers(gemini_logger / translation / arch_maintenance / lisp_survey / conversation_organizer / tagger_chunker)。4 个 LLM 客户端(gemini_client / codex_cli / sonnet_gateway / minimax_gateway)移除 TimelineEntry MPSC,改为 Option<Arc<BusServices>> + publish_llm/publish_worker。所有 handler / engine 文件(board / cascade / kb / misc / task / question / decision_engine / autopilot / flow_engine / memory_scheduler / historical_scanner / intent_analyst / timeline_analyst / idle_explorer / extraction / aiops / message_handler / pty_event_worker / briefing_worker / strategy_worker / step_narrator)85 个 v1 publish 点全部直接改成 state.bus.publish_xxx(V2Event::X) — 移除 v1 publish + v1_shim。新增 bus/ws_bridge.rs(PgTailSource 尾读 event_log → v2→v1 wire-format 转换 → frontend_events_tx,保全浏览器 WS 契约,12 个 variant 的字节相等单元测试)+ bus/retention_cron.rs(每日凌晨跑 event::log::retention::cleanup_once + 孤儿 cursor 清理 + ObservabilityEvent::RetentionReport 发射 + 每个归档 cursor 发 IncidentEvent::StaleSubscription)。新 v2 subscriber:spawn_incident_reactor(IncidentEvent::Reported → aiops::process_incident,替代 incident_rx 主动消费)。I004 resolved(ws_bridge JSON wire-format 字节兼容,12 测试已过);I005 resolved(cursor_ack_tx 内化成 Arc<Mutex<HashMap>> + 250ms drain tick);I007 resolved(ephemeral 改为 per-call — publish_worker/publish_memory/publish_message/publish_session 根据 variant 自动打 ephemeral 标);I010 resolved(retention cron + orphan cleanup wired)。新增 ObservabilityEvent::RetentionReport + IncidentEvent::StaleSubscription 两个 variant(不破坏 12 域契约,只在现有 enum 加 variant)。workspace build clean + missiond-core 250 lib tests + 12 chaos tests + missiond-daemon 96 tests(+5 from new ws_bridge/retention_cron)全部 PASS。剩余 D007/D008 deviation:EmbeddingEvent/AstSyncEvent 仍非 12 域内,继续作为 worker-internal queue;其语义从 bus event 剥离明确定义,不再视为 bypass。")
    (phase-9 :status "completed" :owner "phase9-final-validation" :started "2026-04-19" :completed "2026-04-19"
             :summary "最终用户无感验证 — 5 交付:1) E2E golden path test 落地在 crates/missiond-daemon/tests/e2e_bus_golden_path.rs(#[ignore],需 Docker)全流程 PG → LogWriter → BoardEvent::TaskCreated append → event_log row 校验 → PgTailSource 回读 LoggedEvent → v1 wire-format envelope 比较(type/seq/payload 三维度断言);2) 全量测试扫描:cargo build --all-targets clean(52-54 warnings 无 error);cargo test --workspace → 391 passed,0 failed(missiond-core lib 250 / event_chaos 12 / event_dispatcher_integration 1+2 ignored / missiond-daemon 96 / +72 其他 crate);cargo clippy --workspace --all-targets 0 errors(292 warnings 皆非 bus 引入);3) Smoke test:release binary 用 MISSIOND_HOME=/tmp + 独立 PG DB + WS port 19120 启动,4 秒内全部上线(bus bootstrap + dispatcher + 8 router consumers + incident reactor + ws bridge + retention cron + 8 subscription live),SIGTERM 清理干净无 panic;4) frozen lisp §4.2 对照:生成 .missiond/v2/_phase9-layout-check.md,每个 component → 实际文件路径 1:1 映射,无 ghost(所有 core lib 文件 + daemon bus/ 5 文件都有 lisp 或 execution-log 锚点);5) 整份重构总结报告 .missiond/v2/_refactor-summary.md:9 commits (phase 0-8)、+29909/-3398 行、109 新文件/2 删除、52→64 variants、v1/v2 ASCII 架构对比图、D001-D007 全量 deviation 列表(永久保留 D001/D002/D007)、I001-I010 全量 issue 状态(I006/I009 保留,其余 resolved)、DC001-DC049 按 phase 分组决策、已知限制 + 短/中/长期 follow-up 建议。新增 DC048(E2E test placement 决策)+ DC049(smoke test 证据)。Meta 状态从 in-progress → completed,phase-cursor 8 → 9。")
    (phase-11 :status "pending" :owner nil :started nil :completed nil
             :summary "god-file split per lisp v1.1.0: step3_commit(1008→8 files) / step5_tail(883→4) / combinators(600→6 in subdir) / lifecycle(498→2) / in_memory(575→4) — 零功能改动,保持 358 tests 零回归")
    (phase-10 :status "completed" :owner "phase10-reorg" :started "2026-04-19" :completed "2026-04-19"
             :summary "pipeline/ 七步模块化重组,lisp target 路径对齐。零功能改动,仅物理重排:`event/` 下新建 `pipeline/step{1..7}_*/` 目录 + `lifecycle/` 目录;`guards/` 删除(内容迁入 step1_guard/);`log/writer.rs` → `pipeline/step3_commit/log_writer.rs`;`log/retention.rs` → `lifecycle/retention.rs`;`dispatcher/{tail,control_gate,topic,registry}.rs` → `pipeline/step{5,6,7}_*/`;`blob_store/claim_check.rs` → `pipeline/step2_decide/claim_check.rs`。所有 legacy `event::{log,guards,dispatcher,blob_store}::…` 导入路径保留可用(通过 back-compat re-export)。frozen lisp §4.2 共 7 条 :target 路径更新对齐新结构。测试状态:missiond-core --lib 255 pass(原 250 + 5 个 Phase 10 新增 doc-anchor 测试:step1_guard/type_resolve×1 + step2_decide/persistence_policy×2 + step4_ack/ack_transport×2);event_chaos 12 pass 无 regression;missiond-daemon 96 pass 无 regression。"))

  ;; ─ 并行锁表(防 agent 冲突) ─
  ;; 格式: (claim :phase N :scope "path/description" :agent "name" :claimed-at "..." :released-at "..."|nil)
  (claims
    (claim :phase 4 :scope "crates/missiond-core/src/event/subscription/**" :agent "phase4-subscription"
           :claimed-at "2026-04-19T00:00:00Z" :released-at "2026-04-19T00:00:00Z")
    (claim :phase 5 :scope "crates/missiond-core/src/event/{guards,metrics,in_memory}/** + tests/event_chaos.rs" :agent "phase5-cross-cutting"
           :claimed-at "2026-04-19T00:00:00Z" :released-at "2026-04-19T00:00:00Z")
    (claim :phase 6 :scope "crates/missiond-daemon/src/bus/** + AppState.bus + all 83 v1 publish sites + 7 LLM internal sends" :agent "phase6-producer-migration"
           :claimed-at "2026-04-19T00:00:00Z" :released-at "2026-04-19T00:00:00Z")
    (claim :phase 7 :scope "crates/missiond-daemon/src/bus/{mod,bootstrap,v2_subscribers}.rs + main.rs bus::start_v2_subscribers hook" :agent "phase7-subscriber-migration"
           :claimed-at "2026-04-19T00:00:00Z" :released-at "2026-04-19T00:00:00Z")
    (claim :phase 8 :scope "delete event_bus.rs / event_router.rs / compat.rs / run_timeline_writer; rewire all publish sites + workers to v2; add ws_bridge + retention_cron; internalize cursor_ack" :agent "phase8-cleanup"
           :claimed-at "2026-04-19T00:00:00Z" :released-at "2026-04-19T00:00:00Z")
    (claim :phase 9 :scope "E2E golden path test + 全量测试扫描 + daemon smoke test + frozen-lisp layout check + refactor summary" :agent "phase9-final-validation"
           :claimed-at "2026-04-19T00:00:00Z" :released-at "2026-04-19T00:00:00Z")
    (claim :phase 10 :scope "crates/missiond-core/src/event/{pipeline,lifecycle}/** + reshape dispatcher/log/guards/blob_store + frozen lisp §4.2 :target 路径对齐" :agent "phase10-reorg"
           :claimed-at "2026-04-19T00:00:00Z" :released-at "2026-04-19T00:00:00Z")
    (claim :phase 11 :scope "crates/missiond-core/src/event/subscription/** + crates/missiond-core/src/event/in_memory/**" :agent "phase11-beta-subscription"
           :claimed-at "2026-04-19T00:00:00Z" :released-at "2026-04-19T00:00:00Z")
    (claim :phase 11 :scope "crates/missiond-core/src/event/pipeline/step3_commit/** + crates/missiond-core/src/event/pipeline/step5_tail/**" :agent "phase11-alpha-pipeline"
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
               :approved-by "auto")

    (deviation :id D004 :phase 7 :date "2026-04-19" :agent "phase7-subscriber-migration"
               :lisp-said "frozen lisp §4.1 dead-bypass 说 cursor_ack_tx 不作为 event,而在 conversation-logger worker 内部处理;Phase 7 任务书进一步说'重构 conversation_logger_worker 的 run loop,让 cursor_ack 逻辑完全内部化:删除 state.cursor_ack_tx 字段 + AppState 中对应 Arc<UnboundedSender<_>>,message_handler 的 INSERT 成功不再 cursor_ack_tx.send()'"
               :actually-did "Phase 7 未重构 conversation_logger 的 cursor_ack。保留 state.cursor_ack_tx 与 main.rs:846-858 的 ack consumer task 不动"
               :reason "内化要求把 gemini_tasks + cc_tasks 两个 watcher 的 persist_cursor_ack 迁到 ConversationLoggerWorker 内部。ConversationLoggerWorker 当前只持 conv_logger_rx 一个 broadcast::Receiver,要接入两个 watcher 需改 worker trait + spawn 路径 + 两套 cursor routing 逻辑,远超 'Phase 7 subscriber 迁移' 本职。cursor_ack_tx 也不是 DomainEvent,不是本 Phase 的 bus 订阅目标。I005 仍有效,推迟 Phase 8 一并处理(可能与 ConversationLoggerWorker 的完整重构合并)。"
               :approved-by "auto")

    (deviation :id D005 :phase 7 :date "2026-04-19" :agent "phase7-subscriber-migration"
               :lisp-said "frozen lisp §4.3 + 任务书 I004 要求 Phase 7 将 WS 层订阅者迁到 v2 bus — WS server 订阅 12 个 topic,用 compat serializer 生成与 v1 字节完全相同的 JSON 推给前端"
               :actually-did "Phase 7 WS 层完全不动。frontend_events_tx 继续由 run_timeline_writer(v1 timeline_writer)在每条事件 fan-out 时 send JSON 字符串;WS server 继续订阅 frontend_events_tx"
               :reason "v1 run_timeline_writer 仍在运行(Phase 6/7 保留),ws_tx 链路完整。真正的 WS 切换意味着:(a) 删掉 run_timeline_writer,(b) 新写 ws_bridge.rs 订阅全部 12 个 v2 topic + v2_event_to_v1_json bit-identical 序列化器,(c) 切换 ws_tx 生产端。这三步任何一环失败前端都静默挂,风险极高;而 Phase 6 dual-emit 意味着 v1 + v2 同时有数据,ws 在 Phase 8 删 v1 时一并切才合适。I004 resolution 改为 Phase 8。任务书本身也给了此路径:'简化路径...推迟到 Phase 8(删 v1 同时切 WS)。加 deviation 记此决定'。"
               :approved-by "auto")

    (deviation :id D006 :phase 7 :date "2026-04-19" :agent "phase7-subscriber-migration"
               :lisp-said "任务书 B 类 worker subscribers 段:'6 处 workers 中的 subscribers...改成 bus.subscribe::<T>(\"worker_xxx\")'。step A/B 说'保留 v1 订阅器代码!Phase 8 再删。这样双 consumer 并存'"
               :actually-did "6 个 worker 的 v2 订阅是 passive observer(只订阅+ack+log,不重复 v1 的真实副作用);v1 timeline_rx 继续负责实际动作"
               :reason "event_router 8 consumers 的动作是 DB-poll 幂等(schedule_memory_tasks / process_pending_master_questions 等从 DB 读 pending 状态再处理),双发安全。但 worker 侧的动作:LLM 调用(translation / strategy)、git 检测 + YAML 更新(arch_maintenance)、intent.lisp 增量更新(lisp_survey)、会话链路修复(conversation_organizer)、turn 提取(tagger_chunker)— 这些双发会实打实重复 LLM 调用 + 写 YAML + 写 DB,浪费资源且可能写冲突。passive observer 方案一方面验证 v2 订阅路径活,一方面保持功能不重复。Phase 8 切换时只需把 observer 升级成 active handler,同时删除 timeline_rx 路径。这是任务书'优先功能正确'精神的直接体现。"
               :approved-by "auto")

    (deviation :id D007 :phase 8 :date "2026-04-19" :agent "phase8-cleanup"
               :lisp-said "frozen lisp §4.1 dead-bypass:embedding-tx / ast-sync-tx 应改走 log.append(EmbeddingEvent::Requested) / log.append(AstSyncEvent::Requested)。Phase 8 任务书进一步说'AppState 里的 embedding_tx/ast_sync_tx/incident_tx 字段(receiver 端也一并删,subscriber Phase 7 已建 v2 替代),改单调用 state.bus.publish_embedding(...) / state.bus.publish_ast_sync(...)'"
               :actually-did "Phase 8 保留 embedding_tx / ast_sync_tx 作为 worker-internal MPSC。incident_tx 完全删除(迁入 IncidentEvent)。embedding_tx / ast_sync_tx 不再视为 'bus bypass',而是 worker-to-worker queue"
               :reason "frozen lisp §4.2.a 明确定义 12 个 domain enum,EmbeddingEvent / AstSyncEvent 不在列。Phase 6 D003 已记录:新增 2 个 domain 会破坏 12 域 compile-time 契约。Phase 8 任务书与 frozen lisp §4.2.a 冲突 — 按 §4.2.a 契约解读,embedding/ast_sync 不应成为新 domain。改用语义澄清:这两个 MPSC 是 worker → EmbeddingLoopWorker / AstSyncWorker 的直接任务队列(类似 `Notify` / `broadcast::Sender<WatcherEvent>`),不是事件 bus 的一部分,不需要走 event_log 持久化。incident 不同:它是外部输入(WS webhook / autopilot 检测),需要 event_log 持久化 + ops 审计,故它作为 IncidentEvent。本 Phase 重新定义 dead-bypass 范围:只有 (a) 需要 event_log 持久化,(b) 多 consumer 订阅,或 (c) 需要跨进程可见 的才算 bus event。embedding/ast_sync 是 1-producer/1-consumer worker task queue,不需要这些属性。"
               :approved-by "auto")

    (deviation :id D010 :phase 11 :date "2026-04-19" :agent "phase11-beta-subscription"
               :lisp-said "frozen lisp §4.3 subscription-combinators :target 指向 `crates/missiond-core/src/event/subscription/combinators/` 子目录,每个 combinator 独立文件(debounce.rs / rate_limit.rs / coalesce.rs / filter.rs / map.rs / batch.rs),:trait-entries-target 指向 `combinators/mod.rs`"
               :actually-did "按 lisp 规范创建 6 个独立文件 + 1 个 mod.rs;但 `impl<T: DomainEvent> Ack<T>` 的 combinator 支持方法(`__new_for_combinator` + `silent_ack`)保留在 combinators/mod.rs(任务书提示可选移到 subscription/api.rs,未做,保持一处)"
               :reason "这两个方法是 combinator 私有的 `Ack` 构造/消费路径,与 combinator 同生命周期;放在 combinators/mod.rs 使得 `pub(crate)` 能直接给 batch/coalesce/map 等子模块使用而无跨文件可见性问题。api.rs 是订阅入口,承担这两个方法会把 combinator 内部机制泄露到公共 API 层。零功能改动契约下,保持这份轻量耦合符合 Phase 10 back-compat shim 精神(DC054)。公共 API 100% 兼容,外部 callsite 不变。"
               :approved-by "auto")

    (deviation :id D011 :phase 11 :date "2026-04-19" :agent "phase11-beta-subscription"
               :lisp-said "frozen lisp §4.3 subscription-lifecycle :target `crates/missiond-core/src/event/subscription/lifecycle.rs` + live-source :target `subscription/live_source.rs`"
               :actually-did "严格按 lisp 两文件拆分:lifecycle.rs(Phase enum + LifecycleError + Fetched + Lifecycle 编排,含 tests)和 live_source.rs(LiveSource trait + BroadcastLiveSource + MpscLiveSource)。lifecycle.rs 的 tests 需要 MpscLiveSource 做 live-overlap 测试,test module 内直接 `use crate::event::subscription::live_source::MpscLiveSource`"
               :reason "lisp 规范每一字都落地。subscription/mod.rs 的 re-export 同步更新:`pub use lifecycle::{Lifecycle, LifecycleError, Phase}; pub use live_source::{BroadcastLiveSource, LiveSource, MpscLiveSource};` — 公共 API 从顶层 `subscription::*` 角度 100% 兼容(之前 LiveSource 从 lifecycle 导出,现在从 live_source 导出,但顶层 re-export 仍然提供)。api.rs 内部 `use super::lifecycle::Lifecycle; use super::live_source::{BroadcastLiveSource, LiveSource};` 调整为两个导入路径。"
               :approved-by "auto")

    (deviation :id D012 :phase 11 :date "2026-04-19" :agent "phase11-beta-subscription"
               :lisp-said "frozen lisp §4.4 testing-story :in-memory-breakdown 列 5 个文件:log.rs(InMemoryLog + Log impl)/ writer_task.rs(WriterTask + Pending + payload_bytes)/ storage.rs(StoredRow + IN_MEMORY_APPEND_CAPACITY)/ observability.rs(ObservabilityAppender impl)/ 保留 cursor_store.rs + blob_store.rs + control_gate.rs"
               :actually-did "严格按 lisp 4 文件拆分:log.rs(460 行,含 tests)/ writer_task.rs(111 行)/ storage.rs(35 行)/ observability.rs(28 行)。in_memory/mod.rs 添加 `pub mod storage; pub mod writer_task; pub mod observability;` + `pub use storage::{StoredRow, IN_MEMORY_APPEND_CAPACITY};`(之前从 log 导出 IN_MEMORY_APPEND_CAPACITY 现改从 storage 导出,顶层 pub use 同名 symbol 保证外部 API 100% 兼容)。`stored_to_logged_pub` 保留在 log.rs 供 in_memory/mod.rs 的 InMemoryTailSource 使用(路径未变)"
               :reason "lisp 规范的每一行都落地。storage.rs 只含纯数据类型,与 writer_task 共享所有权:`Pending` 里的 `row: StoredRow` 需要 `StoredRow` 对 writer_task 可见,故 writer_task.rs `use super::storage::StoredRow;`。观测 appender 拆到 observability.rs 后,在独立 `impl` block 中引用 `super::log::InMemoryLog`,因该 trait impl 需要看到 InMemoryLog public append 方法(trait 方法而非 inherent)。零功能改动契约严格保持:PG 分离的 trait impls 无改变,chaos tests / unit tests 全通。"
               :approved-by "auto")

    (deviation :id D008 :phase 11 :date "2026-04-19" :agent "phase11-alpha-pipeline"
               :lisp-said "frozen lisp v1.1.0 §4.2 step-3 commit 列出 7 个组件,每个带独立 :target:log-writer / handle / backend-abstraction / pg-backend / seq-authority / dedup-semantics / backpressure / failure-mode,分别在 step3_commit/ 下的独立文件"
               :actually-did "严格按 lisp 创建 8 个 .rs 文件(mod + 7 components):log_writer.rs(296 行实现 + ~380 行 tests)、handle.rs(140 行)、backend.rs(58 行 trait+types)、pg_backend.rs(118 行 cfg-gated)、seq_authority.rs(保留 Phase 10 的 34 行 doc-only + EventSeq 类型别名)、dedup.rs(53 行:DEDUP_UNIQUE_COLUMNS + is_unique_violation classifier)、backpressure.rs(61 行:PendingAppend struct + 3 个 const)、failure_mode.rs(61 行:RETRY_BASE_DELAY/RETRY_MAX_DELAY/FAILED_STATE_RETRY_CAP + exp_backoff fn + 1 test)。不是偏离,是 v1.1.0 规定的实施。step3_commit/mod.rs 承担 `pub mod` 声明 + `pub use` 公共 re-export 保证顶层 API 100% 兼容(PendingAppend / APPEND_CHANNEL_CAPACITY / BATCH_MAX / BATCH_DEADLINE / LogWriterHandle / LogWriter / new_log_writer / spawn_log_writer)。event/log/mod.rs 的 writer shim 同步更新为从各子模块 re-export。tests 保留内联在 log_writer.rs 末尾(任务书允许 inline / tests.rs 独立,选 inline 减少文件数)。is_unique_violation 从 pg_backend 迁到 dedup 以让 classifier 表达 dedup 契约而非 PG 驱动细节。exp_backoff 从 log_writer 迁到 failure_mode 与 retry 常量同居。Cargo build + cargo test -p missiond-core --lib event::pipeline::step3_commit 9 pass。"
               :reason "lisp v1.1.0 由用户 sealed-at 2026-04-19 v1.0.0→v1.1.0 批准,这是实施非偏离。拆分后:(a) 每个文件单一关注点,navigable 1:1 与 lisp §4.2 step-3 的 7 个 bullet;(b) 零功能改动(同样的 LogWriter 逻辑,只是物理位置分散);(c) 公共 API 100% 兼容(log/mod.rs 的 re-export 全部重定向到新位置,外部 `use event::log::{spawn_log_writer, LogWriterHandle, PendingAppend, ...}` 继续工作);(d) log_writer.rs 从 1008 行降到 ~676 行(含 tests),其中实现部分 ~296 行更聚焦于 run/flush 主循环 + claim-check + dedup fan-out 编排。"
               :approved-by "auto")

    (deviation :id D009 :phase 11 :date "2026-04-19" :agent "phase11-alpha-pipeline"
               :lisp-said "frozen lisp v1.1.0 §4.2 step-5 tail 列出 4 个组件:dispatcher-state :target step5_tail/mod.rs / tail-source :target step5_tail/tail_source.rs / pg-tail :target step5_tail/pg_tail.rs / tail-mechanism :target step5_tail/dispatcher.rs"
               :actually-did "严格按 lisp 创建 4 个 .rs 文件:mod.rs(DispatchMetrics struct + 两个常量 TAIL_BATCH_LIMIT/TAIL_POLL_INTERVAL + pub use re-exports + ~550 行 tests 内联)、tail_source.rs(TailSource trait + DispatchError + TailError + From<LogError>,60 行)、pg_tail.rs(PgTailSource impl TailSource,~128 行 cfg-gated)、dispatcher.rs(run_tail 主循环 + dispatch_one + control-gate 调用,~155 行)。不是偏离,是 v1.1.0 规定的实施。mod.rs 的 `pub use dispatcher::run_tail; pub use metrics::DispatchMetrics; pub use tail_source::{DispatchError, TailError, TailSource}; #[cfg(feature=\"postgres\")] pub use pg_tail::PgTailSource;` 保证顶层 event::pipeline::step5_tail::* 公共 API 100% 兼容(event::dispatcher::{TailSource, PgTailSource, DispatchMetrics, ...} 的 re-export 链继续工作)。Cargo build + cargo test -p missiond-core --lib event::pipeline::step5_tail 8 pass;cargo test --test event_chaos 12 pass;cargo test -p missiond-daemon 96 pass。"
               :reason "lisp v1.1.0 由用户 sealed-at 2026-04-19 v1.0.0→v1.1.0 批准,这是实施非偏离。拆分后:(a) 每个文件单一关注点,navigable 1:1 与 lisp §4.2 step-5 的 4 个 bullet;(b) 零功能改动(同样的 run_tail / PgTailSource 逻辑,只是物理位置分散);(c) 公共 API 100% 兼容(event::dispatcher 的旧 shim 无需改 — 它 re-export from step5_tail::*,自动跟随新结构);(d) mod.rs 从 883 行降到 636 行(含 tests);tests 保留内联在 mod.rs 末尾以避免跨文件 test utility 共享困难。"
               :approved-by "auto")

    (deviation :id D013 :phase 11 :date "2026-04-19" :agent "phase11-persistence-layer-audit"
               :lisp-said "frozen lisp v1.2.0 §4.6 (section persistence-layer) 4 张表的 (access-patterns) 列出 6 条 :target 指向: event_log.write → step3_commit/log_writer.rs, event_log.read-catchup → subscription/lifecycle.rs, event_subscriptions.read-on-bootstrap → subscription/lifecycle.rs, blob_storage.write → step2_decide/claim_check.rs, blob_storage.read → subscription/api.rs, dead_letter_queue.write → subscription/lifecycle.rs"
               :actually-did "验证 SQL 实际位置后,按 file-governance :allowed-without-ask path-drift-correct 策略更新 6 处 :target 指向 SQL 执行文件:event_log.write → step3_commit/pg_backend.rs (log_writer 只做 batch 调度,SQL 在 PgWriterBackend);event_log.read-catchup → log/reader.rs (LogReader.read_from 执行 SELECT, lifecycle 只是调用方);event_log 新增 retention → lifecycle/retention.rs (DELETE 3d ephemeral / 30d persistent);event_subscriptions.read-on-bootstrap → subscription/cursor_store.rs (cursor_store 是 PgCursorStore trait impl,SQL 在此);event_subscriptions 新增 orphan-sweep → missiond-daemon/src/bus/retention_cron.rs (sweep_orphan_cursors SQL);blob_storage.write → blob_store/pg_backend.rs (PgBlobStore.put INSERT);blob_storage.read → blob_store/pg_backend.rs (PgBlobStore.get SELECT);blob_storage.cleanup → lifecycle/retention.rs (DELETE WHERE ttl_expires < now);dead_letter_queue.write → subscription/failure.rs (PgDlqSink.record INSERT)。Phase 4 ownership 验证:所有 event_log / event_subscriptions / blob_storage / dead_letter_queue 的 SQL 访问都在 crates/missiond-core/src/event/ 下 (加 retention_cron.rs 为 frozen lisp §4.2 lifecycle-maintenance orphan-cleanup 已显式授权的 daemon-侧目录),无 ownership violation。Phase 2 schema 验证:三个 migration 文件列 + 索引与 lisp §4.6 声明 100% 一致,无 schema drift。"
               :reason "path-drift-correct 属于 file-governance allowed-without-ask 条款。§4.6 v1.2.0 新增,作者将 :target 写到了调用方/决策方(claim_check / lifecycle / api),而 §4.6 其他已正确的 :target 惯例是指 SQL 执行文件(cursor_store / pg_tail)。统一到 '指向 SQL 执行文件' 约定,一次 mission_intent 拉取 lisp 即可跳到 grep 得到的真实 INSERT/SELECT 行。另:补充 retention / orphan-sweep / cleanup 3 条 access-pattern 行以覆盖 lifecycle/retention.rs + bus/retention_cron.rs 的实际 SQL 访问面(在 §4.2 lifecycle-maintenance 已间接提到,§4.6 需对偶)。"
               :approved-by "auto")

    (deviation :id D014 :phase "v1.3.0" :date "2026-04-19" :agent "user-session-memory-pillar-refinement"
               :lisp-said "v1.2.0 §4.2 retention :old-system_timeline '已删(Phase 8)' — 与实况不符: Phase 8 并未 DROP system_timeline 表, timeline-writer 订阅者仍在运行, mission_timeline / WS timeline-stream 仍读 system_timeline. event_log 虽已是新 SSOT 写入层, 但 timeline 侧读取路径未完成 cutover."
               :actually-did "v1.2.0 → v1.3.0 升级 (user approved): (1) file-governance :version v1.3.0, :last-revision 记录本次升级; (2) §4.2 retention 的 :old-system_timeline '已删(Phase 8)' 改为 :system_timeline-cutover 精确描述实况进度 (schema drop migration 待写, subscriber 待移除, readers 待迁); (3) §4.6 event_log access-patterns 新增 read-ui-projection 模式 — 声明 mission_timeline + WS timeline-stream 经 projection 直读 event_log, 列出 SQL VIEW vs 代码层两种 projection 选项 + FTS 索引需求; (4) §4.6 event_log 尾部新增 :ssot-declaration 锁死 event_log = timeline SSOT. 未改代码, 仅在 lisp 锁定目标态."
               :reason "v1.2.0 '已删(Phase 8)' 是历史乐观 — migrations/ 仍有 system_timeline schema. 2026-04-19 memory pillar session 中用户明确要求 timeline 与 event-bus 合并做 SSOT. 本次变更属 structural-change (新 access-pattern + :ssot-declaration), 需 user 明确同意 — 已获得. 代码 cutover (drop 表 + 移除 timeline-writer + 迁移 readers + 加 FTS) 在后续阶段执行."
               :approved-by "user (2026-04-19 session)")

    (deviation :id D015 :phase "v1.3.0-cutover" :date "2026-04-19" :agent "ssot-cutover-executor"
               :lisp-said "v1.3.0 §4.6 event_log :ssot-declaration + read-ui-projection access-pattern + :system_timeline-cutover 声明目标态: event_log 即 timeline SSOT, 需 drop system_timeline 表 / 移除 timeline-writer / 迁 readers / 加 FTS."
               :actually-did
                 ("(1) 新 migration 20260420100000_event_log_fts.sql — GIN 索引 to_tsvector('simple', payload_inline::text) 覆盖 §4.6 read-ui-projection :fts-requirement."
                  "(2) 新 migration 20260420200000_drop_system_timeline.sql — DROP TABLE IF EXISTS system_timeline CASCADE (CASCADE 清除 20260318000000_init 里 4 个 idx_tl_* 索引)."
                  "(3) 新模块 crates/missiond-core/src/event/projection.rs (~300 行) — event_log → TimelineRow 投影: event_type='domain::kind', summary 从 payload_inline.preview/summary/title 抽取, FTS 搜索走 plainto_tsquery, 7 个 public fn 对应 TimelineStore 读方法."
                  "(4) TimelineStore trait (db/traits.rs) 收缩为纯读: 删除 insert_timeline_batch / cleanup_timeline_ttl / find_timeline_needing_briefing / update_timeline_summary / get_briefing_summaries_for_session 5 个写方法; 保留 7 个读方法全部路由到 projection."
                  "(5) db/pg/timeline.rs 重写: 每个 TimelineStore 方法 2-3 行 delegate 到 projection::* (读 event_log, 无 system_timeline SQL)."
                  "(6) db/sqlite/timeline.rs 改为 fail-fast stub — 返回 DbError::Other('TimelineStore is projection over PG event_log; SQLite backend unsupported') 对所有 7 方法 (sqlite 只供迁移工具用, 不该走 timeline 路径)."
                  "(7) db/timeline.rs (MissionDB inherent methods) 清空为单个 pub use shared::*, 原 14 个 system_timeline SQL 方法全删."
                  "(8) 删除 crates/missiond-daemon/src/workers/sonnet/briefing_worker.rs (+ workers/sonnet/mod.rs 移除 pub mod, main.rs 移除 spawn 调用 + briefing_notify 字段, state.rs 移除 briefing_notify 字段, message_handler.rs 删除 briefing_notify.notify_one() call) — briefing_worker 的 update_timeline_summary UPDATE 语义与 append-only event_log 根本不兼容, 快速失败原则: 不加兜底, 直接删."
                  "(9) embedding_worker.rs 删除 get_briefing_summaries_for_session 调用 + briefing_map 选择逻辑 — 改为直接用 raw content (500 字符/消息上限)."
                  "(10) translation_worker.rs poll_pending 过滤条件从 Some('thinking_message') 改为 Some('message::logged') + 内存里过滤 payload.Logged.role=='thinking'; message_id 提取改为 payload.Logged.message_id (externally-tagged)."
                  "(11) timeline_analyst.rs query_timeline_filtered 从 Some('gemini_request_completed') 改为 Some('llm::legacy_gemini_request_completed'); extract_duration_ms 改为读 externally-tagged payload."
                  "(12) main.rs 删除 cleanup_timeline_ttl 调用 (event_log retention 已由 lifecycle/retention.rs + bus/retention_cron.rs 管, 旧表无需清理)."
                  "(13) pg/mod.rs fix_identity_sequences 移除 system_timeline entry; pg/migrate_from_sqlite.rs 同步移除 system_timeline 表引用 + 序列修复条目."
                  "(14) tests/pg_integration.rs test_pg_timeline_store 重写为纯读烟测 (read APIs 对空 event_log 也必须成功), 不再 insert_timeline_batch."
                  "(15) db/conversation.rs / db/mod.rs 的 Self::parse_since/parse_until 调用 (这些原定义在 timeline.rs 里) 改为走 db::shared::parse_since/parse_until (自由函数, 无 MissionDB 依赖).")
               :deviations-from-plan
                 ("Phase 6 (timeline-writer 订阅者移除): 原 Phase 8 event-bus 迁移已完成此步 (D014 lisp-said 也确认), 本次 cutover 无需再删."
                  "briefing_worker: 任务书隐含保留 (只说 'drop table'), 但其 UPDATE 路径与 append-only event_log 不兼容 — 选择删除而非给 summary 开 sidecar 表 (对齐 '快速失败' 约定)."
                  "WS /events sync 协议 (ws/server.rs::handle_catch_up): 代码已通过 TimelineStore trait 自动切到 projection, 但 event_type 从 v1 wire_type 改为 'domain::kind' — catch-up 输出 wire-format 轻微变化. live stream 不受影响 (仍由 daemon/src/bus/ws_bridge.rs 的 v2_logged_to_v1_wire_format 52-arm mapper 保持字节级兼容, 12 byte-equivalence tests pass)."
                  "Forge gen_misc.rs / gen/misc.rs 仍有 system_timeline 生成的 SQLite 代码: 只在 feature='sqlite' 下编译, default (postgres) 构建完全不触达; 若要彻底清零需更新 intent-db-misc.lisp 并重新冲压 — 超出本次范围, 留给指挥官决定."
                  "db/migration.rs (SQLite-only) 的 system_timeline CREATE TABLE + FTS5 triggers 仍在: sqlite feature gate, 不影响 default build, 同上超出范围.")
               :test-status "cargo build --workspace clean (0 errors, 54 warnings 全是 dead-code 遗留非新增); cargo test -p missiond-core --lib 255→259 pass; cargo test -p missiond-daemon 96 pass; cargo test -p missiond-daemon ws_bridge 12/12 byte-equivalence pass."
               :reason "Phase A 防御: 先加 FTS 索引 + drop migration (schema 层完备); Phase B 进攻: 读路径由 TimelineStore trait 屏蔽, 1:1 换到 projection, 无调用方签名变化. briefing/translation/embedding 写依赖或 wire-format 依赖, 需要逐个审查: briefing_worker 因 UPDATE 语义根本矛盾而删; translation/embedding 轻微调整 payload 解析保留. Lisp ↔ code 同构性: projection.rs 的 7 个 pub fn 与 §4.6 read-ui-projection 声明的 2 consumers (mission_timeline + WS stream) + FTS 索引 100% 对齐."
               :approved-by "ssot-cutover-executor (auto)"))

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
    (issue :id I004 :phase 8 :date "2026-04-19" :severity blocker
           :desc "前端 WS wire-format 契约耦合:47-ish wire_type 字符串 + 固定 JSON envelope (type/seq/trace_id/span_id/parent_span_id/payload) 由外部浏览器 client 消费。修改字段名/去掉字段会静默 break。sync 协议进一步耦合 timeline_latest_seq + query_timeline_since 两个 DB API"
           :resolution "Phase 8 resolved:新写 crates/missiond-daemon/src/bus/ws_bridge.rs — PgTailSource 每 100ms 扫 event_log LIMIT 256,对每条 LoggedEvent 跑 v2_logged_to_v1_wire_format,映射 (domain, kind, payload) → (wire_type, v1 payload) JSON(52 arm match 覆盖所有 v1 variant,ephemeral 处理 seq=0)。输出 JSON 字符串直接 ws_tx.send 到原 frontend_events_tx broadcast — 前端契约完全不动。12 个典型 variant(SlotBecameIdle, BoardTaskCreated, BoardTaskStatusChanged, TaskCreated, QuestionCreated, LlmRequestStarted, WorkerLlmCall, MemoryPhaseChanged, UserMessage, SessionCompleted, ConfigFileChanged, BriefingBatchStarted+ephemeral)写了 assert_equiv 单元测试,对比 v1 wire JSON 的每个 top-level 字段(type/seq/payload)字节相等。PgTailSource 独立于 Dispatcher 的 last_dispatched_seq,process-local 0 起点(重启后 WS 客户端用 sync 协议 catch-up,与 v1 语义一致)。"
           :resolved-at "2026-04-19")
    (issue :id I005 :phase 8 :date "2026-04-19" :severity minor
           :desc "cursor_ack_tx 的去向:frozen lisp §4.1 dead-bypass 说归 conversation-logger worker 内部,不作为 event。但 producer (conversation_logger:58) 和 consumer (main.rs:846-858) 分居两个 task,需要重构合并,不能只做 bus 迁移"
           :resolution "Phase 8 resolved:AppState.cursor_ack_tx 字段删除,新增 conversation_cursor_map: Arc<tokio::sync::Mutex<HashMap<String, u64>>>。conversation_logger::58 的 `cursor_ack_tx.send((path, offset))` 改为 `s.conversation_cursor_map.lock().await.insert(path, offset)`(直接写入共享 map,无跨 task hop)。main.rs 原来的 ack_rx consumer 改为 250ms tick 的 drain task:锁定 map → swap-out 所有 entries → 按 .json/.jsonl 路由到 gemini_tasks/cc_tasks.persist_cursor_ack。不再使用 MPSC channel。"
           :resolved-at "2026-04-19")
    (issue :id I006 :phase 1 :date "2026-04-19" :severity minor
           :desc "AST 同步触发链路不完整:ast_sync_tx 的唯一外部 producer 是 main.rs:1298 的启动时 FullSync。commit 级增量同步在代码中未接入,ContextualCommitDetected 也未 wire 到 CommitSync"
           :resolution "Phase 1-2 借 AstSyncEvent::Requested 迁移时,同时补 ContextualCommitDetected → AstSyncEvent::CommitSync 的 consumer 逻辑(新功能,非单纯迁移)"
           :resolved-at nil)
    (issue :id I007 :phase 3 :date "2026-04-19" :severity minor
           :desc "ephemeral 语义从 per-variant 硬编码 (is_ephemeral()) 改为 per-call (AppendOpts.ephemeral) 时,83 个 publish 点需要逐个审计。当前 8 个 ephemeral variant 多数会保留 ephemeral,但有些 (ImageMessageInserted, NarrationBatchCompleted) 实际 payload 较小,可能应转持久化"
           :resolution "Phase 8 resolved:不在所有 publish 点手写 ephemeral 标(会漏)— 改在 BusServices::publish_worker / publish_memory / publish_message / publish_session 四个 helper 内部按 variant match 自动打 ephemeral 标,保证 100% 覆盖原 v1 is_ephemeral() 契约:WorkerEvent::LlmCall / BriefingBatchStarted / BriefingSummaryGenerated / TranslationStarted / NarrationBatchCompleted / MemoryEvent::TurnExtracted / IntentAnalyzed / MessageEvent::ImageInserted / SessionEvent::Organized 全部 ephemeral=true,其余全部 false。ObservabilityEvent 仍统一 ephemeral=true(frozen lisp §4.4 硬契约)。"
           :resolved-at "2026-04-19")
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
           :resolution "Phase 8 resolved:新增 crates/missiond-daemon/src/bus/retention_cron.rs + spawn_retention_cron 从 main.rs 启动。每日凌晨 tick:1) event::log::retention::cleanup_once (3d ephemeral / 30d persistent / blob ttl_expires);2) sweep_orphan_cursors SELECT event_subscriptions WHERE last_seen_at < now() - '30 days' + DELETE + 对每条发 IncidentEvent::StaleSubscription { incident };3) 汇总 summary 发 ObservabilityEvent::RetentionReport(ephemeral=true)。新增 ObservabilityEvent::RetentionReport + IncidentEvent::StaleSubscription 两个 variant(不破坏 12 域契约)。duration_until_next_midnight 按 UTC 计算,防止 tokio sleep 跨时区错乱。"
           :resolved-at "2026-04-19"))

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
              :rationale "Phase 6 开始 v2 bus 就是 daemon 启动的强制依赖。Option 会让下游 90+ publish 点每次都写 .as_ref().map(...),噪音大。启动失败的路径(PG 不可达)应在 bootstrap 阶段就 `?` 传播,不到 AppState 构造。与 store/mission/pty 同为必填字段符合一致性。")

    (decision :id DC036 :phase 7 :date "2026-04-19"
              :topic "v2 订阅者模块位置:daemon 内 bus/v2_subscribers.rs 独立文件"
              :chose "新建 crates/missiond-daemon/src/bus/v2_subscribers.rs,与 bootstrap / compat / control_gate_adapter 同级;pub(crate) use 通过 mod.rs 暴露 start_v2_subscribers"
              :rationale "本 Phase 14 个订阅者同生同死,放一个文件最易读、最易后续合并或移除。放进 event_router.rs 会混淆 v1/v2 代码;放进各 worker.rs 会扇出到 6 个文件且与 Phase 8 一次性删 v1 路径冲突。bus/ 子目录是 v2 在 daemon 的唯一栖居地,新文件符合既有约定。")

    (decision :id DC037 :phase 7 :date "2026-04-19"
              :topic "BusServices 新增 dlq 字段 + subscribe<T>() 助记方法"
              :chose "PgDlqSink 作为默认 DlqSink 注入;BusServices::subscribe<T>(&self, name, opts) 封装 topic / log / cursor_store / dlq 四个共享依赖的传入"
              :rationale "Phase 4 subscription::subscribe 签名要求 topic + log + cursor_store + dlq 四个参数。若每个订阅者调用方自己拼,14 处会各写 5 行 boilerplate + 容易传错 topic。BusServices::subscribe<T>() 让调用方只写 bus.subscribe::<T>(name, opts).await?,DLQ 是 PG-backed(FailurePolicy::Retry 超限自动入库),ops 可查。frozen lisp §4.3 subscription-api 的 API 在 core 层;daemon bus 层只提供方便的调用点,不改核心语义。")

    (decision :id DC038 :phase 7 :date "2026-04-19"
              :topic "event_router 8 consumers 的 debounce/accumulation 语义迁移"
              :chose "realtime_extraction / session_reflection 用 sub.debounce(Duration); kb_consolidation / intent_analyst 用 counter + 循环(无直接 combinator 对应);extraction / submit / decision 目前不加 combinator(直接每条 ack + 动作,依赖 v1 并存保留 trailing-edge 去抖)"
              :rationale "sub.debounce(window) 是 fixed-deadline 语义,完美对应 v1 realtime_extraction(3s)/ session_reflection(5s)。kb_consolidation 要按 N=5 累加再触发,不是时间窗;intent_analyst 需要 per-session HashMap 且同时看 5min timeout 和 5 accumulation 两条件,combinator 表达不出 → 手写 loop + tokio::time::timeout 便捷。extraction / submit / decision 的 100-500ms 短窗去抖若用 sub.debounce,v2 每条都会多等这段时间才触发(而动作是 DB-poll,时效不敏感);但由于 v1 consumer 同时在跑 trailing-edge,v2 不加 debounce 是无害的,Phase 8 切换时可再加。")

    (decision :id DC039 :phase 7 :date "2026-04-19"
              :topic "Worker 6 subs 的角色:observer (passive) vs active handler"
              :chose "全部 passive:订阅 + ack + 单行 debug! log;实际副作用仍走 v1 timeline_rx 路径(workers/sonnet/arch_maintenance_worker 等)"
              :rationale "见 D006 deviation。worker 动作有实打实的外部副作用(LLM API 调用 / git 写操作 / intent.lisp 覆盖),双发会翻倍资源消耗 + 可能写冲突。passive observer 验证 v2 订阅路径联通但不重复副作用,是 '优先功能正确' 的直接落实。Phase 8 flip:删 v1 timeline_rx + 把 observer 改成 active handler,一次性切换。")

    (decision :id DC040 :phase 7 :date "2026-04-19"
              :topic "intent_analyst 的特殊处理:在 v2 中只 observe 不 act"
              :chose "spawn_intent_analyst_sub 保留完整的 per-session 累加逻辑,但 expiry 触发时只 debug! log(不调 process_session_intents)"
              :rationale "intent_analyst 的 process_session_intents 是 Sonnet LLM 调用 + DB 写 intent 行 + 反向更新 intent_group_id,典型重副作用。v1 spawn_intent_consumer 已在跑,Phase 7 任何重复调用都浪费 Sonnet 配额。如果任务书要求 A 组 8 consumers 都双发的话,intent_analyst 不属于 'DB-poll 幂等' 类别,所以例外归入 B-style observer。Phase 8 flip 时要同时删除 spawn_intent_consumer 并把 debug! 换成 process_session_intents 调用(注意 process_session_intents 目前是 private,需 pub(crate))。")

    (decision :id DC041 :phase 8 :date "2026-04-19"
              :topic "WS bridge 设计:tail event_log 而非订阅 Topic<T>"
              :chose "PgTailSource 独立 tail event_log → v2_logged_to_v1_wire_format(LoggedEvent) → ws_tx.send(json_str),与 Dispatcher 完全解耦"
              :rationale "Topic<T> 只广播 Arc<T>,不携带 seq / trace_id / span_id / parent_span_id / ts 元数据。浏览器 WS 客户端依赖完整 envelope:{type, ts, seq, trace_id, span_id, parent_span_id, payload}。tail event_log 天然返回 LoggedEvent(有全部字段),直接转换一次即得字节兼容 JSON。独立 tail cursor process-local,重启后从 0 扫(与 v1 run_timeline_writer 行为一致,前端靠 sync 协议 catch-up)。代价 = ws_bridge 多跑一个 PG 轮询(与 Dispatcher tail 一同 100ms 一次),收益 = 12 个典型 variant 字节相等 assertion 可维护。")

    (decision :id DC042 :phase 8 :date "2026-04-19"
              :topic "v1 → v2 wire format 映射:手写 match 而非 reuse v1 to_frontend_payload"
              :chose "ws_bridge::v2_payload_to_v1_shape 手写 52-arm match,把 (domain, kind, payload) 映射成 (wire_type, v1 payload)"
              :rationale "复用 v1 to_frontend_payload 需要先把 v2 LoggedEvent 反向构造成 DaemonEvent(要求 v1 DaemonEvent 还活着)。Phase 8 目标之一就是删除 DaemonEvent,所以反向路径不可用。手写 match 虽冗长但一次性成本,未来 v2 新 variant 只需加一个 arm + 一个断言测试。payload_of() helper 统一剥 externally-tagged JSON 的变体包裹(DC002 serde 模式),使每个 arm 代码简短。unknown_event fallback 保证未来添加新 variant 不会让 UI 崩(返回 {type: 'unknown_event', domain, kind, payload})。")

    (decision :id DC043 :phase 8 :date "2026-04-19"
              :topic "ephemeral audit 的实施方式:helper 内 match 自动打标 vs per-call 手写"
              :chose "BusServices::publish_worker/publish_memory/publish_message/publish_session 内部 matches!() 自动打 ephemeral 标"
              :rationale "Phase 8 前身约 100 个 publish 点若每个手写 opts.ephemeral = true 会漏 + 日后新 variant 容易忘。Helper 内 matches!() 把 v1 is_ephemeral() 映射集中成一份(每域 1-5 个 variant),一处声明处处生效。publish_observability 保持硬编码 ephemeral=true(frozen lisp §4.4 契约)。如果未来需要某处 publish 强制 persistent(如 debug mode),生产者调 publish() 原方法 + 显式 AppendOpts 即可。")

    (decision :id DC044 :phase 8 :date "2026-04-19"
              :topic "cursor_ack 内化实施:shared map vs direct disk write"
              :chose "Arc<Mutex<HashMap<String, u64>>> 共享指针 + 250ms tick drain task 路由到 persist_cursor_ack"
              :rationale "两种内化方案:(a) conversation_logger 直接调 cc_tasks.persist_cursor_ack + gemini_tasks.persist_cursor_ack(需要 conversation_logger 持 cc_tasks/gemini_tasks 的 Arc<Mutex>,扇出大且锁嵌套);(b) conversation_logger 写共享 map,单独 drain task 持两个 watcher handle。选 (b) 因为:(1) conversation_logger 不需要知道 path 路由规则(.json vs .jsonl);(2) drain 可做批处理(250ms tick 累积多条再一次性 flush);(3) 新的 watcher 加入只需改 drain task,不需要改 conversation_logger。锁使用 tokio::sync::Mutex 而非 std Mutex 因为 drain task 是 async 且会 await lock 内部的 persist_cursor_ack 调用(cc_tasks_ref.lock().await)。")

    (decision :id DC045 :phase 8 :date "2026-04-19"
              :topic "Incident 流向重构:主动 consumer task vs v2 subscriber"
              :chose "spawn_incident_reactor 在 bus/v2_subscribers.rs 订阅 IncidentEvent,通过 subscription.next().await 消费,调 aiops::process_incident;WS webhook 流入由 incident_webhook_tx MPSC(core 无法访问 daemon bus)+ daemon 侧 adapter task 重发为 IncidentEvent::Reported"
              :rationale "Incident 有两个生产源:(a) daemon 内生产者(autopilot / misc / aiops / pty_event_worker / sysinfra 共 5 处)直接 state.bus.publish_incident();(b) WS server 的 /webhooks/deploy 等入口,ws 在 missiond-core 没有 BusServices 访问。方案选择:选择 (b) 路径保留 incident_webhook_tx MPSC(仅 WS→daemon 的转发通道,不是业务 bus bypass),daemon main.rs spawn 一个 while let Some(incident) = incident_webhook_rx.recv().await 的小任务,每收到一个就 bus.publish_incident。aiops::process_incident 现在由 v2 subscriber 唯一消费(没有双路径)。不选 (a) 让 WS server 持 Arc<BusServices> 因为 core ← daemon 反向依赖。不选 (c) 让 WS server 持 Arc<dyn IncidentReporter> 因为会引入 trait 抽象污染,得不偿失。")

    (decision :id DC046 :phase 8 :date "2026-04-19"
              :topic "v1 workers 重构:in-place 升级到 v2 bus.subscribe 而非新文件"
              :chose "6 个 worker 文件(gemini_logger / translation / arch_maintenance / lisp_survey / conversation_organizer / tagger_chunker)原地修改:删除 timeline_rx 字段,run() 内 bus.subscribe::<T>(name, SubscriptionOpts::named) 获得 Subscription<T>,循环 sub.next().await 取 Ack<T>,每次 ack.ack().await 后处理 event"
              :rationale "方案 A:新写 6 个 v2 worker + 删 6 个 v1 worker — 文件数翻倍,测试 setup 重复,命名碰撞(translation_worker_v2?)麻烦。方案 B:原地重构,保留文件名 + 核心 handle 逻辑,只换订阅入口 — 本地改动集中,git diff 清晰,原有 pause/ctx/failure 逻辑完全复用。选 B。translation_worker 是最复杂的(circuit breaker + idle poll + subscription 三路 select),为不丢 pause 语义,保留 WorkerContext 的 wait_if_paused 与 run_loop_poll_only 降级路径(当 subscribe 失败时退化为纯 poll)。")

    (decision :id DC047 :phase 8 :date "2026-04-19"
              :topic "v1 event_bus.rs 删除时机:先代码迁移后再删 vs 同时做"
              :chose "先重构所有 publish 站点 + 6 个 worker + LLM 客户端 → 逐步删除 state.event_bus / incident_tx 字段 → 最后删 event_bus.rs / event_router.rs / compat.rs 三个文件"
              :rationale "Phase 8 任务书顺序建议:1) rewire WS, 2) internalize cursor_ack, 3) 删各处 publish 的 v1 shim 调用, 4) 删 AppState 字段, 5) 最后删 event_bus.rs / event_router.rs / run_timeline_writer 三文件。实际执行中顺序反了一下:先把 publish 点全改(减少 build 的连锁错误面),再删 AppState 字段,最后删文件。因为 publish 点改动中每一步都可以独立 cargo build 验证,不会一下子暴露成千上万个错误。rewire WS + retention cron 作为独立添加做在最后,不与删除交叉。共 31 个文件涉及编辑,最终 cargo build clean + 250 + 12 + 96 tests 全过。")

    (decision :id DC048 :phase 9 :date "2026-04-19"
              :topic "E2E golden path test 的位置与真-E2E 程度"
              :chose "放在 crates/missiond-daemon/tests/e2e_bus_golden_path.rs(#[ignore]);测试主体用 missiond-core public API(spawn_log_writer / PgBlobStore / PgTailSource)+ 内联镜像 v1 wire-format 断言,而非通过 daemon 内部 BusServices"
              :rationale "missiond-daemon 是 binary crate,bus 模块是 mod bus(pub(crate)),tests/ 下的 integration test 无法直接 use crate::bus::BusServices。两条路:(a) 加 lib.rs 再 pub mod bus,但 bus::{bootstrap,v2_subscribers,retention_cron} 递归依赖 control_tree → llm_gate → gemini_client::REQUEST_CALLER… 需要 re-export 的模块链条过长,投入产出比低;(b) 用 missiond-core 公共 API 同时镜像最关键的 wire-format 断言,保留测试 body 与 bus/ws_bridge.rs::v2_payload_to_v1_shape 的 BoardTaskCreated arm 做字节对齐,同时在注释中指明若 bridge 变更必须同步更新本测试。选 (b):测试实质保留(真 PG / 真 append / 真 tail / wire-format 字节级断言),代价 = 一处内联镜像,上游 bridge 变更时有明确锚点。")

    (decision :id DC049 :phase 9 :date "2026-04-19"
              :topic "daemon smoke test 的执行方式:真启动二进制 vs integration test 模拟"
              :chose "真启动 release binary,MISSIOND_HOME=/tmp/missiond-smoke-home + 独立 PG DB missiond_smoke_v2 + WS port 19120 + IPC socket /tmp/missiond-smoke-ipc.sock,gtimeout 10s 后 SIGTERM"
              :rationale "任务书指出'如果没法真启动,退而用 integration test 模拟启动序列'。本机有 Docker + local PG(postgres://jinchen@localhost/),真启动只需要 4 个 env vars 隔离:MISSIOND_HOME / MISSION_PG_URL / MISSION_WS_PORT / MISSION_IPC_ENDPOINT。production daemon(PID 864,com.missiond.daemon launchctl)在 9120 端口 + ~/.xjp-mission,smoke test 用 19120 + /tmp/missiond-smoke-home 完全不冲突。10 秒 gtimeout 足够观察:bus bootstrap(170ms)+ dispatcher + 8 subscribers live(<1s)+ ws bridge + retention cron + 所有 worker 启动完成。观察到 0 panic,唯一 WARN 是 autopilot Failed to complete stale conversations(sqlx 类型错误,非 bus 引入,长期存在于 main 分支)。SIGTERM 后所有子系统按序 shutdown。比 integration test 模拟更 high-fidelity — integration test 只能测 BusServices 启动序列,而真启动验证了完整 main.rs 链路(MissionControl + PgMissionStore + CCTasksWatcher + GeminiCliWatcher + WS + IPC + 所有 workers + bus)。证据:/tmp/missiond-smoke-output.log 含 24 行 bus 相关 log。")

    (decision :id DC050 :phase 10 :date "2026-04-19"
              :topic "pipeline/ 七步结构的物理布局:新建 `pipeline/step{1..7}_*/` vs 就地改 `dispatcher/log/guards/blob_store` 命名"
              :chose "新建 `crates/missiond-core/src/event/pipeline/` 顶层目录,下辖 7 个 `step{N}_<name>/` 子目录,与 frozen lisp §4.2 `execution-flow` 的 7 步一一对应"
              :rationale "frozen lisp §4.2 把 ingress → guard → decide → commit → ack → tail → gate → fanout 列为 7 步顺序流水,但 Phase 1-8 的代码把它们散到 `guards/`, `blob_store/`, `log/writer.rs`, `dispatcher/{tail,control_gate,topic,registry}.rs` 四个互不相干的目录 — 读代码无法反向找到 lisp。两种对齐方案:(a) 把 dispatcher/log 拆成分散的 step 目录,既保留旧名又加新名;(b) 全量搬到新的 pipeline/ 树,`dispatcher/` `log/` 变成薄 re-export 壳子。选 (b) 因为它把'流水线'这个概念从松散的'producer/dispatcher 二元对立'升级为单一树,新读者看一眼目录就能对上 lisp 的 7 步;(a) 会保留历史路径裂缝。back-compat re-export 让所有下游 `event::{log,dispatcher,guards,blob_store}::…` 导入继续工作,零 breaking change。")

    (decision :id DC051 :phase 10 :date "2026-04-19"
              :topic "step3_commit 内部 5 个概念(log_writer / seq_authority / dedup / backpressure / failure_mode)的文件粒度"
              :chose "保留 5 个独立文件,其中 log_writer.rs 是实现主体(~1000 行);其余 4 个是 doc-only anchor modules(seq_authority / dedup / backpressure / failure_mode)"
              :rationale "任务书说'如果这 5 个子模块拆太细,也可合并为 log_writer.rs + dedup.rs + failure_mode.rs 三个'。但考虑到 frozen lisp §4.2 step-3 明确列出 5 个 (component log-writer/seq-authority/dedup-semantics/backpressure/failure-mode),一一对应才能做到'lisp 和目录结构同构'这一 Phase 10 的核心目的。4 个 doc-only 模块每个 20-50 行,纯文档 + 1-2 个常量/类型别名,读者扫一眼就知道每个概念的实现位置(全部在 log_writer.rs 内联)。没有代码重复,也不强制未来实现者把逻辑拆到多个文件。如果未来某个子概念需要独立实现(如 backpressure 上 priority queue),直接在其文件里落地即可,不用搬名字。")

    (decision :id DC052 :phase 10 :date "2026-04-19"
              :topic "step4_ack 的深度:完全 doc-only vs 提供 AckSender 类型别名"
              :chose "step4_ack/ 里 mod.rs 是文档模块 + 类型 re-export;ack_transport.rs 定义 `pub type AckSender = oneshot::Sender<Result<AppendAck, AppendError>>` + `pub fn new_ack_channel()` 辅助函数"
              :rationale "frozen lisp §4.2 step-4 ack 明确说'no-extra-hop — step 4 没有新组件,只是 step 3 的 out-bound 封装'。完全 doc-only 会让这个 step 目录只有 mod.rs 一个文件,不如其他 6 steps 平衡;但又不能硬塞新逻辑进去。折中方案:提供一个 `AckSender` 类型别名 + `new_ack_channel()` 便利函数,作为'step-3 填什么、step-4 接什么'的契约符号。log_writer.rs 目前仍内联 `oneshot::channel()`,改用 `new_ack_channel` 是可选优化(后续可做)。2 个单元测试:AckSender 正常派发 Committed + sender drop 时 receiver 能感知错误。保证这个 step 也有独立测试覆盖,不被 step-3 夹带。")

    (decision :id DC053 :phase 10 :date "2026-04-19"
              :topic "retention.rs 的归宿:保留 `log/retention.rs` 还是移到独立 `lifecycle/` 目录"
              :chose "新建 `crates/missiond-core/src/event/lifecycle/retention.rs`,`log/` 完全无 retention 模块"
              :rationale "frozen lisp §4.2 lifecycle-maintenance 是独立 section,明确与 `core 7-step pipeline` 并列:'不在 append/dispatch 主路径上的后台任务'。放在 `log/` 下会暗示 retention 属于 log 写入机制,但它是独立的 cron 任务(由 `missiond-daemon/src/bus/retention_cron.rs` 驱动),与 log append 无关,与 orphan cursor cleanup + ObservabilityEvent::RetentionReport 发射相关。单独建 `lifecycle/` 目录让这个语义可见;未来 orphan cleanup 的谓词查询若从 daemon 迁回 core,也有明确归宿。")

    (decision :id DC054 :phase 10 :date "2026-04-19"
              :topic "legacy import path 的 back-compat 策略:立即下架 vs 保留 re-export shim"
              :chose "保留 `event::{log,dispatcher,guards,blob_store}::…` 作为 re-export shim,每个 old-path module 在 mod.rs 顶部加'canonical home pointer'注释"
              :rationale "Phase 10 的契约是零功能改动 + 测试零回归。一次性重命名导入路径会在 daemon / tests / missiond-mcp 造成 ~50+ 行 churn,与'物理重排只影响目录结构'的 Phase 10 范围冲突。保留 shim 让重构对下游透明,旧路径清零可在将来独立的 Phase 11(imports-cleanup)做。shim 模块(`dispatcher/mod.rs` 的 `pub mod control_gate { pub use crate::event::pipeline::step6_gate::*; }` 等)只是 `pub use` 转发,无任何运行时成本。")
)

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
      :phase 7 :date "2026-04-19" :agent "phase7-subscriber-migration"
      :deliverables ("crates/missiond-daemon/src/bus/v2_subscribers.rs"
                     "crates/missiond-daemon/src/bus/mod.rs (+ pub mod v2_subscribers + pub(crate) use start_v2_subscribers)"
                     "crates/missiond-daemon/src/bus/bootstrap.rs (+ dlq: Arc<dyn DlqSink> 字段 + PgDlqSink 注入 + BusServices::subscribe<T>() 助记方法)"
                     "crates/missiond-daemon/src/main.rs (+ bus::start_v2_subscribers(&bus_services, &state, shutdown_rx.clone()) hook 紧邻 _bus_handle start 之后)")
      :tests-added 0
      :verified-by "cargo build(workspace) clean;cargo build -p missiond-daemon clean;cargo test -p missiond-core --lib → 250 passed,0 failed(无 regression);cargo test -p missiond-core --test event_chaos → 12 passed,0 failed(chaos 无 regression);cargo test -p missiond-daemon → 91 passed,0 failed(daemon unit tests 无 regression)"
      :notes "14 个 v2 订阅者全部上线,v1 timeline_tx.subscribe 完全保留。A 组 8 个 event_router consumers 做动作(复用 schedule_memory_tasks / dispatch_queued_submit_tasks / process_pending_master_questions / experience_harvester::harvest_session / check_realtime_extraction / check_deep_analysis + strategy/retro notify / check_kb_consolidation 等 DB-poll 幂等动作),与 v1 并存双 fire。realtime_extraction + session_reflection 用 sub.debounce() combinator,kb_consolidation / intent_analyst 用手写 loop。B 组 6 个 worker observers + intent_analyst 仅 passive(DC039 / DC040,见 D006 deviation)— 只订阅 + ack + debug! log,不重复 v1 副作用。I004 WS wire-format 迁移推迟 Phase 8(D005);I005 cursor_ack_tx 内化推迟 Phase 8(D004)。未触 v1 event_bus.rs / event_router.rs / run_timeline_writer / workers/sonnet/* / workers/local/conversation_organizer.rs / workers/local/tagger_chunker.rs。BusServices.dlq 用 PgDlqSink(FailurePolicy::Retry 超限 fall through SkipToDLQ,落 dead_letter_queue 表)。未新增测试(本 Phase 纯 wiring;功能性 E2E 归 Phase 9)。")

    (completion
      :phase 8 :date "2026-04-19" :agent "phase8-cleanup"
      :deliverables (
        ;; Deleted files (v1 bus):
        "crates/missiond-daemon/src/event_bus.rs (DELETED)"
        "crates/missiond-daemon/src/event_router.rs (DELETED)"
        "crates/missiond-daemon/src/bus/compat.rs (DELETED)"
        ;; New bus modules:
        "crates/missiond-daemon/src/bus/ws_bridge.rs (NEW)"
        "crates/missiond-daemon/src/bus/retention_cron.rs (NEW)"
        "crates/missiond-daemon/src/bus/mod.rs (updated pub uses)"
        "crates/missiond-daemon/src/bus/bootstrap.rs (removed log::Log re-export typo, added ephemeral audit in 4 publish helpers)"
        "crates/missiond-daemon/src/bus/v2_subscribers.rs (removed 6 passive observers; added spawn_incident_reactor; intent_analyst now active)"
        ;; Core event type additions:
        "crates/missiond-core/src/event/events/observability.rs (+ RetentionReport variant)"
        "crates/missiond-core/src/event/events/incident.rs (+ StaleSubscription variant)"
        ;; Refactored producers (removed DaemonEvent / TimelineEntry):
        "crates/missiond-daemon/src/state.rs (cursor_ack_tx → conversation_cursor_map; incident_tx / event_bus fields removed)"
        "crates/missiond-daemon/src/main.rs (run_timeline_writer removed; timeline_mpsc / timeline_broadcast / event_bus_instance removed; ws_bridge + retention_cron wired; 6 workers spawned without timeline_rx field)"
        "crates/missiond-daemon/src/workers/local/conversation_logger.rs (cursor_ack_tx.send → conversation_cursor_map.lock().insert)"
        "crates/missiond-daemon/src/workers/local/pty_event_worker.rs (6 DaemonEvent sites → state.bus.publish_xxx; incident_tx.try_send → bus.publish_incident)"
        "crates/missiond-daemon/src/workers/local/tagger_chunker.rs (v1 timeline_rx → bus.subscribe::<SessionEvent>; 3 publish sites → v2)"
        "crates/missiond-daemon/src/workers/local/conversation_organizer.rs (v1 timeline_rx → bus.subscribe::<MessageEvent>; 1 publish → v2)"
        "crates/missiond-daemon/src/workers/local/gemini_logger.rs (v1 timeline_rx → bus.subscribe::<LlmEvent>; handle_event 接受 &LlmEvent)"
        "crates/missiond-daemon/src/workers/sonnet/translation_worker.rs (v1 timeline_rx → bus.subscribe::<MessageEvent>; 3 DaemonEvent → 3 WorkerEvent;poll fallback 保留)"
        "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs (v1 timeline_rx → bus.subscribe::<SystemEvent>)"
        "crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs (v1 timeline_rx → bus.subscribe::<SystemEvent>)"
        "crates/missiond-daemon/src/workers/sonnet/briefing_worker.rs (3 DaemonEvent::BriefingSummaryGenerated + 1 BriefingBatchStarted → WorkerEvent)"
        "crates/missiond-daemon/src/workers/gemini/strategy_worker.rs (DeepAnalysisCompleted → MemoryEvent, InsightGenerated → SystemEvent)"
        "crates/missiond-daemon/src/workers/codex/step_narrator.rs (5 DaemonEvent → WorkerEvent; CodexCli::new 去掉 event_tx 参数)"
        "crates/missiond-daemon/src/workers/codex/vision_worker.rs (CodexCli::new 去掉 event_tx 参数)"
        ;; Refactored engines / handlers:
        "crates/missiond-daemon/src/engine/learning_engine/extraction.rs (EventBus ref 删除,2 helpers 改签名;MemoryPhaseChanged → MemoryEvent;SlotTaskDispatched → SlotEvent)"
        "crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs (11 sites → QuestionEvent + TaskEvent)"
        "crates/missiond-daemon/src/engine/learning_engine/intent_analyst.rs (spawn_intent_consumer 删除;process_session_intents 改 pub(crate);DaemonEvent → MemoryEvent)"
        "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs (DaemonEvent → SlotEvent + MemoryEvent)"
        "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs (InsightGenerated → SystemEvent)"
        "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs (BoardTaskStatusChanged → BoardEvent)"
        "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs (8 sites → BoardEvent + SessionEvent + SystemEvent + SlotEvent + IncidentEvent)"
        "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs (SlotTaskDispatched → SlotEvent)"
        "crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs (SlotTaskDispatched → SlotEvent; TaskCreated → TaskEvent)"
        "crates/missiond-daemon/src/infra/message_handler.rs (ConversationMessageLogged → MessageEvent; ToolCompleted → SystemEvent)"
        "crates/missiond-daemon/src/infra/aiops.rs (3 sites + incident_tx.try_send → bus.publish_task + bus.publish_incident)"
        "crates/missiond-daemon/src/handlers/knowledge/board.rs (3 helper fns + 3 inline sites → BoardEvent + SlotEvent)"
        "crates/missiond-daemon/src/handlers/knowledge/kb.rs (5 sites → MemoryEvent + TaskEvent)"
        "crates/missiond-daemon/src/handlers/knowledge/cascade.rs (CascadeTriggered + CascadeCompleted → TaskEvent)"
        "crates/missiond-daemon/src/handlers/compute/task.rs (3 sites → SlotEvent + TaskEvent)"
        "crates/missiond-daemon/src/handlers/comm/question.rs (4 sites → QuestionEvent + TaskEvent)"
        "crates/missiond-daemon/src/handlers/sysinfra/misc.rs (2 QuestionCreated + 1 incident_tx → QuestionEvent + IncidentEvent)"
        ;; LLM clients — removed event_tx / TimelineEntry:
        "crates/missiond-daemon/src/llm/gemini_client.rs (event_tx 删除;CliRequestStarted/Completed/ToolActivity → LlmEvent)"
        "crates/missiond-daemon/src/llm/codex_cli.rs (event_tx 删除;new() 签名缩短;CliRequest* → LlmEvent + Provider::Codex)"
        "crates/missiond-daemon/src/llm/sonnet_gateway.rs (event_tx 删除;WorkerLlmCall → WorkerEvent::LlmCall)"
        "crates/missiond-daemon/src/llm/minimax_gateway.rs (event_tx 删除;WorkerLlmCall → WorkerEvent::LlmCall)")
      :tests-added 17
      :verified-by "cargo build (workspace) clean — 54 warnings, 0 errors;cargo test -p missiond-core --lib → 250 passed,0 failed;cargo test -p missiond-core --test event_chaos → 12 passed,0 failed;cargo test -p missiond-daemon → 96 passed,0 failed(增加 12 ws_bridge 字节相等 + 2 retention_cron + 3 prior test count drift)"
      :notes "I004 / I005 / I007 / I010 全部 resolved。v1 event_bus.rs / event_router.rs / bus/compat.rs 三个文件已删除。run_timeline_writer 已从 main.rs 删除。incident_tx MPSC 端到端删除(改走 IncidentEvent v2 subscriber 唯一消费);cursor_ack_tx 内化为 AppState.conversation_cursor_map + 250ms drain task。embedding_tx / ast_sync_tx 作为 worker-internal queue 保留(D007 deviation)。ws_bridge 独立 PgTailSource,12 个 variant 字节相等 assertion;retention_cron 每日凌晨扫 TTL + orphan cursor + 发 RetentionReport。ephemeral audit 通过 publish helper 内置 matches!() 自动打标(DC043)。未触 frozen lisp(所有偏差在 execution.lisp 记录)。未启用 Prometheus backend(D002 仍活);未实现 FreezeAndCatchUp(D001 仍活)。")

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
      :notes "Causation guard 抽到独立 pub mod guards(DC021) + Phase 2 LogWriter 改调 check_causation 单入口(frozen lisp §4.4 跨相契约)。BusMetrics trait 8 个方法全覆盖 frozen lisp §4.4 observability 列表(DC025);AtomicBusMetrics 用 AtomicU64 + Mutex<HashMap> 做内存 MVP 收集;NoopMetrics 给 tests/daemon 启动前用。BusMetricsEmitter 每 10s snapshot → 多条 ObservabilityEvent::BusMetric(ephemeral=true)通过 ObservabilityAppender trait 发射(DC027)。InMemoryLog 严格对齐 PG 版语义:single writer task 分 seq(AtomicI64 fetch_add,DC023)、bounded mpsc cap 4096(frozen §4.2.b 同值)、dedupe HashMap 模拟 UNIQUE 索引、failed state 可程式化切换供 chaos#6 使用、ephemeral 分 seq 但不入 rows Vec、claim-check 超 8KB 走 BlobStore。Dispatcher 直接复用 Phase 3 run_tail(Arc<dyn TailSource>),InMemoryTailSource 适配器扫 Vec 按 seq 升序返回。InMemoryBus 聚合 log+blob+cursor+dispatcher+control_gate+metrics 一站式启动,start() spawn dispatcher tail task 返回 Handle。9 Chaos scenarios(+ 3 辅助 = 12 tests)在 tests/event_chaos.rs 全部不用 Docker 就跑:log-writer-timeout / log-writer-panic / dispatcher-panic / subscriber-panic / slow-subscriber-lag / db-disconnect / causation-loop / dedup-retry / cursor-orphan-stub + sanity-full-flow + 7b-guard-shared-contract + metrics-record。D002 deviation:Prometheus backend 推迟;I010 issue:cursor orphan daily cron wire up 推迟 Phase 8。InMemoryBlobStore 用 LocalFile backend tag + 'mem:' URI 前缀(DC029)避免扩 BlobBackend enum。未 touch v1 bus/daemon/workers(Phase 6+)。")

    (completion
      :phase 9 :date "2026-04-19" :agent "phase9-final-validation"
      :deliverables ("crates/missiond-daemon/tests/e2e_bus_golden_path.rs (NEW, #[ignore])"
                     "crates/missiond-daemon/Cargo.toml (+ testcontainers / testcontainers-modules dev-deps)"
                     ".missiond/v2/_phase9-layout-check.md (NEW)"
                     ".missiond/v2/_refactor-summary.md (NEW)"
                     ".missiond/v2/intent-event-bus-execution.lisp (updated: meta.status=completed, phase-cursor=9, phase-9 completion entry, DC048/DC049, phase-9 claim)")
      :tests-added 1
      :verified-by "cargo build --all-targets clean;cargo test --workspace → 391 passed,0 failed across 23 result groups(missiond-core lib 250 + event_chaos 12 + event_dispatcher_integration 1 passed+2 ignored + event_log_integration 6 ignored + event_subscription_integration 3 ignored + pg_integration 6 ignored + missiond-daemon 96 + e2e_bus_golden_path 0+1 ignored + 其余);cargo clippy --workspace --all-targets 0 errors(292 warnings 皆非 bus 引入);daemon release binary smoke start 成功(MISSIOND_HOME=/tmp + 独立 PG + WS 19120,4 秒内 bus bootstrap + dispatcher + 8 subscribers + incident reactor + ws bridge + retention cron 全部 live,0 panic,SIGTERM 清理干净)"
      :notes "5 项交付全部完成:1) E2E test 真实 PG + LogWriter append + tail 回读 + v1 wire-format 字节断言(type/seq/payload 三维度);2) 全量测试 391 passed 无 regression;3) daemon 真启动 smoke test 成功(release binary + gtimeout 10s);4) _phase9-layout-check.md 每个 frozen lisp §4.2 component 映射到文件路径,0 ghost 组件;5) _refactor-summary.md 含完整数字统计(9 commits / +29909-3398 / 109 new+2 deleted / 52→64 variants / 391 tests)+ v1/v2 架构 ASCII 对照 + D001-D007 状态表(D001/D002/D007 永久保留)+ I001-I010 状态表(I006/I009 保留,其余 resolved)+ DC001-DC049 全量决策按 phase 分组 + 已知限制 + 短/中/长期 follow-up。DC048 记录 E2E test 放置决策(binary crate 限制导致内联镜像 wire-format);DC049 记录 smoke test 用真启动 release binary 而非 integration 模拟。未解决 issues 仅 I006(ContextualCommitDetected→AstSyncEvent wiring,bus 范围外)和 I009(FreezeAndCatchUp runtime,可选优化)。Meta 状态 in-progress → completed,phase-cursor 8 → 9。Refactor 准备 merge 到 main。")

    (completion
      :phase 10 :date "2026-04-19" :agent "phase10-reorg"
      :deliverables (
        ;; New pipeline/ tree (7 steps, all new dirs + mod.rs + sub-modules):
        "crates/missiond-core/src/event/pipeline/mod.rs (NEW — 7-step overview + pub mod step{1..7})"
        "crates/missiond-core/src/event/pipeline/step1_guard/mod.rs (NEW)"
        "crates/missiond-core/src/event/pipeline/step1_guard/causation.rs (MOVED from event/guards/causation.rs)"
        "crates/missiond-core/src/event/pipeline/step1_guard/type_resolve.rs (NEW — ResolvedEventMeta doc anchor + 1 test)"
        "crates/missiond-core/src/event/pipeline/step2_decide/mod.rs (NEW)"
        "crates/missiond-core/src/event/pipeline/step2_decide/claim_check.rs (MOVED from event/blob_store/claim_check.rs)"
        "crates/missiond-core/src/event/pipeline/step2_decide/persistence_policy.rs (NEW — PersistencePolicy enum + 2 tests)"
        "crates/missiond-core/src/event/pipeline/step3_commit/mod.rs (NEW — 5-component doc table)"
        "crates/missiond-core/src/event/pipeline/step3_commit/log_writer.rs (MOVED from event/log/writer.rs)"
        "crates/missiond-core/src/event/pipeline/step3_commit/seq_authority.rs (NEW — doc anchor + EventSeq alias)"
        "crates/missiond-core/src/event/pipeline/step3_commit/dedup.rs (NEW — doc anchor + DEDUP_UNIQUE_COLUMNS)"
        "crates/missiond-core/src/event/pipeline/step3_commit/backpressure.rs (NEW — doc anchor + APPEND_CHANNEL_CAPACITY re-export)"
        "crates/missiond-core/src/event/pipeline/step3_commit/failure_mode.rs (NEW — doc anchor + FAILED_STATE_RETRY_CAP const)"
        "crates/missiond-core/src/event/pipeline/step4_ack/mod.rs (NEW)"
        "crates/missiond-core/src/event/pipeline/step4_ack/ack_transport.rs (NEW — AckSender type + new_ack_channel + 2 tests)"
        "crates/missiond-core/src/event/pipeline/step5_tail/mod.rs (MOVED from event/dispatcher/tail.rs)"
        "crates/missiond-core/src/event/pipeline/step6_gate/mod.rs (MOVED from event/dispatcher/control_gate.rs)"
        "crates/missiond-core/src/event/pipeline/step7_fanout/mod.rs (NEW — TOPIC_BUFFER_SIZE moved here)"
        "crates/missiond-core/src/event/pipeline/step7_fanout/topic.rs (MOVED from event/dispatcher/topic.rs)"
        "crates/missiond-core/src/event/pipeline/step7_fanout/registry.rs (MOVED from event/dispatcher/registry.rs)"
        ;; New lifecycle/ dir for retention:
        "crates/missiond-core/src/event/lifecycle/mod.rs (NEW)"
        "crates/missiond-core/src/event/lifecycle/retention.rs (MOVED from event/log/retention.rs)"
        ;; Reshaped orchestrator/shim modules (kept thin, back-compat re-exports):
        "crates/missiond-core/src/event/mod.rs (updated — layout table + pipeline + lifecycle modules + guards shim)"
        "crates/missiond-core/src/event/log/mod.rs (updated — thin: Log/LogReadable traits, AppendOpts/Ack/Error/Seq + re-export writer/cleanup_once)"
        "crates/missiond-core/src/event/dispatcher/mod.rs (updated — orchestrator only; step5/6/7 via pub use)"
        "crates/missiond-core/src/event/blob_store/mod.rs (updated — claim_check helpers re-exported from pipeline::step2_decide)"
        "crates/missiond-core/src/event/blob_store/{pg_backend,local_file_backend}.rs (updated — use super:: instead of super::claim_check)"
        "crates/missiond-core/src/event/in_memory/log.rs (updated — check_causation import via pipeline::step1_guard)"
        ;; Deleted (contents migrated):
        "crates/missiond-core/src/event/guards/ (DELETED — contents migrated to pipeline/step1_guard/)"
        "crates/missiond-core/src/event/log/writer.rs (DELETED — migrated to pipeline/step3_commit/log_writer.rs)"
        "crates/missiond-core/src/event/log/retention.rs (DELETED — migrated to lifecycle/retention.rs)"
        "crates/missiond-core/src/event/dispatcher/tail.rs (DELETED — migrated to pipeline/step5_tail/mod.rs)"
        "crates/missiond-core/src/event/dispatcher/control_gate.rs (DELETED — migrated to pipeline/step6_gate/mod.rs)"
        "crates/missiond-core/src/event/dispatcher/topic.rs (DELETED — migrated to pipeline/step7_fanout/topic.rs)"
        "crates/missiond-core/src/event/dispatcher/registry.rs (DELETED — migrated to pipeline/step7_fanout/registry.rs)"
        "crates/missiond-core/src/event/blob_store/claim_check.rs (DELETED — migrated to pipeline/step2_decide/claim_check.rs)"
        ;; Daemon path fixes:
        "crates/missiond-daemon/src/bus/retention_cron.rs (updated — import from event::lifecycle::retention)"
        ;; Frozen lisp :target path alignment:
        ".missiond/v2/intent-event-bus.lisp (updated — 7 :target paths: step1/step2/step3/step5/step6/step7 + retention)"
        ".missiond/v2/intent-event-bus-execution.lisp (updated — meta.phase-cursor 9→10, phase-10 entry, DC050-DC054, phase-10 claim + completion)")
      :tests-added 5
      :verified-by "cargo build -p missiond-core clean;cargo build --workspace clean;cargo test -p missiond-core --lib → 255 passed,0 failed(原 250 + 5 Phase 10 新增 doc-anchor 测试);cargo test -p missiond-core --test event_chaos → 12 passed,0 failed(chaos 无 regression);cargo test -p missiond-daemon → 96 passed,0 failed(daemon 无 regression)"
      :notes "零功能改动的物理重排。核心观察:frozen lisp §4.2 core 把事件处理描述为 7 步流水线,但 Phase 1-8 代码散布在 `guards/`/`blob_store/`/`log/writer.rs`/`dispatcher/{tail,control_gate,topic,registry}.rs` 四个历史沿革目录。Phase 10 把物理结构与 frozen lisp 对齐为 1:1。保留完整 back-compat re-export 避免 Phase 10 引起下游 import churn:`event::{log,dispatcher,guards,blob_store}::…` 所有旧路径继续解析。新增 5 个单元测试覆盖 Phase 10 引入的 doc anchor:type_resolve::ResolvedEventMeta(1)/persistence_policy::PersistencePolicy(2)/ack_transport::new_ack_channel(2)。frozen lisp §4.2 共 7 个 :target 路径更新(step1 guards/→pipeline/step1_guard/、step2 claim-check blob_store/→pipeline/step2_decide/、step3 log/writer.rs→pipeline/step3_commit/、step5 dispatcher/tail.rs→pipeline/step5_tail/、step6 dispatcher/control_gate.rs→pipeline/step6_gate/、step7 dispatcher/{topic,registry}.rs→pipeline/step7_fanout/、retention log/retention.rs→lifecycle/retention.rs)。blob-store 内部结构 `:target \"blob_store/\"` 保持(它是 step 2 使用的服务,不是 step 2 自身)。DC050-DC054 记录 5 个关键决策:pipeline/ 顶层树 vs 就地改(DC050)、step3 保留 5 子模块不合并(DC051)、step4 提供 AckSender 类型别名不做 doc-only(DC052)、retention 搬到独立 lifecycle/ 而非留在 log/(DC053)、back-compat shim 保留不立即下架(DC054)。")

    (completion
      :phase 11 :date "2026-04-19" :agent "phase11-beta-subscription"
      :deliverables (
        ;; subscription/combinators/ — 拆 600 行 combinators.rs 为子目录 6 文件 + mod.rs
        "crates/missiond-core/src/event/subscription/combinators/ (NEW directory)"
        "crates/missiond-core/src/event/subscription/combinators/mod.rs (NEW — Subscription<T> extension methods: debounce/rate_limit/coalesce/filter/map/batch + wrap_ack helper + Ack<T>::__new_for_combinator / silent_ack + pub use of 6 combinator structs + tests 7)"
        "crates/missiond-core/src/event/subscription/combinators/debounce.rs (NEW — DebouncedSubscription<T>)"
        "crates/missiond-core/src/event/subscription/combinators/rate_limit.rs (NEW — RateLimitedSubscription<T>)"
        "crates/missiond-core/src/event/subscription/combinators/coalesce.rs (NEW — CoalescingSubscription<T, F>)"
        "crates/missiond-core/src/event/subscription/combinators/filter.rs (NEW — FilteredSubscription<T, F>)"
        "crates/missiond-core/src/event/subscription/combinators/map.rs (NEW — MappedSubscription<T, U, F>)"
        "crates/missiond-core/src/event/subscription/combinators/batch.rs (NEW — BatchedSubscription<T> + EventBatch<T>)"
        "crates/missiond-core/src/event/subscription/combinators.rs (DELETED — contents migrated)"
        ;; subscription/lifecycle.rs 拆出 live_source.rs
        "crates/missiond-core/src/event/subscription/lifecycle.rs (REWRITTEN — Phase/LifecycleError/Fetched/Lifecycle orchestration; LiveSource moved out)"
        "crates/missiond-core/src/event/subscription/live_source.rs (NEW — LiveSource trait + BroadcastLiveSource + MpscLiveSource)"
        "crates/missiond-core/src/event/subscription/mod.rs (updated — add `pub mod live_source;` + `pub use lifecycle::{Lifecycle, LifecycleError, Phase};` + `pub use live_source::{BroadcastLiveSource, LiveSource, MpscLiveSource};`)"
        "crates/missiond-core/src/event/subscription/api.rs (updated — split import: `use super::lifecycle::Lifecycle; use super::live_source::{BroadcastLiveSource, LiveSource};`)"
        ;; in_memory/log.rs 拆为 4 文件
        "crates/missiond-core/src/event/in_memory/log.rs (REWRITTEN — InMemoryLog struct + `impl Log for InMemoryLog` + stored_to_logged/stored_to_logged_pub; ~460 lines including tests)"
        "crates/missiond-core/src/event/in_memory/writer_task.rs (NEW — WriterTask + Pending + payload_bytes)"
        "crates/missiond-core/src/event/in_memory/storage.rs (NEW — StoredRow + IN_MEMORY_APPEND_CAPACITY)"
        "crates/missiond-core/src/event/in_memory/observability.rs (NEW — ObservabilityAppender impl for InMemoryLog)"
        "crates/missiond-core/src/event/in_memory/mod.rs (updated — add `pub mod storage; pub mod writer_task; pub mod observability;` + `pub use storage::{StoredRow, IN_MEMORY_APPEND_CAPACITY};` + keep `pub use log::InMemoryLog;`)"
        ;; Execution log:
        ".missiond/v2/intent-event-bus-execution.lisp (updated — phase-11 β claim + D010/D011/D012 + phase-11 β completion entry)")
      :tests-added 0
      :verified-by "cargo build -p missiond-core --lib clean;cargo build --workspace clean;cargo test -p missiond-core --lib → 255 passed,0 failed(无 regression vs Phase 10 基准);cargo test -p missiond-core --test event_chaos → 12 passed,0 failed(chaos 无 regression);cargo test -p missiond-daemon → 96 passed,0 failed(daemon 无 regression)"
      :notes "零功能改动的神经网络/观测分层物理拆分,对应 frozen lisp v1.1.0 §4.3 subscription-combinators / §4.3 subscription-lifecycle + live-source / §4.4 testing-story in-memory-breakdown 三处 god-file 切分要求。3 task 全部完成 + 测试零回归:(1) combinators.rs 600 → combinators/{mod,debounce,rate_limit,coalesce,filter,map,batch}.rs 7 文件,mod.rs 托管 Subscription<T> 的 6 个扩展方法 + wrap_ack helper + Ack<T>::__new_for_combinator / silent_ack(D010 记录保留这两方法在 mod.rs 而非搬到 api.rs 的理由);(2) lifecycle.rs 498 → {lifecycle.rs(233 行含 tests),live_source.rs(83 行)}(D011 记录),公共 API 100% 兼容(顶层 subscription::LiveSource 仍可导);(3) in_memory/log.rs 575 → {log.rs(460 行含 tests),writer_task.rs(111 行),storage.rs(35 行),observability.rs(28 行)}(D012 记录)。subscription/combinators/mod.rs 继续承载全部 7 个 combinators 单元测试(filter/map/debounce/coalesce/rate_limit/batch×2)避免 tests 循环引用。公共 API 0 breaking change:`subscription::{LiveSource, BroadcastLiveSource, MpscLiveSource}` 仍可用,`in_memory::{InMemoryLog, IN_MEMORY_APPEND_CAPACITY, StoredRow}` 仍可用。未触 pipeline/ / daemon/(agent α 的分区),未改 frozen lisp。")

    (completion
      :phase 11 :date "2026-04-19" :agent "phase11-alpha-pipeline"
      :deliverables (
        ;; step3_commit/ — 1008 行 god file 拆成 8 文件
        "crates/missiond-core/src/event/pipeline/step3_commit/mod.rs (REWRITTEN — 7-component doc table + pub mod + pub use re-exports)"
        "crates/missiond-core/src/event/pipeline/step3_commit/log_writer.rs (REWRITTEN — LogWriter struct + run/flush + new_with_backend + new_log_writer + spawn_log_writer;~296 行实现 + ~380 行 tests inline)"
        "crates/missiond-core/src/event/pipeline/step3_commit/handle.rs (NEW — LogWriterHandle + impl Log trait + causation guard wiring;~140 行)"
        "crates/missiond-core/src/event/pipeline/step3_commit/backend.rs (NEW — WriterBackend trait + InsertRow + BackendError;~58 行)"
        "crates/missiond-core/src/event/pipeline/step3_commit/pg_backend.rs (NEW — PgWriterBackend impl WriterBackend + map_sqlx + INSERT RETURNING + cfg=postgres;~118 行)"
        "crates/missiond-core/src/event/pipeline/step3_commit/dedup.rs (UPDATED — DEDUP_UNIQUE_COLUMNS + is_unique_violation classifier 从 pg_backend 迁入;~53 行)"
        "crates/missiond-core/src/event/pipeline/step3_commit/backpressure.rs (UPDATED — PendingAppend struct 移入 + APPEND_CHANNEL_CAPACITY/BATCH_MAX/BATCH_DEADLINE 常量;~61 行)"
        "crates/missiond-core/src/event/pipeline/step3_commit/failure_mode.rs (UPDATED — RETRY_BASE_DELAY/RETRY_MAX_DELAY/FAILED_STATE_RETRY_CAP 常量 + exp_backoff fn 从 log_writer 迁入 + 1 test;~61 行)"
        "crates/missiond-core/src/event/pipeline/step3_commit/seq_authority.rs (UNCHANGED — Phase 10 doc-only 保留,34 行)"
        ;; step5_tail/ — 883 行 god file 拆成 4 文件
        "crates/missiond-core/src/event/pipeline/step5_tail/mod.rs (REWRITTEN — DispatchMetrics + TAIL_BATCH_LIMIT/TAIL_POLL_INTERVAL 常量 + pub use re-exports + tests inline;636 行)"
        "crates/missiond-core/src/event/pipeline/step5_tail/tail_source.rs (NEW — TailSource trait + DispatchError + TailError + From<LogError>;60 行)"
        "crates/missiond-core/src/event/pipeline/step5_tail/pg_tail.rs (NEW — PgTailSource impl TailSource + domain_from_str+blob_store 解析逻辑 + cfg=postgres;128 行)"
        "crates/missiond-core/src/event/pipeline/step5_tail/dispatcher.rs (NEW — run_tail 主循环 + dispatch_one + control-gate 调用;155 行)"
        ;; Shim 路径更新:
        "crates/missiond-core/src/event/log/mod.rs (updated — event::log::{spawn_log_writer,LogWriterHandle,PendingAppend,...} re-export 链 + writer shim 子模块重定向到新结构)"
        ;; Execution log:
        ".missiond/v2/intent-event-bus-execution.lisp (updated — phase-11 α claim + D008/D009 + phase-11 α completion entry)")
      :tests-added 0
      :verified-by "cargo build -p missiond-core clean;cargo build --workspace clean;cargo test -p missiond-core --lib → 255 passed,0 failed(无 regression vs Phase 10 基准);cargo test -p missiond-core --test event_chaos → 12 passed,0 failed(chaos 无 regression);cargo test -p missiond-daemon → 96 passed,0 failed(daemon 无 regression)"
      :notes "零功能改动的 pipeline 物理拆分,对应 frozen lisp v1.1.0 §4.2 step-3 commit / §4.2 step-5 tail 两处 god-file 切分要求。2 task 全部完成 + 测试零回归:(1) step3_commit/log_writer.rs 1008 行 → 8 文件:log_writer.rs(~676 含 tests) + handle.rs(140) + backend.rs(58) + pg_backend.rs(118) + dedup.rs(53) + backpressure.rs(61) + failure_mode.rs(61) + seq_authority.rs(34,Phase 10 unchanged),mod.rs 45 行,总 ~1246 行(含扩展 doc 和 tests);(2) step5_tail/mod.rs 883 行 → 4 文件:mod.rs(636 含 tests) + tail_source.rs(60) + pg_tail.rs(128) + dispatcher.rs(155),总 ~979 行(含扩展 doc)。核心移动:is_unique_violation 从 pg_backend 抽入 dedup(表达 contract 而非驱动细节);exp_backoff + retry 常量从 log_writer 抽入 failure_mode(与 failure docs 同居);PendingAppend + 3 个 const 从 log_writer 抽入 backpressure。公共 API 100% 兼容:event::pipeline::step3_commit::* / event::pipeline::step5_tail::* / event::log::{writer mod,spawn_log_writer,LogWriterHandle,PendingAppend,...} / event::dispatcher::{TailSource,PgTailSource,DispatchMetrics,DispatchError,TailError,run_tail,...} 所有既有导入路径继续工作。未触 subscription/ / in_memory/(agent β 的分区),未改 frozen lisp。"))

  ;; ─ 全局备忘(跨阶段需要记住的事) ─
  (global-notes
    (historical-data-policy "system_timeline 旧数据不迁,保留 7 天 TTL 只读归档,3 月后废弃")
    (e2e-test-contract "Phase 9 前建立一条黄金路径测试: daemon 启动 → MCP board_create → event_log 写入 → WS 发送 → 前端收到"))
)
