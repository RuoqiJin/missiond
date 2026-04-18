;; MissionD v2 — Pillar: event-bus (FROZEN DESIGN)
;; Split from v2/intent.lisp for parallel loading
;; Parent: v2/intent.lisp
;;
;; Status: 架构基线已冻结 — 所有决策不可动摇,代码按此落地
;; 前身: intent-pillar-event-workers.lisp (MPSC facade + dual broadcast + sweeper)
;; 核心转变: Log-as-Bus — 追加式日志即总线,tail-and-pull 模式分离 live 与 catch-up

  (pillar event-bus
    (purpose "进程内神经网络 — 追加式事件日志 + 类型化 topic 路由 + 游标式订阅")

    ;; ═════════════════════════════════════════════════
    ;; 架构基线一句话(所有决策的不可动摇契约)
    ;; ═════════════════════════════════════════════════
    (decided-options
      :seq                "DB BIGSERIAL + INSERT RETURNING — 单写入点,批量摊成本"
      :topic              "一域一 Topic (12),按 Rust 类型订阅;variant 热点可后期提专属 topic"
      :delivery           "at-least-once — 消费者必须幂等(契约级要求,非类型强制)"
      :cursor             "subscription-name 为 PK,双阈值 flush (每 batch ack + 最长 1s)"
      :new-subscriber     "default latest;opt-in earliest / Seq(N)"
      :pause              "default drop + live-resume (跳到 head);opt-in keep_cursor 冻结+限速 replay"
      :big-payload        "<=8KB 直接进 Log;>8KB 走 blob_storage 表,Log 只存 PayloadRef"
      :crash-recovery     "Producer: append 未 ack 视为未确认,凭 dedupe_key 重试;Dispatcher: live-only,不替人 catch-up;Consumer: tail-and-pull 自恢复"
      :global-replay      "NEVER — dispatcher 不扫全局最小 cursor,O(1) state"
      :dedup-purpose      "支撑 producer 重试,非业务去重"
      :log-ttl            "event_log 默认 30 天 (恢复基座);ephemeral 3 天或不入表"
      :causation-limit    "每事件 causation_depth ≤ 10,超限 CausalLoop 错误"
      :in-memory-bus      "必须与生产同语义 — 单 writer 分 seq + append-ack + seq-ordered replay")

    (design-philosophy
      :principle-1 "一个事件概念 — 不分内外层,seq/trace/ts 是事件固有元数据"
      :principle-2 "一条进入路径 — 所有生产者只调 log.append(),无旁路通道"
      :principle-3 "Log 即总线 — 追加式日志是唯一真理源"
      :principle-4 "Live 与 catch-up 分离 — Dispatcher 只做 live fan-out,Consumer 自己 pull 补历史"
      :principle-5 "Topic 来自类型 — 订阅按 Rust 类型,match 零成本分支"
      :principle-6 "控制面在路由层 — 暂停/限流在 Dispatcher 单点,消费者零感知"
      :principle-7 "显式优于聪明 — pause 默认 drop,catch-up 显式声明,无隐藏语义")

    (scope
      :in  "事件类型定义 / 持久化日志 (含大 payload Claim-Check) / 类型路由 / 订阅 API / 控制闸门"
      :out "业务消费(consumer 的去抖/业务逻辑归消费者自身或 worker pillar)")

    ;; ═════════════════════════════════════════════════
    ;; 4.1 · 入点 (Ingress) — 唯一入口
    ;; ═════════════════════════════════════════════════
    (section ingress
      (desc "全系统唯一入口 — log.append();无 bypass 无 facade 无两套心智")

      (component append-api
        (desc "生产者调用的唯一门面;append 成功即事件已定序并持久化")
        :api "log.append(event: impl DomainEvent, opts: AppendOpts) -> Result<AppendAck, AppendError>"

        (struct AppendOpts
          (field ephemeral       :type "bool"              :default "false" :desc "true 跳过 DB 持久化")
          (field dedupe-key      :type "Option<Uuid>"      :desc "生产者重试保护 — 同 key 二次 append 返回 AlreadyExists(existing_seq)")
          (field after           :type "Option<Seq>"       :desc "可选因果依赖 — 声明此事件必须在 seq 之后定序")
          (field causation-depth :type "u8"                :default "0"     :desc "继承触发事件的 depth+1,>10 抛 CausalLoop")
          (field span            :type "SpanContext"       :desc "trace_id / span_id / parent_span_id"))

        (enum AppendAck
          (Committed     "seq: Seq, durable: true  — 正常提交")
          (Volatile      "seq: Seq, durable: false — ephemeral 路径,仅进 in-memory fan-out")
          (AlreadyExists "seq: Seq — dedupe_key 命中,无副作用"))

        (enum AppendError
          (Backpressure   "append channel 满,生产者自决重试/丢弃")
          (CausalLoop     "causation_depth > 10,拒绝入库")
          (LogUnavailable "DB 不可达;恢复后重试")
          (SchemaMismatch "event 类型未注册到 topic registry"))

        :invariant-1 "生产者不直接接触 broadcast / MPSC / DB,只调 append()"
        :invariant-2 "append() 返回 Ok(Committed) ⟺ 事件已持久化 + seq 已分配"
        :invariant-3 "大 payload 通过 Arc<T> 传入,内部序列化时触发 Claim-Check")

      (dead-bypass
        (desc "原 4 条旁路 MPSC 全部废除,统一走 log.append()")
        (embedding-tx     "→ EmbeddingEvent::Requested")
        (ast-sync-tx      "→ AstSyncEvent::Requested")
        (incident-tx      "→ IncidentEvent::Reported")
        (cursor-ack-tx    "→ 不作为事件;光标追踪由 conversation-logger worker 内部处理,不占用总线")))

    ;; ═════════════════════════════════════════════════
    ;; 4.2 · 核心 (Core) — 三层架构
    ;; ═════════════════════════════════════════════════
    (section core
      (desc "schema / storage / routing 三层架构 — 单向依赖,无反向耦合")

      (data-flow
        :entry      "← append() from 4.1 ingress"
        :schema     "event-types      — 定义类型(编译期),routing 和 storage 都靠这里"
        :storage    "event-log        — 持久化 + 分配 seq + 大 payload Claim-Check"
        :routing    "topic-dispatcher — tail log → 按 domain 派发 → 控制闸 → live fan-out"
        :exit-live  "→ Topic<T> broadcast 给 4.3 live 订阅者"
        :exit-pull  "→ Consumer 直接 SELECT event_log 做 catch-up (4.3 phase-1)")

      ;; ────────────────────────────────────────────────
      ;; 4.2.a · schema 层
      ;; ────────────────────────────────────────────────
      (component event-types
        (desc "trait + 12 个 domain enum,替代旧 DaemonEvent god-enum")
        :target "crates/missiond-core/src/event/"

        (trait DomainEvent
          :super "Send + Sync + Serialize + DeserializeOwned + 'static"
          :method "fn domain() -> Domain  // 静态域,编译期已知"
          :method "fn kind(&self) -> &'static str  // variant 名用于 metrics"
          :method "fn payload_size_hint(&self) -> usize  // 用于 Claim-Check 阈值判断")

        (domain-enums 12
          (SlotEvent          "BecameIdle / StateChanged / TaskDispatched / Stuck")
          (BoardEvent         "TaskCreated / StatusChanged / NoteAdded / Claimed / Deleted / Updated")
          (TaskEvent          "Created / Completed")
          (QuestionEvent      "Created / Resolved / DecisionResolved")
          (LlmEvent           "RequestStarted / RequestCompleted / ToolActivity (带 Provider 标签)")
          (WorkerEvent        "LlmCall / Translation* / Narration* / Briefing*")
          (MemoryEvent        "PhaseChanged")
          (MessageEvent       "Logged / ImageInserted")
          (SessionEvent       "Completed / JarvisTaskCompleted")
          (SystemEvent        "ConfigChanged / ToolCompleted / InsightGenerated")
          (ObservabilityEvent "HealthSnapshot / BusMetric / SlowConsumer — 强制 ephemeral")
          (IncidentEvent      "Reported / Resolved"))

        :topic-discovery "bus.topics() -> [Domain; 12] — 静态编译期契约,无字符串通配"
        :escape-hatch "若某 variant 后期变热点,可单独提升为专属 sub-topic;12 域是起点不是终点")

      ;; ────────────────────────────────────────────────
      ;; 4.2.b · storage 层
      ;; ────────────────────────────────────────────────
      (component event-log
        (desc "追加式事件日志 — 系统的唯一真理源 + Consumer catch-up 的 pull 目标")
        :target "crates/missiond-core/src/event/log.rs"

        (schema
          :table "event_log"
          :columns
            ("seq             BIGSERIAL PRIMARY KEY  -- tail 主索引兼 seq 单调权威"
             "domain          TEXT NOT NULL"
             "kind            TEXT NOT NULL"
             "payload_inline  JSONB           -- NULL 时参见 payload_ref"
             "payload_ref     TEXT            -- blob_storage 键,>8KB 时使用"
             "producer_id     TEXT NOT NULL"
             "dedupe_key      UUID            -- 可空;(producer_id, dedupe_key) 唯一索引"
             "causation_depth SMALLINT NOT NULL DEFAULT 0"
             "trace_id        UUID"
             "span_id         UUID"
             "parent_span_id  UUID"
             "ts              TIMESTAMPTZ NOT NULL DEFAULT now()"
             "ephemeral       BOOLEAN NOT NULL DEFAULT false  -- 仅供 TTL 分流,已入表者仍有效")
          :secondary-indexes
            ("INDEX (domain, seq)                       -- 按域 catch-up 扫描"
             "UNIQUE (producer_id, dedupe_key) WHERE dedupe_key IS NOT NULL  -- producer 重试保护"
             "INDEX (ts) WHERE ephemeral = true          -- 快速 TTL 清理"))

        (writer-semantics
          :pattern  "唯一 LogWriter task 消费 append channel"
          :batching "首条到达后 drain ≤100 条 / 10ms 到期,取先到者"
          :return   "append() 在 batch 落盘后返回 Ok(Committed);失败返回 Err(生产者自决)"
          :invariant "append() Ok ⟺ DB committed,不存在 in-flight 语义")

        (seq-authority
          :source         "DB BIGSERIAL — 全局严格单调,DB 单写入点保证"
          :crash-recovery "DB 保存 max(seq),无应用层对账"
          :invariant      "seq 只增不减;已分配 seq 终生不变")

        (dedup-semantics
          :purpose            "Producer 重试保护,非业务去重"
          :key                "(producer_id, dedupe_key) UNIQUE INDEX WHERE dedupe_key IS NOT NULL"
          :collision-behavior "二次 append 相同 key → 返回 Ok(AlreadyExists(existing_seq)),无副作用"
          :producer-contract  "生产者超时/崩溃重试时必须带同一 dedupe_key")

        (persistence-policy
          :default    "持久化 — 所有 append 默认写 DB"
          :ephemeral  "AppendOpts.ephemeral=true 跳过 DB,只走 in-memory fan-out"
          :use-case   "ObservabilityEvent 默认 ephemeral(高频心跳/快照)"
          :rationale  "ephemeral 是调用方决策,不污染事件类型定义")

        (backpressure
          :channel  "append channel 有界 (默认 4096)"
          :overflow "满则 append() 返回 Err(Backpressure),生产者决定重试/丢弃/panic"
          :rationale "可见失败 > 静默吞 > 无界内存膨胀")

        (retention
          :default-ttl         "30 天 — event_log 是恢复基座,不能比 consumer 离线周期短"
          :per-domain-override "可 per-domain 配置,如 ObservabilityEvent = 3 天"
          :ephemeral-ttl       "3 天(若入表)"
          :cleanup-strategy    "每日清理 job,一次 DELETE WHERE age > domain_ttl"
          :old-system_timeline "沿用 7 天 TTL;event_log 上线后转只读归档,3 月后废弃")

        ;; ─ Claim-Check: 超大 payload 的存储扩展(原 4.4) ─
        (claim-check
          (desc "大 payload 不进 Log 主表,只留 durable pointer")
          :threshold-inline "payload_size_hint() <= 8KB → 直接 JSONB 入 Log"
          :threshold-ref    "payload_size_hint() >  8KB → 数据入 blob_storage 表,Log 只存 PayloadRef"

          (struct PayloadRef
            (field backend  :type "BlobBackend" :desc "blob-table / local-file")
            (field uri      :type "String"      :desc "backend 内的定位键")
            (field size     :type "u64"         :desc "原始 payload 字节数")
            (field checksum :type "Sha256"      :desc "完整性校验"))

          (backends
            (blob-table
              :storage   "PostgreSQL 表 blob_storage(id UUID PK, data BYTEA, size INT, created_at, ttl)"
              :default   "MissionD 单机 local-first 默认后端"
              :rationale "同 DB 一致性 / 备份简单 / 不引外依赖")
            (local-file
              :storage   ".missiond/blobs/<hash-prefix>/<uuid> 文件系统"
              :use-case  "payload 极大(>1MB)或频繁读但不共享"
              :rationale "避免 PG BYTEA 对单表的负担"))

          (forbidden-backends
            :in-memory-handle "禁止 — 重启即废,不能作为 durable pointer"
            :s3-out-of-scope  "missiond 单机优先,暂不引入对象存储")

          (retrieval-api
            :method  "blob_store.fetch(payload_ref) -> Result<Bytes>"
            :caching "进程内 LRU 可选;订阅者不需自管"))

        :replaces "原 timeline_mpsc_tx + run_timeline_writer + system_timeline 三合一")

      ;; ────────────────────────────────────────────────
      ;; 4.2.c · routing 层
      ;; ────────────────────────────────────────────────
      (component topic-dispatcher
        (desc "Log tail → 按 domain 分派到 Topic<T> → live 订阅者 fan-out")
        :target "crates/missiond-core/src/event/dispatcher.rs"

        (scope-invariant
          :does      "live fan-out 最新提交的事件给当前在线订阅者"
          :does-not  "不替离线 consumer 扫库;不做全局最小 cursor replay;不维护 per-subscription 状态"
          :state     "O(1) — 只需一个 tail cursor(上次派发的 seq)"
          :rationale "离线 consumer 一周不上线 → Dispatcher 零负担;Consumer 上线后自己 pull 补追")

        (tail-mechanism
          :source        "PostgreSQL LISTEN/NOTIFY 或长轮询 SELECT WHERE seq > last_dispatched"
          :ordering      "严格按 seq 升序派发;同 batch 内保持 INSERT 顺序"
          :missed-events "Dispatcher 崩溃重启时从 persisted last_dispatched_seq 继续;不替订阅者补发,订阅者靠自己 cursor 自救")

        (topic-registry
          :type             "static HashMap<Domain, Topic<dyn Any>>"
          :fanout-transport "tokio::broadcast::channel<Arc<Event>> per Topic"
          :buffer-size      "默认每 topic 1024;慢订阅者溢出触发 SlowConsumer incident")

        (control-gate
          :desc      "Dispatcher 在派发前检查 ControlManager,暂停域不进 topic"
          :input     "ControlManager.is_domain_paused(domain) — watch::Receiver<ControlTree>"
          :action    "paused=true 时跳过该 domain 的所有投递(事件仍在 Log 中)"
          :stateless "Dispatcher 不记录 per-subscription pause 时刻;pause/resume 语义归订阅侧决定(见 4.3 PauseBehavior)"
          :rationale "Dispatcher O(1) 不变;resume 行为差异化由 consumer opt-in,符合'控制面单点+数据面多态'")

        :replaces "原 timeline_tx broadcast + event_router 8 consumers + sweeper 三位一体"))

    ;; ═════════════════════════════════════════════════
    ;; 4.3 · 出点 (Egress) — 订阅 API
    ;; ═════════════════════════════════════════════════
    (section egress
      (desc "消费者声明订阅 → tail-and-pull 双阶段接入 → combinators 声明式处理")

      (component subscription-api
        (desc "类型安全的订阅入口")
        :primary-api "bus.subscribe::<T: DomainEvent>(name: &str, opts: SubscriptionOpts) -> Subscription<T>"

        (struct SubscriptionOpts
          (field start-from     :type "StartFrom"     :default "Latest"                           :desc "Latest / Earliest / Seq(n)")
          (field batch-size     :type "usize"         :default "100"                              :desc "每批最大事件数;per-subscription 可调")
          (field failure-policy :type "FailurePolicy" :default "Retry { max: 3, backoff: exp }"   :desc "处理失败的回退")
          (field pause-behavior :type "PauseBehavior" :default "DropAndLiveResume"                :desc "pause 时的累积语义")
          (field cursor-flush   :type "CursorFlush"   :default "BatchOr1s"                        :desc "Cursor 持久化频率"))

        (enum FailurePolicy
          (Retry        "max: u8, backoff: exp / fixed — 就地重试,超限转策略")
          (SkipToDLQ    "失败事件入 dead_letter_queue 表,cursor 推进,consumer 不被阻塞")
          (Halt         "失败即停止 consumer,等人工介入 — 适合关键路径"))

        (enum PauseBehavior
          (DropAndLiveResume    "默认:paused 期间不投递,resume 时 cursor 跳到 head")
          (FreezeAndCatchUp     "opt-in:paused 期间 cursor 冻结不前推;resume 时 subscription 触发 pull catch-up(batch_size 节流)"))

        (enum CursorFlush
          (PerEvent          "每条 ack 即 flush — 最小重复量,吞吐最差")
          (BatchOr1s         "默认:每 batch ack + 最长 1s 强制 flush — 平衡")
          (Periodic Duration "自定义周期 flush — 最松")))

      (component subscription-lifecycle
        (desc "订阅者上线的两阶段模型 — tail-and-pull")

        (phase-1-bootstrap
          :action    "从持久 cursor 读 last_acked_seq → pull Log: SELECT WHERE seq > last_acked ORDER BY seq LIMIT batch_size"
          :loop      "处理 batch → ack → 继续 pull,直到 pull 返回空"
          :invariant "完成前不订阅 live stream,避免重复消费")

        (phase-2-live
          :action        "切 Dispatcher 的 Topic broadcast Receiver,进入 live 模式"
          :invariant     "live 模式下事件按 seq 严格单调到达(同 Dispatcher 派发顺序)"
          :on-disconnect "broadcast Lagged → 记 SlowConsumer incident → 切回 phase-1 重 pull"))

      (component cursor-store
        (desc "订阅 cursor 持久化")
        :table "event_subscriptions"
        :schema
          ("subscription_name  TEXT PRIMARY KEY  -- ⚠ 同一 consumer 可有多订阅,PK 是订阅名不是 consumer 名"
           "consumer_name      TEXT NOT NULL      -- 仅信息字段,便于运维归类"
           "domain             TEXT NOT NULL"
           "last_acked_seq     BIGINT NOT NULL"
           "last_seen_at       TIMESTAMPTZ"
           "failure_policy     JSONB"
           "created_at         TIMESTAMPTZ")

        (flush-policy
          :default   "BatchOr1s — 每 batch ack 后写,或 1s 未写强制 flush"
          :guarantee "崩溃最多**重复**处理 min(batch_size, 1 秒内事件数) 条;依赖 consumer 幂等消化")

        (orphan-cleanup
          :policy    "last_seen_at 超过 30 天未更新 → 后台 job 归档该 cursor + 发 IncidentEvent::StaleSubscription 通知运维"
          :re-subscribe-behavior "cursor 被归档后再订阅 = 新订阅者,按 default Latest 起;要恢复需 ops 跑重放脚本"))

      (component delivery-semantics
        :guarantee           "at-least-once — 消费者必须幂等"
        :rationale           "cursor update 与业务副作用不在同一事务,崩溃可能重放最后 batch"
        :idempotency-helper  "SeqDedupSet(Arc<Mutex<BTreeSet<Seq>>>) — consumer 想要 seq 级幂等但业务无天然键时可用"
        :NOT-trait-enforced  "幂等是契约级要求,不走类型强制;强制 idempotency_key 会让天然幂等 consumer 编造伪 key"
        :design-contract     "每个 consumer 设计评审时必须回答:同一事件重跑是否安全?如何保证?答案写进文档")

      (subscription-combinators
        (desc "声明式订阅组合子,替代每个 consumer 手写样板")
        (debounce   :api "sub.debounce(Duration::from_millis(500))"
                    :semantics "固定 deadline 窗口,到期只触发一次,不滑动")
        (rate-limit :api "sub.rate_limit(max_per_sec)")
        (coalesce   :api "sub.coalesce(|prev, new| ...)"
                    :semantics "合并语义相同的连续事件(如多条 StateChanged 只保留最终态)")
        (filter     :api "sub.filter(|e| e.is_some_kind())")
        (map        :api "sub.map(|e| transform(e))")
        (batch      :api "sub.batch(max: 50, window: 500ms) — 聚合成 Vec<E> 再投递")

        :rationale "旧 event_router 8 consumers 各自手写去抖;combinators 让模式声明化,实现一处"))

    ;; ═════════════════════════════════════════════════
    ;; 4.4 · 横切面 (Cross-cutting)
    ;; ═════════════════════════════════════════════════
    (section cross-cutting
      (desc "贯穿 4.1/4.2/4.3 所有层的系统属性 — 每条都跨相,不属于任何单一相")

      (component causation-loop-guard
        (desc "防止 consumer 处理事件触发自己订阅的事件 → 无限循环")
        :mechanism "每 append 的 causation_depth = 触发事件.causation_depth + 1"
        :limit     "MAX_DEPTH = 10"
        :on-exceed "AppendError::CausalLoop,事件不入库 + 发 IncidentEvent"
        :rationale "真实业务链很少超 5 层;10 给合理余量同时拦住 bug")

      (observability
        :bus-metrics
          ("append_rate / reject_rate"
           "dispatch_lag (tail cursor 相对 max seq)"
           "per-topic queue depth + slow consumer count"
           "per-subscription lag (head seq - last_acked_seq)"
           "ephemeral vs persistent 比例"
           "control-gate 丢弃计数")
        :self-emission   "bus 自观测 → ObservabilityEvent 必须 ephemeral=true,否则形成观测自循环"
        :isolation-rule  "ObservabilityEvent 的 handler 产生新 append 时,新事件也必须 ephemeral=true")

      (fault-isolation
        :producer   "append 失败抛 Err,不拖垮调用方"
        :log-writer "batch INSERT 失败 → exp backoff 重试 → 超限转 IncidentEvent + 拒新 append + DB unavailable 状态"
        :dispatcher "panic 由 supervisor 重启对应 topic task;Dispatcher 全体崩从 last_dispatched 继续,不替人补发"
        :subscriber "panic 断开该订阅,其他订阅者无感知;自动重订阅由消费者自行决策")

      (testing-story
        :in-memory-bus "必须与生产同语义 — 单 writer 分 seq + append-ack + seq-ordered replay,不是裸 AtomicU64"
        :determinism   "可注入固定 seq / ts / trace_id,单测可重现"
        :replay-debug  "录制 Log 片段 → 换机 replay → 复现 bug")

      (chaos-test-matrix
        (desc "必须覆盖的故障模式 — Week 2 chaos testing 基线")
        (log-writer-timeout     "DB INSERT > 超时阈值 → publish Err,生产者可见")
        (log-writer-panic       "重启后从最后提交 seq 继续,in-flight batch 视为丢失")
        (dispatcher-panic       "重启后从 last_dispatched 继续,不替订阅者补发")
        (subscriber-panic       "仅断本订阅,其他零感知")
        (slow-subscriber-lag    "lag > 阈值 → SlowConsumer incident + drop 策略")
        (db-disconnect          "LogWriter 进 failed,拒新 append,恢复后续流")
        (causation-loop-trigger "构造 depth > 10 链 → CausalLoop + incident")
        (dedup-key-retry        "同 dedupe_key 二次 append → AlreadyExists,无副作用")
        (cursor-orphan          "模拟 30 天未更新 cursor → 被归档 + 通知 + 新订阅走 Latest")))

    ;; ═════════════════════════════════════════════════
    ;; 4.5 · 未来空位(声明不做,防止作用域蔓延)
    ;; ═════════════════════════════════════════════════
    (section deferred
      (desc "声明范围之外的事情,避免设计蔓延")

      (not-now
        (distributed-bus      "单进程设计;未来如需跨进程,Log 可换 NATS/Redpanda,API 层保持")
        (exactly-once         "at-least-once + 幂等已够;不做两阶段提交")
        (projection-framework "Dispatcher + cursor 已够搭 projection;通用框架暂不做")
        (schema-registry      "Rust 类型 + Forge 即 schema;独立 registry 暂不引入")
        (producer-token-auth  "当前文档问题非技术问题;未来可在 append-api 加 ProducerToken")
        (variant-level-topic  "当前 12 域 topic 足够;某 variant 真成热点再提升"))

      (revisit-triggers
        (desc "触发重新评估 deferred 的条件")
        "若 Log 单表 >1B 行 → 考虑分片"
        "若某 topic QPS >10k → 考虑 variant-level topic"
        "若多进程 missiond 实例共享状态 → 考虑 distributed-bus"
        "若 exactly-once 成合规要求 → 考虑 outbox pattern")))
