;; ╔════════════════════════════════════════════════════════════════════╗
;; ║  ARCHITECTURE-UNLOCKED — DIRECT EDITS ALLOWED BY USER              ║
;; ║  已解锁 — 指挥官明确授权: 架构完整性需要时可直接修改本文件        ║
;; ║                                                                    ║
;; ║  规则:                                                             ║
;; ║    1. 若跨 pillar 设计需要 event-bus 变更,直接改。                ║
;; ║    2. 必须标清 implemented / architecture-designed / pending。     ║
;; ║    3. 重大事件契约或治理变化仍写 companion execution log。         ║
;; ║    4. 代码实现留待统一 ClaudeCode 同构阶段。                       ║
;; ╚════════════════════════════════════════════════════════════════════╝
;;
;; MissionD v2 — Pillar: event-bus (FROZEN DESIGN)
;; Split from v2/intent.lisp for parallel loading
;; Parent: v2/intent.lisp
;;
;; Status: 架构基线已冻结 — 所有决策不可动摇,代码按此落地
;; 前身: intent-pillar-event-workers.lisp (MPSC facade + dual broadcast + sweeper)
;; 核心转变: Log-as-Bus — 追加式日志即总线,tail-and-pull 模式分离 live 与 catch-up
;;
;; ⚠ 阅读提示:每个 component / trait / struct / enum 都带 :target,
;;             读 lisp 后可直接定位代码文件,减少 survey 步数

