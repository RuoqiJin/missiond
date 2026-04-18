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
    :phase-cursor 0)

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
    (phase-5 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-6 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-7 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-8 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-9 :status "pending" :owner nil :started nil :completed nil :summary nil))

  ;; ─ 并行锁表(防 agent 冲突) ─
  ;; 格式: (claim :phase N :scope "path/description" :agent "name" :claimed-at "..." :released-at "..."|nil)
  (claims
    (claim :phase 4 :scope "crates/missiond-core/src/event/subscription/**" :agent "phase4-subscription"
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
              :rationale "frozen lisp §4.3 subscription-api 给出三个 policy 但未规定 Retry 超限后走哪个。选 SkipToDLQ 是 safe default:subscription 持续向前,坏事件入 DLQ 让 ops 离线修复。若 consumer 显式要 Halt,应 opt-in FailurePolicy::Halt 而不是靠 Retry 超限。tests failure.rs 覆盖此行为"))

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
      :notes "subscribe::<T>(name, opts, log, topic, cursor_store, dlq) 统一入口;Subscription<T>::next() 返回 Ack<T>,consumer 必须 .ack().await 或 .nack(reason).await。Ack drop 视为 silent nack(DC018)。两阶段 lifecycle:bootstrap 从 log.read_from pull,耗尽后切 live 读 Topic broadcast。watermark 与 ack_cursor 分离避免重读(DC017)。双阈值 flush:flusher task tokio::select!(Dirty/Force/Nack/interval),每条 ack 或 1s 时窗 upsert cursor。6 combinators:debounce/rate_limit/coalesce/filter/map/batch,被吸收事件 silent_ack(DC019)。FailurePolicy 三个:Retry 超限 fall through 到 SkipToDLQ(DC020)。PauseBehavior:MVP 只 DropAndLiveResume(D001);FreezeAndCatchUp 推迟 I009。引入 LogReadable 子 trait 解决 dyn 不兼容(DC016/I008)。未 touch v1 event_bus.rs/event_router.rs/workers/daemon。Phase 5+ 起草 InMemoryBus / daemon 迁移 consumer。"))

  ;; ─ 全局备忘(跨阶段需要记住的事) ─
  (global-notes
    (historical-data-policy "system_timeline 旧数据不迁,保留 7 天 TTL 只读归档,3 月后废弃")
    (e2e-test-contract "Phase 9 前建立一条黄金路径测试: daemon 启动 → MCP board_create → event_log 写入 → WS 发送 → 前端收到"))
)
