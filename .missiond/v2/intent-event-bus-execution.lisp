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
    (phase-2 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-3 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-4 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-5 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-6 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-7 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-8 :status "pending" :owner nil :started nil :completed nil :summary nil)
    (phase-9 :status "pending" :owner nil :started nil :completed nil :summary nil))

  ;; ─ 并行锁表(防 agent 冲突) ─
  ;; 格式: (claim :phase N :scope "path/description" :agent "name" :claimed-at "..." :released-at "..."|nil)
  (claims
    ;; active claims here
    )

  ;; ─ 偏离 frozen lisp 的记录 ─
  ;; 格式: (deviation :id D001 :phase N :date "..." :agent "name"
  ;;                  :lisp-said "引用原 lisp 的决策"
  ;;                  :actually-did "实际实现"
  ;;                  :reason "为什么偏离"
  ;;                  :approved-by "user|auto|agent-consensus")
  (deviations
    ;; none yet
    )

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
           :resolution "Phase 3 在 control_tree.rs 加 Domain::to_ctl_domain() 多对一映射函数。映射表进 decisions"
           :resolved-at nil)
    (issue :id I003 :phase 7 :date "2026-04-19" :severity major
           :desc "控制闸语义变化风险:v1 paused domain 仍经过 Timeline Writer 入库并广播,consumer 自行 no-op;v2 paused domain 的事件不再 fan-out 给 subscriber。前端若依赖'暂停时仍能看到事件'会 break"
           :resolution "Phase 7 上线前,前端代码 grep 确认无此依赖;或保留 v1 fan-out-then-gate 行为到 WS 层(让 frontend_events_tx 单独 topic,不受 pause 影响)"
           :resolved-at nil)
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
              :rationale "frozen lisp 未让我们立即抛弃 legacy 语义,Phase 6 才会折叠。保留完整字段确保 v1 DB 行 round-trip 可行(Phase 2 shim 必须无损双向映射)。Phase 6 会引入 From impl 把 legacy 折叠入 RequestStarted { provider: Gemini/Codex, .. }。"))

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
      :notes "12 domain enum + DomainEvent trait + Domain enum。Variant 总数 55 = v1 49 variants 完整覆盖(含 LEGACY Gemini×3/Codex×2 保留)+ 6 new(SlotEvent::Stuck 占位 + ObservabilityEvent×3 新引入 + IncidentEvent×2 新引入)。I001 的 9 个遗漏 variant 全部解决(映射见 DC001)。未触碰 missiond-daemon::event_bus::DaemonEvent,与 v1 完全共存。serde 采用默认 externally tagged(见 DC002 原因)。Provider enum 独立于 CliEngine(DC003)。"))

  ;; ─ 全局备忘(跨阶段需要记住的事) ─
  (global-notes
    (historical-data-policy "system_timeline 旧数据不迁,保留 7 天 TTL 只读归档,3 月后废弃")
    (e2e-test-contract "Phase 9 前建立一条黄金路径测试: daemon 启动 → MCP board_create → event_log 写入 → WS 发送 → 前端收到"))
)