(file-governance
  :lock                "architecture-unlocked"
  :version             "v1.3.4"
  :sealed-at           "2026-04-19"
  :last-revision       "2026-04-25: v1.3.3 → v1.3.4 — code-aligned ExecutionEvent as Domain::Execution and ObservabilityEvent CapabilityUsageSnapshot/CapabilityStaleCandidate; LlmProviderLifecycle remains planned"
  :prior-revisions
    ("v1.3.2 → v1.3.3: added planned ObservabilityEvent capability usage snapshot/candidate markers for tool+flow usage monitor; no code implementation implied"
     "v1.3.1 → v1.3.2: user unlocked memory/event-bus for direct architecture edits; added SessionEvent completion emit contract; no code implementation implied"
     "v1.3.0 → v1.3.1: planning-only extension markers for mission_execution ExecutionEvent and xjp-router provider lifecycle events; no change to implemented 12-domain contract; approved by user"
     "v1.2.0 → v1.3.0: event_log 正式锁定为 timeline SSOT (原 system_timeline 表废弃); event_log 新增 read-ui-projection 访问模式 + §4.2 retention 更新 cutover 状态"
     "v1.1.0 → v1.2.0: §4.6 persistence-layer 新增 (4 表所有权从 memory pillar 划回), D013 deviation"
     "v1.0.0 → v1.1.0: god-file split design (approved), D008-D012 deviations")
  :approved-by         "指挥官 (user)"
  :change-policy       "direct-edit-when-cross-pillar-architecture-requires; record major contract changes in companion log"
  :companion-log       ".missiond/v2/intent-event-bus-execution.lisp"
  :who-can-approve     "human user approved full architecture unlock on 2026-04-25"
  :who-must-ask        "no ask required for architecture design edits; implementation code still follows normal review/code-alignment process"

  (allowed-without-ask
    (typo-fix             "错别字 / 标点修正,execution lisp 留痕即可")
    (path-drift-correct   ":target 路径因代码移动而过时,更新 :target 并在 execution lisp 记 drift")
    (new-target-addition  "代码新增子模块可补 :target,不可改已存在的"))

  (record-required
    (design-decision-change  "任何 decided-options / design-philosophy 字段变动,写 execution log decision")
    (invariant-change        "invariants / 契约 / :stateless / :guarantee 字段变动,写 execution log decision")
    (structural-change       "新增/删除 step / section / component 层级,写 execution log completion")
    (enum-variant-change     "AppendAck / AppendError / FailurePolicy 等枚举的增删改,标明 code-alignment pending or implemented")
    (schema-change           "event_log / event_subscriptions / blob_storage schema,必须标明 migration/code-alignment 状态")
    (governance-change       "治理规则变化,写 execution log decision"))

  (enforcement-layers
    (layer-1-banner      "文件顶部大横幅警告,任何 LLM reader 第一眼可见")
    (layer-2-governance  "此 (file-governance) 块,机器可解析的锁声明")
    (layer-3-fs-permission "chmod 444 OS 级只读(可选,用户可 chmod 644 临时打开)")
    (layer-4-claude-md   "~/.claude/CLAUDE.md 或项目 CLAUDE.md 全局提醒")
    (layer-5-auto-memory "Claude Code 长期记忆,跨 session 生效"))

  :violation-protocol
    "v1.3.2+ 后不再按未授权修改处理架构编辑;若发现未标 implemented/pending 或未写重大变更记录,补 execution log 并修正状态标注")

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
      :log-ttl            "event_log 默认 30 天 (恢复基座);ephemeral 3 天自动清"
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
      :target "crates/missiond-core/src/event/log/mod.rs"
      (desc "全系统唯一入口 — log.append();无 bypass 无 facade 无两套心智")

      (component append-api
        :target "crates/missiond-core/src/event/log/mod.rs"
        :impl-target "crates/missiond-core/src/event/pipeline/step3_commit/handle.rs"
        (desc "生产者调用的唯一门面;append 成功即事件已定序并持久化")
        :api "log.append(event: impl DomainEvent, opts: AppendOpts) -> Result<AppendAck, AppendError>"
        :trait-decl-in   "log/mod.rs — Log trait 定义"
        :trait-impl-in   "step3_commit/handle.rs — LogWriterHandle 实现 Log trait,转发到 step 3 writer task"

        (struct AppendOpts
          :target "crates/missiond-core/src/event/log/mod.rs"
          (field ephemeral       :type "bool"              :default "false" :desc "true 跳过 DB 持久化")
          (field dedupe-key      :type "Option<Uuid>"      :desc "生产者重试保护 — 同 key 二次 append 返回 AlreadyExists(existing_seq)")
          (field after           :type "Option<Seq>"       :desc "可选因果依赖 — 声明此事件必须在 seq 之后定序")
          (field causation-depth :type "u8"                :default "0"     :desc "继承触发事件的 depth+1,>10 抛 CausalLoop")
          (field span            :type "SpanContext"       :desc "trace_id / span_id / parent_span_id"))

        (enum AppendAck
          :target "crates/missiond-core/src/event/log/mod.rs"
          (Committed     "seq: Seq, durable: true  — 正常提交")
          (Volatile      "seq: Seq, durable: false — ephemeral 路径,仅进 in-memory fan-out")
          (AlreadyExists "seq: Seq — dedupe_key 命中,无副作用"))

        (enum AppendError
          :target "crates/missiond-core/src/event/log/mod.rs"
          (Backpressure   "append channel 满,生产者自决重试/丢弃")
          (CausalLoop     "causation_depth > 10,拒绝入库")
          (LogUnavailable "DB 不可达;恢复后重试")
          (SchemaMismatch "event 类型未注册到 topic registry"))

        :invariant-1 "生产者不直接接触 broadcast / MPSC / DB,只调 append()"
        :invariant-2 "append() 返回 Ok(Committed) ⟺ 事件已持久化 + seq 已分配"
        :invariant-3 "大 payload 通过 Arc<T> 传入,内部序列化时触发 Claim-Check")

      (dead-bypass
        (desc "原 MPSC 旁路的归宿(无代码文件 — 概念映射)")
        (incident-tx    "→ IncidentEvent::Reported,走 log.append()")
        (cursor-ack-tx  "→ 不作为事件;光标追踪由 conversation-logger worker 内部 Mutex<HashMap> 共享,不占总线"
                         :runtime-location "crates/missiond-daemon/src/state.rs (conversation_cursor_map)")
        (embedding-tx   "→ 保留为 embedding_worker 的 1:1 内部任务队列,不升级为 DomainEvent(符合 §4.2 prerequisite 的 12 域契约)"
                         :runtime-location "crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs")
        (ast-sync-tx    "→ 保留为 ast_sync_worker 的 1:1 内部任务队列,不升级为 DomainEvent(同上)"
                         :runtime-location "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs")))

    ;; ═════════════════════════════════════════════════
    ;; 4.2 · 核心 (Core) — 7 步事件处理流水线
    ;; ═════════════════════════════════════════════════
    (section core
      :target "crates/missiond-core/src/event/pipeline/"
      (desc "事件从 append 到 fan-out 的 7 步流水线 — 模块从上到下对应执行顺序,可观测")

      ;; ─ 流水线概览 ─
      (execution-flow
        :entry "← append(event, opts) from 4.1 ingress"
        (step-1 guard    "因果校验 + 类型解析")
        (step-2 decide   "claim-check 阈值 + ephemeral 决策")
        (step-3 commit   "批处理 + DB INSERT + seq 分配 + dedup 处理")
        (step-4 ack      "回 producer (Committed / Volatile / AlreadyExists / Err)")
        ;; ── 上方 sync,下方 async 分界线 ──
        (step-5 tail     "Dispatcher 独立 task 读 log,不阻塞 step 1-4")
        (step-6 gate     "control-plane 暂停域过滤")
        (step-7 fanout   "Topic<T> broadcast 扇出到订阅者")
        :exit "→ 4.3 egress subscribers")

      (invariants
        :sync-chain    "step 1-4 在 append() 调用栈内同步;Ok(Committed) 返回 ⟺ commit 落盘"
        :async-chain   "step 5-7 异步,与 append 解耦;Dispatcher 崩溃不阻塞 append"
        :idempotency   "step 3 dedup 保证 producer 重试语义")

      ;; ═══════════════════════════════════════════════
      ;; 前置 · 共享基础(非流水线步骤,1-7 步都依赖)
      ;; ═══════════════════════════════════════════════
      (prerequisite event-types
        :target "crates/missiond-core/src/event/"
        (desc "类型体系 — trait + 13 domain enum,是流水线所有步骤的类型基础")

        (trait DomainEvent
          :target "crates/missiond-core/src/event/event_trait.rs"
          :super "Send + Sync + Serialize + DeserializeOwned + 'static"
          :method "fn domain() -> Domain  // 静态域,编译期已知"
          :method "fn kind(&self) -> &'static str  // variant 名用于 metrics"
          :method "fn payload_size_hint(&self) -> usize  // step 2 claim-check 阈值判断依据")

        (enum Domain
          :target "crates/missiond-core/src/event/domain.rs"
          :values "Slot / Board / Task / Question / Llm / Worker / Memory / Message / Session / System / Observability / Incident")

        (domain-enums 12
          :target "crates/missiond-core/src/event/events/"
          (SlotEvent          :target "crates/missiond-core/src/event/events/slot.rs"
                              :variants "BecameIdle / StateChanged / TaskDispatched / Stuck")
          (BoardEvent         :target "crates/missiond-core/src/event/events/board.rs"
                              :variants "TaskCreated / StatusChanged / NoteAdded / Claimed / Deleted / Updated")
          (TaskEvent          :target "crates/missiond-core/src/event/events/task.rs"
                              :variants "Created / Completed / CascadeTriggered / CascadeCompleted")
          (QuestionEvent      :target "crates/missiond-core/src/event/events/question.rs"
                              :variants "Created / Resolved / DecisionResolved")
          (LlmEvent           :target "crates/missiond-core/src/event/events/llm.rs"
                              :variants "RequestStarted / RequestCompleted / ToolActivity (含 Provider 标签)")
          (WorkerEvent        :target "crates/missiond-core/src/event/events/worker.rs"
                              :variants "LlmCall / Translation* / Narration* / Briefing*")
          (MemoryEvent        :target "crates/missiond-core/src/event/events/memory.rs"
                              :variants "PhaseChanged / DeepAnalysisCompleted / KBBatchMutated / TurnExtracted / IntentAnalyzed")
          (MessageEvent       :target "crates/missiond-core/src/event/events/message.rs"
                              :variants "Logged / ImageInserted")
          (SessionEvent       :target "crates/missiond-core/src/event/events/session.rs"
                              :variants "Completed / JarvisTaskCompleted / SessionOrganized")
          (SystemEvent        :target "crates/missiond-core/src/event/events/system.rs"
                              :variants "ConfigChanged / ToolCompleted / InsightGenerated / JarvisProactivePush / ContextualCommitDetected")
          (ObservabilityEvent :target "crates/missiond-core/src/event/events/observability.rs"
                              :variants "HealthSnapshot / BusMetric / SlowConsumer / RetentionReport — 强制 ephemeral")
          (IncidentEvent      :target "crates/missiond-core/src/event/events/incident.rs"
                              :variants "Reported / Resolved / StaleSubscription"))

        (session-completion-contract
          :status "architecture-designed; verify/code-alignment pending"
          :domain "Session"
          :primary-variant "SessionEvent::Completed"
          :related-worker-variant "WorkerEvent::Narration* may still produce NarrationSessionCompleted-like semantics; should bridge to SessionEvent::Completed when session-level completion is known"
          :producers
            ["pty_event_worker when semantic parser observes stable idle after user task completion"
             "conversation organizer when a persisted conversation/session is closed or organized"
             "flow-engine-v2 when a flow-bound session reaches terminal completed/failed state"
             "manual/maintenance path may synthesize Completed for backfilled history with explicit source=backfill"]
          :payload-fields
            ["session_id" "project_id?" "slot_id?" "conversation_id?" "completion_source" "ended_at" "last_message_seq?" "summary_ref?" "dedupe_key"]
          :dedupe-key "session_id + completion_source + ended_at_window; producer retries must reuse key"
          :consumers
            ["F8-retrospective-to-memory :: retro_worker"
             "F-strategy-analysis :: strategy_worker"
             "experience_harvester / session_reflection subscribers"
             "timeline/ws projection for user-visible session close"]
          :idempotency "Consumers key by session_id + event seq; duplicate Completed must not create duplicate retrospectives/deep_analysis rows"
          :cross-ref ["flow :: F-session-completion-event-chain" "flow :: F8-retrospective-to-memory" "flow :: F-strategy-analysis" "worker :: pty_event_worker / conversation organizer"])

        :topic-discovery "bus.topics() -> [Domain; 13] — 静态编译期契约,无字符串通配"
        :escape-hatch "若某 variant 后期变热点,可单独提升为专属 sub-topic;13 域是当前状态,域集合允许按 planned extension 晋升"

        (event-extensions
          :status "ExecutionEvent + CapabilityUsageObservability code-aligned; LlmProviderLifecycle still planned"
          (ExecutionEvent
            :status "implemented"
            :trigger "mission_execution mutating actions emit live projection events"
            :domain "Execution"
            :variants "Opened / Claimed / Heartbeat / Released / DeviationRecorded / DecisionRecorded / IssueRecorded / Completed / Audited / Repaired / StaleClaim"
            :rationale "agent-execution-coordination is operational state that needs live status/audit projection; durable truth remains companion Lisp log"
            :cross-ref ["memory :: helper agent-execution-coordination" "worker :: agent-execution-manager-interface" "flow :: F-execution-log-governance"])
          (LlmProviderLifecycle
            :status "planned"
            :trigger "xjp-router client bootstrap/provider health becomes runtime-visible"
            :candidate-placement "extend existing LlmEvent, not new domain"
            :candidate-variants "ProviderConfigured / ProviderUnavailable / ProviderRecovered"
            :rationale "provider lifecycle belongs to Llm domain; xjp-router embedding failures should be observable without adding an embedding domain")
          (CapabilityUsageObservability
            :status "implemented"
            :trigger "mission_capability_usage snapshot/report/candidates after read-model computation succeeds"
            :placement "extend existing ObservabilityEvent, not new domain"
            :variants "CapabilityUsageSnapshot / CapabilityStaleCandidate"
            :payload-shape "window, scope, generated_at, counts_by_capability, stale_candidates, merge_candidates, protected_ids, report_ref"
            :ephemeral-default true
            :rationale "tool/flow usage monitor is observability over existing capabilities, not a new business domain; durable evidence remains in memory/tool audit, while bus event is for live projection and review notification"
            :cross-ref ["memory :: capability-usage-read-model" "flow :: F-capability-usage-monitoring" "intent-layer :: capability-evolution-governance" "tools :: mission_capability_usage"])))

      (prerequisite event-log-schema
        :target "crates/missiond-core/migrations/20260419000000_event_log.sql"
        (desc "真理源存储 — step 3 写 / step 5 读的公共媒介")
        :table "event_log"
        :columns
          ("seq             BIGSERIAL PRIMARY KEY  -- tail 主索引兼 seq 权威"
           "domain          TEXT NOT NULL"
           "kind            TEXT NOT NULL"
           "payload_inline  JSONB             -- NULL 时参见 payload_ref (step 2 claim-check 决定)"
           "payload_ref     TEXT              -- PayloadRef JSON,>8KB 时使用"
           "producer_id     TEXT NOT NULL     -- dedup 语义依赖"
           "dedupe_key      UUID              -- 可空;(producer_id, dedupe_key) 唯一索引"
           "causation_depth SMALLINT NOT NULL DEFAULT 0"
           "trace_id        UUID"
           "span_id         UUID"
           "parent_span_id  UUID"
           "ts              TIMESTAMPTZ NOT NULL DEFAULT now()"
           "ephemeral       BOOLEAN NOT NULL DEFAULT false  -- TTL 分流标志")
        :secondary-indexes
          ("INDEX (domain, seq)                                       -- step 5 按域 catch-up 扫描"
           "UNIQUE (producer_id, dedupe_key) WHERE dedupe_key IS NOT NULL  -- step 3 dedup 依据"
           "INDEX (ts) WHERE ephemeral = true                         -- 快速 TTL 清理"))

      ;; ═══════════════════════════════════════════════
      ;; ─── 同步段:step 1-4 在 append() 调用栈内 ───
      ;; ═══════════════════════════════════════════════

      ;; ─── STEP 1 · guard ───────────────────────────
      (step-1 guard
        :target "crates/missiond-core/src/event/pipeline/step1_guard/"
        (purpose "入口校验 — 排除循环事件 / 确认类型有效")

        (component causation-guard
          :target "crates/missiond-core/src/event/pipeline/step1_guard/causation.rs"
          (desc "防 consumer 处理事件 → 触发新事件 → 再次消费 → 无限递归")
          :mechanism "每 append 的 causation_depth = 触发事件.depth + 1"
          :limit     "MAX_DEPTH = 10"
          :on-exceed "AppendError::CausalLoop,事件不入库,同时触发 IncidentEvent"
          :rationale "真实业务链很少超 5 层;10 给余量同时拦 bug")

        (component type-resolve
          :target "crates/missiond-core/src/event/pipeline/step1_guard/type_resolve.rs"
          (desc "调用 DomainEvent::domain() / kind() / payload_size_hint() 准备下一步")
          :uses "trait 返回值驱动后续所有步骤")

        :input  "(event: impl DomainEvent, opts: AppendOpts)"
        :output "validated event + metadata | AppendError::CausalLoop|SchemaMismatch")

      ;; ─── STEP 2 · decide ──────────────────────────
      (step-2 decide
        :target "crates/missiond-core/src/event/pipeline/step2_decide/"
        (purpose "决定 payload 存放方式 + 是否持久化")

        (component claim-check
          :target "crates/missiond-core/src/event/pipeline/step2_decide/claim_check.rs"
          (desc "大 payload 不进 Log 主表,只留 durable pointer")
          :threshold-inline "payload_size_hint() <= 8KB → 直接 JSONB 存 payload_inline"
          :threshold-ref    "payload_size_hint() >  8KB → blob_store.put(bytes) → 存 payload_ref"

          (struct PayloadRef
            :target "crates/missiond-core/src/event/blob_store/mod.rs"
            (field backend  :type "BlobBackend" :desc "blob-table (默认) / local-file")
            (field uri      :type "String"      :desc "backend 内定位键")
            (field size     :type "u64"         :desc "原始字节数")
            (field checksum :type "Sha256 hex"  :desc "完整性校验"))

          (backends 2
            (blob-table   :target "crates/missiond-core/src/event/blob_store/pg_backend.rs"
                          :desc "PostgreSQL blob_storage(id UUID PK, data BYTEA, size, checksum, ttl)")
            (local-file   :target "crates/missiond-core/src/event/blob_store/local_file_backend.rs"
                          :desc ".missiond/blobs/<prefix>/<uuid>,大 payload 或频繁读但不共享时"))

          (forbidden
            :in-memory-handle "禁止 — 重启即废"
            :s3-out-of-scope  "missiond 单机,暂不引入"))

        (component persistence-policy
          :target "crates/missiond-core/src/event/pipeline/step2_decide/persistence_policy.rs"
          (desc "ephemeral flag 决定 TTL 与 UI 分流,不决定是否写 DB")
          :default "持久化 — 所有 append 默认写 event_log (ephemeral=false, 30 天 TTL)"
          :ephemeral "AppendOpts.ephemeral=true 仍写 event_log 但标 ephemeral=true,3 天 TTL 自动清理"
          :use-case "ObservabilityEvent / HealthSnapshot / 高频心跳 默认 ephemeral=true"
          :rationale "producer 决策,不污染类型定义;retention + UI 根据 ephemeral 列分流")

        :input  "validated event"
        :output "(payload_inline | payload_ref) + ephemeral flag + 准备入库的 row")

      ;; ─── STEP 3 · commit ──────────────────────────
      (step-3 commit
        :target "crates/missiond-core/src/event/pipeline/step3_commit/"
        (purpose "单写入点批处理 + DB INSERT + 分配 seq + dedup 冲突解析")

        (component log-writer
          :target "crates/missiond-core/src/event/pipeline/step3_commit/log_writer.rs"
          :scope   "LogWriter struct + run loop + spawn 构造器 — 只负责 batch 调度,不含 backend 细节"
          :pattern   "唯一 LogWriter task 消费 append channel"
          :batching  "首条到达后 drain ≤100 条 / 10ms deadline,取先到"
          :invariant "append() Ok(Committed) ⟺ DB committed,不存在 in-flight 语义")

        (component handle
          :target "crates/missiond-core/src/event/pipeline/step3_commit/handle.rs"
          :scope "LogWriterHandle — 客户端 facade,impl Log trait;把 log.append() 调用封装成 PendingAppend 送入 channel,等待 oneshot ack"
          :implements "Log trait (定义在 log/mod.rs)")

        (component backend-abstraction
          :target "crates/missiond-core/src/event/pipeline/step3_commit/backend.rs"
          :scope "WriterBackend trait + InsertRow + BackendError — Writer 和底层 DB 实现之间的抽象接口"
          :rationale "隔离 PG 专用 SQL,方便 InMemoryLog / 未来 NATS 等替代实现")

        (component pg-backend
          :target "crates/missiond-core/src/event/pipeline/step3_commit/pg_backend.rs"
          :scope "PgWriterBackend impl WriterBackend + map_sqlx + is_unique_violation 等 PG SQL 辅助"
          :feature-gate "#[cfg(feature = \"postgres\")]")

        (component seq-authority
          :target "crates/missiond-core/src/event/pipeline/step3_commit/seq_authority.rs"
          :scope "Seq type 定义 + 权威性不变量 doc"
          :source         "DB BIGSERIAL — 全局严格单调,单点分配"
          :crash-recovery "DB 自己保存 max(seq),无应用层对账"
          :invariant      "seq 只增不减;已分配 seq 终生不变")

        (component dedup-semantics
          :target "crates/missiond-core/src/event/pipeline/step3_commit/dedup.rs"
          :scope "UNIQUE 违反检测 + collision → AlreadyExists 映射 + find_existing_seq 调用封装"
          :purpose            "Producer 重试保护,非业务去重"
          :key                "(producer_id, dedupe_key) UNIQUE INDEX WHERE dedupe_key IS NOT NULL"
          :collision-behavior "二次 append 相同 key → SELECT existing → 返回 Ok(AlreadyExists(seq)),无副作用"
          :producer-contract  "生产者超时/崩溃重试必须带同一 dedupe_key")

        (component backpressure
          :target "crates/missiond-core/src/event/pipeline/step3_commit/backpressure.rs"
          :scope "PendingAppend struct + APPEND_CHANNEL_CAPACITY 常量 + bounded mpsc try_send 包装"
          :channel   "append channel 有界 (默认 4096)"
          :overflow  "满则 append() 返回 Err(Backpressure),生产者决定重试/丢弃/panic"
          :rationale "可见失败 > 静默吞 > 无界内存膨胀")

        (component failure-mode
          :target "crates/missiond-core/src/event/pipeline/step3_commit/failure_mode.rs"
          :scope "FailureState atomic flag + exp_backoff + retry 状态机 + IncidentEvent emit"
          :retry  "batch INSERT 临时错误 → exp backoff 6 次"
          :fatal  "超限 → LogWriter 进 failed state,拒新 append → AppendError::LogUnavailable"
          :self-report "进 failed 时发 IncidentEvent::Reported(severity=critical)")

        :input  "prepared row (inline|ref, ephemeral flag, dedupe_key, trace 元数据)"
        :output "Ok(AppendAck::Committed{seq}) | Ok(AppendAck::AlreadyExists{seq}) | Err(Backpressure|LogUnavailable)")

      ;; ─── STEP 4 · ack ─────────────────────────────
      (step-4 ack
        :target "crates/missiond-core/src/event/pipeline/step4_ack/"
        (purpose "给 producer 回确定的 AppendAck / AppendError,同步链终结")

        (component ack-transport
          :target "crates/missiond-core/src/event/pipeline/step4_ack/ack_transport.rs"
          :mechanism     "oneshot::Sender<Result<AppendAck, AppendError>>"
          :return-timing "step 3 batch 落盘后立刻回每一个 pending oneshot"
          :ephemeral-path "ephemeral 事件 skip DB 语义下仍 append 成功 → 回 Ok(Volatile{seq})"
          :no-extra-hop "step 4 没有新组件 — 只是 step 3 的 out-bound 封装")

        :input  "step 3 结果"
        :output "return from log.append() to 4.1 ingress → producer")

      ;; ═══════════════════════════════════════════════
      ;; ─── 异步段:step 5-7 在独立 Dispatcher task ───
      ;; ═══════════════════════════════════════════════

      ;; ─── STEP 5 · tail ────────────────────────────
      (step-5 tail
        :target "crates/missiond-core/src/event/pipeline/step5_tail/"
        (purpose "Dispatcher 独立 task 从 event_log 拉新 seq,不阻塞同步链")

        (component dispatcher-state
          :target "crates/missiond-core/src/event/pipeline/step5_tail/mod.rs"
          :scope "Dispatcher struct + DispatchMetrics + re-exports"
          :state    "O(1) — AtomicI64 last_dispatched_seq"
          :does     "live fan-out 最新提交的事件给在线订阅者"
          :does-not "不替离线 consumer 扫库 / 不 global-min replay / 不维护 per-subscription 状态"
          :rationale "离线 consumer 一周不上线 → Dispatcher 零负担;Consumer 上线后自己 pull 补追")

        (component tail-source
          :target "crates/missiond-core/src/event/pipeline/step5_tail/tail_source.rs"
          :scope "TailSource trait(抽象)+ DispatchError + TailError"
          :rationale "抽象出 tail 数据来源,方便 InMemory / 未来 NATS 替代 PG")

        (component pg-tail
          :target "crates/missiond-core/src/event/pipeline/step5_tail/pg_tail.rs"
          :scope "PgTailSource impl TailSource — PG 长轮询 SELECT 实现"
          :feature-gate "#[cfg(feature = \"postgres\")]")

        (component tail-mechanism
          :target "crates/missiond-core/src/event/pipeline/step5_tail/dispatcher.rs"
          :scope "run_tail 主循环 + dispatch_one + control-gate 调用,编排各 TailSource impl"
          :source        "PostgreSQL 长轮询 SELECT WHERE seq > last_dispatched LIMIT 256 每 100ms"
          :future-optim  "可升级 LISTEN/NOTIFY,API 不变(记 revisit-trigger)"
          :ordering      "严格按 seq 升序,同 batch 保持 INSERT 顺序"
          :missed-events "崩溃重启从 last_dispatched 继续,不替订阅者补发")

        :input  "(no args — 轮询 event_log)"
        :output "Vec<LoggedEvent> (batch,严格 seq 升序)")

      ;; ─── STEP 6 · gate ────────────────────────────
      (step-6 gate
        :target "crates/missiond-core/src/event/pipeline/step6_gate/mod.rs"
        (purpose "paused domain 的事件不投递(事件仍在 log 中保留)")

        (component control-gate
          :target "crates/missiond-core/src/event/pipeline/step6_gate/mod.rs"
          :input-source "ControlManager.is_domain_paused(domain) — watch::Receiver<ControlTree>"
          :action       "paused=true 时跳过该 domain 投递,last_dispatched 仍前进"
          :stateless    "Dispatcher 不记录 per-subscription pause 时刻"
          :resume       "Subscription 侧决定 resume 语义(见 4.3 PauseBehavior)"
          :never-gated  "ObservabilityEvent / IncidentEvent 永远不被 gated(Domain→CtlDomain 映射 None)")

        (component domain-mapping
          :target "crates/missiond-core/src/event/pipeline/step6_gate/mod.rs"
          :function "fn domain_to_ctl_domain(d: Domain) -> Option<CtlDomain>"
          :maps     "Memory / Board → 对应 CtlDomain;其余 10 域 → None (永不 gated)"
          :daemon-adapter-target "crates/missiond-daemon/src/bus/control_gate_adapter.rs")

        :input  "LoggedEvent"
        :output "LoggedEvent (pass) | dropped (silent skip)")

      ;; ─── STEP 7 · fanout ──────────────────────────
      (step-7 fanout
        :target "crates/missiond-core/src/event/pipeline/step7_fanout/"
        (purpose "按 domain 送到对应 Topic<T> broadcast channel,出口到 4.3")

        (component topic-registry
          :target "crates/missiond-core/src/event/pipeline/step7_fanout/registry.rs"
          :type             "static HashMap<Domain, Box<dyn AnyTopic>>"
          :init-time        "Dispatcher 启动时 register::<T>() 注入 12 个 Topic"
          :fanout-transport "tokio::broadcast::channel<Arc<T>> per Topic"
          :buffer-size      "默认每 topic 1024;慢订阅者溢出触发 SlowConsumer incident")

        (component per-topic-fanout
          :target "crates/missiond-core/src/event/pipeline/step7_fanout/topic.rs"
          :mechanism  "event.domain() → registry.get(domain) → topic.broadcast_sender.send(Arc::new(event))"
          :isolation  "单 topic panic 由 supervisor 重启;其他 topic 零感知"
          :slow-subscriber "某订阅 Lagged → 仅该 Receiver 受影响,不传染")

        :input  "passed LoggedEvent (from step 6)"
        :output "→ broadcast::Sender<Arc<T>>.send(arc) → 进 4.3 egress")

      ;; ═══════════════════════════════════════════════
      ;; 生命周期维护(非流水线,定期触发)
      ;; ═══════════════════════════════════════════════
      (lifecycle-maintenance
        :target "crates/missiond-core/src/event/lifecycle/"
        (desc "不在 append/dispatch 主路径上的后台任务")

        (retention
          :target              "crates/missiond-core/src/event/lifecycle/retention.rs"
          :default-ttl         "30 天 — event_log 是恢复基座 + timeline SSOT"
          :per-domain-override "ObservabilityEvent = 3 天 可配"
          :ephemeral-ttl       "3 天"
          :cleanup-strategy    "每日清理 job,DELETE WHERE age > domain_ttl"
          :system_timeline-cutover
            "v1.3.0 正式声明 event_log = timeline SSOT; 实施进度: schema drop migration 待写, timeline-writer 订阅者待移除, mission_timeline / WS stream 待迁读 event_log; lisp 先行记录目标态")

        (orphan-cleanup
          :target   "crates/missiond-daemon/src/bus/retention_cron.rs"
          :policy   "event_subscriptions 的 last_seen_at 超 30 天未更新 → 归档 + 发 IncidentEvent::StaleSubscription"
          :re-subscribe "归档后再订阅 = 新订阅者,按 default Latest 起;恢复需 ops 跑重放脚本"))

      :replaces "原 timeline_mpsc_tx + run_timeline_writer + system_timeline + timeline_tx broadcast + event_router 8 consumers + sweeper 七合一"
      :merged-fallout "旧 4 条 MPSC:incident 升级为 event,cursor_ack 内化,embedding/ast_sync 降级为 worker 内部队列(见 4.1 dead-bypass)")

    ;; ═════════════════════════════════════════════════
    ;; 4.3 · 出点 (Egress) — 订阅 API
    ;; ═════════════════════════════════════════════════
    (section egress
      :target "crates/missiond-core/src/event/subscription/"
      (desc "消费者声明订阅 → tail-and-pull 双阶段接入 → combinators 声明式处理")

      (component subscription-api
        :target "crates/missiond-core/src/event/subscription/api.rs"
        (desc "类型安全的订阅入口")
        :primary-api "bus.subscribe::<T: DomainEvent>(name: &str, opts: SubscriptionOpts) -> Subscription<T>"

        (struct SubscriptionOpts
          :target "crates/missiond-core/src/event/subscription/options.rs"
          (field start-from     :type "StartFrom"     :default "Latest"                           :desc "Latest / Earliest / Seq(n)")
          (field batch-size     :type "usize"         :default "100"                              :desc "每批最大事件数;per-subscription 可调")
          (field failure-policy :type "FailurePolicy" :default "Retry { max: 3, backoff: exp }"   :desc "处理失败的回退")
          (field pause-behavior :type "PauseBehavior" :default "DropAndLiveResume"                :desc "pause 时的累积语义")
          (field cursor-flush   :type "CursorFlush"   :default "BatchOr1s"                        :desc "Cursor 持久化频率"))

        (enum FailurePolicy
          :target "crates/missiond-core/src/event/subscription/options.rs"
          :runtime "crates/missiond-core/src/event/subscription/failure.rs"
          (Retry        "max: u8, backoff: exp / fixed — 就地重试,超限转策略")
          (SkipToDLQ    "失败事件入 dead_letter_queue 表,cursor 推进,consumer 不被阻塞")
          (Halt         "失败即停止 consumer,等人工介入 — 适合关键路径"))

        (enum PauseBehavior
          :target "crates/missiond-core/src/event/subscription/options.rs"
          (DropAndLiveResume "默认:paused 期间不投递,resume 时 cursor 跳到 head")
          (FreezeAndCatchUp  "opt-in:paused 期间 cursor 冻结不前推;resume 时触发 pull catch-up(batch_size 节流)  ⚠ MVP 别名到 DropAndLiveResume,见 §4.5 deferred-implementations"))

        (enum CursorFlush
          :target "crates/missiond-core/src/event/subscription/options.rs"
          (PerEvent          "每条 ack 即 flush — 最小重复量,吞吐最差")
          (BatchOr1s         "默认:每 batch ack + 最长 1s 强制 flush — 平衡")
          (Periodic Duration "自定义周期 flush — 最松")))

      (component subscription-lifecycle
        :target "crates/missiond-core/src/event/subscription/lifecycle.rs"
        :scope "Lifecycle<T> struct + Phase enum + LifecycleError + Fetched<T> + phase-1/phase-2 切换编排"
        (desc "订阅者上线的两阶段模型 — tail-and-pull")

        (phase-1-bootstrap
          :action    "从持久 cursor 读 last_acked_seq → pull Log: SELECT WHERE seq > last_acked ORDER BY seq LIMIT batch_size"
          :loop      "处理 batch → ack → 继续 pull,直到 pull 返回空"
          :invariant "完成前不订阅 live stream,避免重复消费")

        (phase-2-live
          :action        "切 Dispatcher 的 Topic broadcast Receiver,进入 live 模式"
          :invariant     "live 模式下事件按 seq 严格单调到达(同 Dispatcher 派发顺序)"
          :on-disconnect "broadcast Lagged → 记 SlowConsumer incident → 切回 phase-1 重 pull"))

      (component live-source
        :target "crates/missiond-core/src/event/subscription/live_source.rs"
        :scope "LiveSource trait(订阅者接收 live 事件的抽象)+ BroadcastLiveSource + MpscLiveSource 两个实现"
        :rationale "Lifecycle 依赖 LiveSource trait,生产用 Broadcast(Topic),测试用 Mpsc,抽象/实现分离"
        :abstraction "LiveSource<T>: recv() + try_recv() 最小接口"
        :impls 2)

      (component cursor-store
        :target "crates/missiond-core/src/event/subscription/cursor_store.rs"
        :schema-target "crates/missiond-core/migrations/20260419000001_event_subscriptions.sql"
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
          :guarantee "崩溃最多**重复**处理 min(batch_size, 1 秒内事件数) 条;依赖 consumer 幂等消化"))

      (component delivery-semantics
        :target "crates/missiond-core/src/event/subscription/lifecycle.rs"
        :guarantee           "at-least-once — 消费者必须幂等"
        :rationale           "cursor update 与业务副作用不在同一事务,崩溃可能重放最后 batch"
        :idempotency-helper  "SeqDedupSet(Arc<Mutex<BTreeSet<Seq>>>) — consumer 想 seq 级幂等但业务无天然键时可用"
        :idempotency-helper-target "crates/missiond-core/src/event/subscription/mod.rs"
        :NOT-trait-enforced  "幂等是契约级要求,不走类型强制;强制 idempotency_key 会让天然幂等 consumer 编造伪 key"
        :design-contract     "每个 consumer 设计评审必须回答:同一事件重跑是否安全?如何保证?答案写进文档")

      (subscription-combinators
        :target "crates/missiond-core/src/event/subscription/combinators/"
        :trait-entries-target "crates/missiond-core/src/event/subscription/combinators/mod.rs"
        (desc "声明式订阅组合子,替代每个 consumer 手写样板;每个 combinator 独立文件")
        (debounce   :target "crates/missiond-core/src/event/subscription/combinators/debounce.rs"
                    :api "sub.debounce(Duration::from_millis(500))"
                    :semantics "固定 deadline 窗口,到期只触发一次,不滑动")
        (rate-limit :target "crates/missiond-core/src/event/subscription/combinators/rate_limit.rs"
                    :api "sub.rate_limit(max_per_sec)")
        (coalesce   :target "crates/missiond-core/src/event/subscription/combinators/coalesce.rs"
                    :api "sub.coalesce(|prev, new| ...)"
                    :semantics "合并语义相同的连续事件(如多条 StateChanged 只保留最终态)")
        (filter     :target "crates/missiond-core/src/event/subscription/combinators/filter.rs"
                    :api "sub.filter(|e| e.is_some_kind())")
        (map        :target "crates/missiond-core/src/event/subscription/combinators/map.rs"
                    :api "sub.map(|e| transform(e))")
        (batch      :target "crates/missiond-core/src/event/subscription/combinators/batch.rs"
                    :api "sub.batch(max: 50, window: 500ms) — 聚合成 Vec<E> 再投递"
                    :extra-type "EventBatch<T> 也在此文件")

        :rationale "旧 event_router 8 consumers 各自手写去抖;combinators 让模式声明化,实现一处"))

    ;; ═════════════════════════════════════════════════
    ;; 4.4 · 横切面 (Cross-cutting)
    ;; ═════════════════════════════════════════════════
    (section cross-cutting
      (desc "贯穿 4.1/4.2/4.3 所有相的系统属性 — 不属于任何单一相")

      (observability
        :target "crates/missiond-core/src/event/metrics/"
        :trait-target "crates/missiond-core/src/event/metrics/mod.rs"
        :emitter-target "crates/missiond-core/src/event/metrics/emitter.rs"
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
        :desc "每相独立的错误不扩散,集中契约分散实现"
        :producer-location   "producer 调用栈 (各 worker / handler)"
        :log-writer-target   "crates/missiond-core/src/event/pipeline/step3_commit/failure_mode.rs"
        :dispatcher-target   "crates/missiond-core/src/event/pipeline/step5_tail/mod.rs + step7_fanout/"
        :subscriber-target   "crates/missiond-core/src/event/subscription/lifecycle.rs"
        :producer   "append 失败抛 Err,不拖垮调用方"
        :log-writer "batch INSERT 失败 → exp backoff 重试 → 超限转 IncidentEvent + 拒新 append + DB unavailable 状态"
        :dispatcher "panic 由 supervisor 重启对应 topic task;Dispatcher 全体崩从 last_dispatched 继续,不替人补发"
        :subscriber "panic 断开该订阅,其他订阅者无感知;自动重订阅由消费者自行决策")

      (testing-story
        :in-memory-bus-target "crates/missiond-core/src/event/in_memory/"
        :in-memory-breakdown
          ("log.rs           — InMemoryLog struct + Log trait impl(公共 API)"
           "writer_task.rs   — WriterTask + Pending + payload_bytes(内部 writer 任务)"
           "storage.rs       — StoredRow + stored_to_logged 辅助 + IN_MEMORY_APPEND_CAPACITY"
           "observability.rs — ObservabilityAppender impl(metrics 桥接)"
           "cursor_store.rs / blob_store.rs / control_gate.rs — 各自同语义内存实现,各一文件")
        :chaos-tests-target   "crates/missiond-core/tests/event_chaos.rs"
        :integration-tests-target "crates/missiond-core/tests/event_log_integration.rs + event_dispatcher_integration.rs + event_subscription_integration.rs"
        :e2e-test-target "crates/missiond-daemon/tests/e2e_bus_golden_path.rs"
        :in-memory-bus "必须与生产同语义 — 单 writer 分 seq + append-ack + seq-ordered replay,不是裸 AtomicU64"
        :determinism   "可注入固定 seq / ts / trace_id,单测可重现"
        :replay-debug  "录制 Log 片段 → 换机 replay → 复现 bug")

      (chaos-test-matrix
        :target "crates/missiond-core/tests/event_chaos.rs"
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
      (desc "声明范围之外的事情,避免设计蔓延 — 无代码 target,未来实现")

      (not-now
        (distributed-bus         "单进程设计;未来如需跨进程,Log 可换 NATS/Redpanda,API 层保持")
        (exactly-once            "at-least-once + 幂等已够;不做两阶段提交")
        (projection-framework    "Dispatcher + cursor 已够搭 projection;通用框架暂不做")
        (schema-registry         "Rust 类型 + Forge 即 schema;独立 registry 暂不引入")
        (producer-token-auth     "当前文档问题非技术问题;未来可在 append-api 加 ProducerToken")
        (variant-level-topic     "当前 12 域 topic 足够;某 variant 真成热点再提升"))

      (deferred-implementations
        (desc "已在 lisp 声明但 MVP 未实现的功能,作为未来补齐基线")
        (freeze-and-catch-up
          :declared-in "§4.3 PauseBehavior::FreezeAndCatchUp"
          :current     "MVP alias 到 DropAndLiveResume(默认行为)"
          :future-work "实现 cursor 冻结 + resume 时 batch_size 节流 pull catch-up;对 Memory 等'不能丢'的域有价值"
          :future-target "crates/missiond-core/src/event/subscription/lifecycle.rs (新增分支)")
        (prometheus-metrics-backend
          :declared-in "§4.4 observability bus-metrics"
          :current     "AtomicBusMetrics MVP + ObservabilityEvent::BusMetric 自吐"
          :current-target "crates/missiond-core/src/event/metrics/mod.rs"
          :future-work "加 metrics/prometheus.rs 导出 HTTP /metrics endpoint,生产监控接入"
          :future-target "crates/missiond-core/src/event/metrics/prometheus.rs (新建)")
        (execution-event-domain
          :declared-in "§4.2 prerequisite event-types :: event-extensions"
          :current "implemented; Domain::ALL includes Execution, current domain count 13"
          :target "crates/missiond-core/src/event/events/execution.rs")
        (llm-provider-lifecycle-events
          :declared-in "§4.2 prerequisite event-types :: planned-event-extensions"
          :current "not implemented; xjp-router client should first return fail-fast provider errors"
          :future-work "xjp_router_client bootstrap/health 需要运行时可观测后,扩展 LlmEvent provider lifecycle variants"
          :future-target "crates/missiond-core/src/event/events/llm.rs"))

      (revisit-triggers
        (desc "触发重新评估 deferred 的条件")
        "若 Log 单表 >1B 行 → 考虑分片"
        "若某 topic QPS >10k → 考虑 variant-level topic"
        "若多进程 missiond 实例共享状态 → 考虑 distributed-bus"
        "若 exactly-once 成合规要求 → 考虑 outbox pattern"
        "若 Memory 域丢事件造成业务损失 → 实现 FreezeAndCatchUp"
        "若需外部监控告警 → 实现 Prometheus backend"
        "若 LISTEN/NOTIFY 替换长轮询有明显收益 → upgrade step 5 tail-mechanism"))

    ;; ═════════════════════════════════════════════════
    ;; 4.6 · 持久化层 (Persistence Layer) — 本 pillar 独占的 4 张 PG 表
    ;;   - v1.2.0 从 memory pillar :: table-catalog :: domain event-bus 划回
    ;;   - 其他 pillar 不直接读写这 4 张表
    ;;   - 本 section 是这 4 张表 schema 的真理源
    ;; ═════════════════════════════════════════════════
    (section persistence-layer
      (desc "event-bus 专属的 4 张 PG 表 schema + 访问模式 — pillar 物理底座")
      :migrated-from "memory pillar :: table-catalog :: domain event-bus (2026-04-19 v1.2.0)"
      :ownership-rule "本 pillar 独占; 其他 pillar 不直接读写, 只能通过 (append) / (subscribe) API 访问"

      (table event_log
        (purpose "Log-as-Bus 核心 — 追加式事件日志, BIGSERIAL seq 即全局单调权威")
        :migration "crates/missiond-core/migrations/20260419000000_event_log.sql"
        (columns
          (seq             :type "BIGSERIAL PRIMARY KEY"        :role "seq 权威, 全局单调")
          (domain          :type "TEXT NOT NULL"                :role "12 域之一, topic 路由键")
          (kind            :type "TEXT NOT NULL"                :role "domain variant name")
          (payload_inline  :type "JSONB"                        :role "≤8KB payload 直存")
          (payload_ref     :type "TEXT"                         :role ">8KB payload → blob_storage.id")
          (producer_id     :type "TEXT NOT NULL"                :role "去重组合键")
          (dedupe_key      :type "UUID"                         :role "producer 侧幂等")
          (causation_depth :type "SMALLINT NOT NULL DEFAULT 0"  :role "step-1 guard ≤ 10")
          (trace_id        :type "UUID"                         :role "端到端追踪")
          (span_id         :type "UUID"                         :role "当前事件 span")
          (parent_span_id  :type "UUID"                         :role "父 span")
          (ts              :type "TIMESTAMPTZ DEFAULT now()"    :role "创建时间")
          (ephemeral       :type "BOOLEAN NOT NULL DEFAULT false" :role "3 天清理标记 vs 30 天常规"))
        (indexes
          (idx-domain-seq   :cols "(domain, seq)"
                            :purpose "step-5 tail 按域 catch-up 扫描")
          (uq-dedupe        :cols "(producer_id, dedupe_key)"
                            :unique true
                            :partial "WHERE dedupe_key IS NOT NULL"
                            :purpose "producer 重试幂等保护")
          (idx-ephemeral-ts :cols "(ts)"
                            :partial "WHERE ephemeral = true"
                            :purpose "TTL 清理 ephemeral 行"))
        (access-patterns
          (write             "step-3 commit 单写点 — LogWriter 批量 INSERT RETURNING seq (SQL 在 pg_backend.rs)"
                             :target "crates/missiond-core/src/event/pipeline/step3_commit/pg_backend.rs")
          (read-live         "step-5 tail Dispatcher 长轮询 seq > cursor"
                             :target "crates/missiond-core/src/event/pipeline/step5_tail/pg_tail.rs")
          (read-catchup      "subscription 订阅启动时 phase-1 pull — lifecycle 调用 LogReader 执行 SELECT"
                             :target "crates/missiond-core/src/event/log/reader.rs")
          (read-ui-projection "v1.3.0+ — UI readers 经 projection 把 event_log 转成 timeline 形; event_type + payload 走 SSOT v1-wire mapper (原 'domain::kind' 仅保留于 AI-facing stats/stratified 分析接口, UI catch-up 路径已对齐 live ws_bridge)"
                              :consumers "mission_timeline MCP tool + WS timeline-event-stream"
                              :ssot-wire-mapper "crates/missiond-core/src/event/wire_format.rs"
                              :target ("crates/missiond-core/src/event/projection.rs"
                                       "crates/missiond-core/src/event/wire_format.rs"
                                       "crates/missiond-daemon/src/bus/ws_bridge.rs")
                              :drift-elimination "live (ws_bridge) + catch-up (projection) 共用 wire_format::v2_payload_to_v1_shape → event_type=v1 wire_type (如 'board_task_created'), payload=flat v1 shape; 12 个 byte-equiv 测试 (daemon) + 5 个 catch-up SSOT 测试 (core::event::projection) 锁定一致性"
                              :fts-requirement "event_log.payload_inline 需补 GIN FTS 索引 (mission_timeline FTS 搜索功能)")
          (retention         "lifecycle-maintenance 每日清理 ephemeral / 30 天过期"
                             :target "crates/missiond-core/src/event/lifecycle/retention.rs"))
        :ssot-declaration
          "v1.3.0+ event_log 正式作为 timeline 的唯一真理源 (SSOT); 原 system_timeline 表废弃, 待代码迁移完成后 DROP")

      (table event_subscriptions
        (purpose "订阅者 cursor 存储 — at-least-once 交付的状态载体")
        :migration "crates/missiond-core/migrations/20260419000001_event_subscriptions.sql"
        (columns
          (subscription_name :type "TEXT PRIMARY KEY"            :role "订阅唯一标识")
          (consumer_name     :type "TEXT NOT NULL"               :role "消费者名, 信息性")
          (domain            :type "TEXT NOT NULL"               :role "订阅的 domain")
          (last_acked_seq    :type "BIGINT NOT NULL DEFAULT 0"   :role "已 ack 的最高 seq")
          (last_seen_at      :type "TIMESTAMPTZ"                 :role "最近活跃时间")
          (failure_policy    :type "JSONB"                       :role "FailurePolicy 序列化")
          (created_at        :type "TIMESTAMPTZ DEFAULT now()"   :role "首次创建"))
        (indexes
          (idx-domain :cols "(domain)" :purpose "按域批量找订阅者"))
        (access-patterns
          (write-cursor-flush "subscription API lifecycle — batch ack 或 1s tick flush"
                              :target "crates/missiond-core/src/event/subscription/cursor_store.rs")
          (read-on-bootstrap  "订阅者启动时读 cursor 决定从哪开始 pull (SQL 在 cursor_store.rs, lifecycle 调用之)"
                              :target "crates/missiond-core/src/event/subscription/cursor_store.rs")
          (orphan-sweep       "retention_cron 每日扫 30 天未 seen 的 cursor → DELETE + incident"
                              :target "crates/missiond-daemon/src/bus/retention_cron.rs"))
        (invariant "subscription_name 全局唯一 (PK); last_acked_seq 只能单调递增"))

      (table blob_storage
        (purpose "claim-check side-channel — >8KB payload 的独立大对象存储")
        :migration "crates/missiond-core/migrations/20260419000002_blob_storage.sql"
        (columns
          (id          :type "UUID PRIMARY KEY DEFAULT gen_random_uuid()" :role "blob 标识, event_log.payload_ref 指向此")
          (data        :type "BYTEA NOT NULL"                   :role "原始 payload 字节")
          (size        :type "INTEGER NOT NULL"                 :role "字节数")
          (checksum    :type "TEXT NOT NULL"                    :role "sha256 hex, 完整性校验")
          (created_at  :type "TIMESTAMPTZ DEFAULT now()"        :role "创建时间")
          (ttl_expires :type "TIMESTAMPTZ"                      :role "可选 TTL 过期点"))
        (indexes
          (idx-ttl :cols "(ttl_expires)"
                   :partial "WHERE ttl_expires IS NOT NULL"
                   :purpose "TTL 清理扫描"))
        (access-patterns
          (write   "step-2 decide 判阈值 > 8KB → BlobStore.put() INSERT (SQL 在 blob_store/pg_backend.rs)"
                   :target "crates/missiond-core/src/event/blob_store/pg_backend.rs")
          (read    "log/reader.rs 解析 event_log 行, payload_ref 非空 → BlobStore.get() SELECT"
                   :target "crates/missiond-core/src/event/blob_store/pg_backend.rs")
          (cleanup "TTL 过期 blob 回收 — lifecycle/retention.rs DELETE WHERE ttl_expires < now()"
                   :target "crates/missiond-core/src/event/lifecycle/retention.rs"))
        (threshold "8KB — 在 §4.2 step-2 decide claim-check 判断"))

      (table dead_letter_queue
        (purpose "订阅者处理失败的事件归档 — 供人工排查 / 重放")
        :migration "crates/missiond-core/migrations/20260419000002_blob_storage.sql (同 migration 文件)"
        (columns
          (id                :type "BIGSERIAL PRIMARY KEY" :role "DLQ 自增 ID")
          (subscription_name :type "TEXT NOT NULL"        :role "来自哪个 subscription")
          (event_seq         :type "BIGINT NOT NULL"      :role "原 event_log.seq (快照, 非 FK)")
          (failure_reason    :type "TEXT NOT NULL"        :role "失败描述")
          (payload_snapshot  :type "JSONB"                :role "失败时 payload 快照")
          (created_at        :type "TIMESTAMPTZ DEFAULT now()" :role "DLQ 写入时间"))
        (indexes
          (idx-subscription :cols "(subscription_name, created_at DESC)"
                            :purpose "按订阅翻最近失败事件"))
        (access-patterns
          (write           "subscription runtime 触发 FailurePolicy::SkipToDLQ → PgDlqSink.record() INSERT"
                           :target "crates/missiond-core/src/event/subscription/failure.rs")
          (read            "人工 MCP 查询 / 重放工具 (待实现)")
          (no-auto-cleanup "DLQ 是故障留痕, 不自动清理 — 需人工处理"))
        (see "§4.3 FailurePolicy :: SkipToDLQ"))

      ;; ── 表之间的关系 ──
      (relationships
        (event_log-blob_storage
          :via "event_log.payload_ref → blob_storage.id"
          :kind "loose reference (TEXT, 非 FK)"
          :rationale "FK 会拖慢写入; loose reference + 回收策略更灵活")
        (event_subscriptions-event_log
          :via "event_subscriptions.last_acked_seq ≤ event_log.seq"
          :kind "logical cursor, 无 FK")
        (dead_letter_queue-event_log
          :via "dead_letter_queue.event_seq → event_log.seq"
          :kind "snapshot reference (非 FK) — 即使 event_log 清理也保留 DLQ"))

      ;; ── 跨 pillar 所有权声明 ──
      (ownership
        :owned-by      "pillar 四 event-bus"
        :not-shared    "其他 pillar 不直接 SQL 读写, 必须通过 (append) / (subscribe) API"
        :memory-pillar-pointer "memory :: table-catalog :: domain event-bus (v0.3.1+ 只含指针)")))
