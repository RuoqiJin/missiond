;; ═════════════════════════════════════════════════════════════
;; MissionD — Worker Pillar (phase-B informed v0.3)
;; 目标: 按 7 subsection pty / llm / xjp-router / context / worker / engine / orchestration
;;       重整, 基于 8 份老图 ground-truth 回填 v0.2 缺失的 "详尽设计"
;; 底稿: .missiond/intent-pillar-*.lisp (8 份) + drift-audit + memory v0.5.1 frozen
;; ═════════════════════════════════════════════════════════════

(pillar worker
  :version "v0.3"
  :status "phase-B informed 2026-04-21 — 吸收 8 份老图 ground-truth + 修 v0.2 结构盲区"
  :predecessor "v0.2 2026-04-21 (integrated by 主 Claude)"
  :target-path ".missiond/v2/intent-worker.lisp"
  :integration-notes
    ["v0.3 关键变更:"
     "  (1) section pty 重构为 5 subsection: pty-transport / semantic-parser / pty-state-machine / slot-orchestrator / learned-permissions"
     "  (2) 新增 (section xjp-router-gateway) — embedding 走 xjp-router, sonnet 去 embedding 职责"
     "  (3) engine-cluster 瘦身: learning-engine 标 'primary-ownership intent-layer pillar (待迁)', 触发机制留 worker"
     "  (4) worker-local 加 :functional-groups (cli-ingestion / 认知管道 / observability-log / code-intel / pty-runtime-hook / meta-briefing)"
     "  (5) pillar-egress 补 :mcp-surface-to-tools — 14+ MCP 工具映射到 worker path"
     "  (6) 补 event-categories 9 类 (v0.2 只列 5)"
     "  (7) 补 flow-engine v1 vs v2 区分 + 7 fail-fast-invariants"
     "  (8) 补 ControlTree 6 层 cascade 精细 semantics + ControlManager watch channel 零轮询"
     "  (9) 补 bootstrap 6 phase depends-graph (transport-bootstrap 老图 + v2 system-layer)"
     "  (10) 补 sole-spawn-bottleneck 10 callers 架构硬不变量"
     "  (11) 补 learned-permissions multi-scope (global/role/project/slot) + extract/learn/sync flow"
     "  (12) 补 registered-tasks 4 个 (arch_maintenance / strategy_analyst / gemini_router / lisp_survey)"
     "  (13) 标 lisp-survey-worker + arch-maintenance-worker 双重归属 (worker pillar = 触发, intent-layer pillar = 语义 ownership)"]
  :actual-state-sources
    [".missiond/v2/drift-audit-2026-04-21.md"
     ".missiond/v2/worker-pillar-execution.lisp"
     ".missiond/v2/intent-pillar-source-index.lisp"
     ".missiond/intent-pillar-event-workers.lisp (worker + event-bus + ControlTree 权威)"
     ".missiond/intent-pillar-engines.lisp (engine + slot-orchestrator + learned-permissions 权威)"
     ".missiond/intent-pillar-llm-context.lisp (LLM + context 权威)"
     ".missiond/intent-pillar-transport-bootstrap.lisp (bootstrap depends-graph 权威)"
     ".missiond/intent-pillar-semantic-parser.lisp (PTY 识别 pipeline 权威)"
     ".missiond/intent-pillar-state-machines.lisp (pty-session FSM 8+14 权威)"
     ".missiond/intent-pillar-mcp-dispatch.lisp (MCP 面 权威)"]
  :design-correction-sources
    [".missiond/v2/intent.lisp :: pillar worker"
     ".missiond/v2/intent.lisp :: pillar intent-layer (含 lisp-survey-worker + learning-engine 迁移依据)"
     ".missiond/v2/intent-memory.lisp v0.5.1 frozen"
     ".missiond/workflows/pillar-refactor.lisp"]
  :historical-footprint-sources
    ["旧 semantic-terminal crate (已 Forge 冲压到 missiond-core/src/semantic_parsing/)"
     "旧 briefing_worker / step_narrator / EventAnalyzerWorker / event_router (orphan coroutine) — 已删"
     "旧 git-watcher (infra/git_watcher.rs) — 替换为 ContextualCommitDetected + tagger_chunker 的 commit detection"
     "旧 flow-engine v1 (project-lifecycle phases) — 与 v2 并存, v2 为主"]

  ;; ══════════════════════════════════════════════════════════
  ;; phase-A-decisions (v0.2 保留 10 条, Q1-Q10)
  ;; ══════════════════════════════════════════════════════════
  (phase-A-decisions
    (Q1 :related-pre-deviation P-D001 :decision accept
        :answer "worker-cluster 按 WorkerKind 四分 (Sonnet/Codex/Gemini/Local)")
    (Q2 :related-pre-deviation P-D004 :decision accept
        :answer "engine-cluster 独立为顶级 section, 与 worker-cluster 并列")
    (Q3 :related-pre-deviation P-D004 :decision accept
        :answer "learning_engine 按 decision/extraction/analysis 3 sub group")
    (Q4 :related-pre-deviation P-D005 :decision accept-with-clarification
        :answer "Gemini 1 条 path `gemini-unified-gateway` + 5 文件 entry-components")
    (Q5 :related-pre-deviation P-D006 :decision accept
        :answer "infra cross-pillar-notes 独立块 + 相关 path step 双写")
    (Q6 :related-pre-deviation P-D008 :decision accept
        :answer "R/W table 以 path-level egress 为主, 每 WorkerKind 子节末补 contract-summary")
    (Q7 :related-pre-deviation P-D009 :decision accept
        :answer "context 独立为 (section context-assembly)")
    (Q8 :related-pre-deviation P-D010 :decision accept
        :answer "active = spawned ∪ on-demand-call; :lifecycle-style 四分")
    (Q9 :related-pre-deviation "global" :decision accept-with-adjustment
        :answer "v0.2 1000-1400, v0.3 调整到 2100-2400 (吸收老图深度)")
    (Q10 :related-pre-deviation "global" :decision accept
         :answer "保留 :actual-state-sources, 补指 8 份老图权威来源"))

  ;; ══════════════════════════════════════════════════════════
  ;; phase-B-decisions (v0.3 新增, 基于 5 个指挥官提问 + 8 老图)
  ;; ══════════════════════════════════════════════════════════
  (phase-B-decisions
    (Q-B1
      :question "embedding 是否该留 sonnet_gateway, 还是独立走 xjp-router?"
      :decision "独立 section xjp-router-gateway. sonnet_gateway 去掉 embedding 职责"
      :rationale "embedding 实际要接入 Windows 12900KF 上的 QWEN 模型, 经 xjp-router HTTP 服务路由. 目前 xjp-router 已接通, missiond 端尚未接入. 此 lisp 描述理想态"
      :code-status "pending — 需新增 HTTP client (路径 phase-C 施工确定)"
      :effect "embedding_worker step 3 从 sonnet_gateway.embed 改指 xjp-router-gateway.embed")

    (Q-B2
      :question "4 个 '扫外部 CLI state' 的 worker 是否该归组?"
      :decision "worker-local 加 :functional-groups, cli-ingestion group 含 conversation_logger + codex_ingestion + gemini_reconcile + reconcile"
      :rationale "这 4 个 worker 共性: '外部 CLI state → MissionD state' 桥接, 存储 ownership 在 memory pillar")

    (Q-B3
      :question "PTY 层级过于平铺, 如何恢复 semantic-parser / state-machine / slot-orchestrator / learned-permissions 的详尽设计?"
      :decision "section pty 重构为 5 subsection"
      :subsections
        ["pty-transport (missiond-pty 6 files — manager/session/screenshot/extractor/anomaly/lib)"
         "semantic-parser (pipeline 5-stage + 8 parser components + missiond-core/src/semantic_parsing + semantic-terminal-napi)"
         "pty-state-machine (pty-session FSM 8 states + 14 transitions)"
         "slot-orchestrator (11 files + sole-spawn-bottleneck + perm-injector + registered-tasks 4)"
         "learned-permissions (multi-scope + extract/learn/sync flow)"]
      :rationale "老图 intent-pillar-semantic-parser + state-machines + transport-bootstrap + engines 各自有专门 pillar, v0.2 压缩丢失设计密度")

    (Q-B4
      :question "engine-cluster 14 path 都该留 worker pillar 吗?"
      :decision "拆: runtime-mechanics 留 worker, 认知/学习逻辑搬 intent-layer pillar (待 intent-layer phase-A)"
      :留-worker
        ["intent-engine::autopilot-tick (timer 机制)"
         "intent-engine::memory-scheduler-queue (扫描 mechanics)"
         "intent-engine::workflow-executor-runtime (tool/slot 派发 mechanics)"
         "flow-engine-v2 (YAML 执行 mechanics, 3 path)"]
      :搬-intent-layer
        ["intent-engine::board-phase-engine (flow-engine v1, project-lifecycle phases — 认知规划性质)"
         "learning-engine 全家 7 sub (decision/extraction/analysis — 学习推理)"]
      :双重归属
        ["lisp_survey_worker: 触发 (worker pillar) + 语义 ownership (intent-layer pillar, v2 intent.lisp 已列)"
         "arch_maintenance_worker: 触发 (worker pillar) + 语义 ownership (intent-layer pillar)"]
      :rationale "v2 intent.lisp 里 intent-layer pillar 已声明 lisp-survey-worker + directive/plan/workflow specs + workflows ownership. worker pillar 应只管执行与 timing, intent-layer 管感知/演化/描述. ownership-by-usage")

    (Q-B5
      :question "是否向 tools pillar 显式暴露 mcp-surface?"
      :decision "pillar-egress 新增 :mcp-surface-to-tools, 列所有 worker 相关 MCP 工具到 worker path 的映射"
      :coverage "14 compute-tools (pty_* 7 + task_* 3 + compute_slot / slots / worker / job_poll / flow_run / forge_build / forge_lint / sonnet_process / minimax_process / agent) + 4 sysinfra-tools (control / pause / permission_query / permission_mutate)"
      :rationale "memory pillar v0.5.1 每个 module 都有 :mcp-surface, worker 应对齐"))

  (purpose "系统如何把计算派出去 — 7 层执行架构: PTY / LLM / xjp-router / context / worker / engine (runtime-mechanics) / orchestration-governance")

  (pillar-ingress
    (entry-1 "工具与外部入口: mission_pty_* 7 / mission_compute_slot / mission_worker / mission_task_* 3 / mission_flow_run / mission_forge_{build,lint} / mission_sonnet_process / mission_minimax_process / mission_agent / board task")
    (entry-2 "PTY 介质实时信号: terminal frame / screenshot / semantic state / JSONL / confirm dialog / stuck/idle anomaly")
    (entry-3 "event-bus 订阅触发: 9 类事件 (pty / message / task / board / slot / knowledge / system / cascade / commit)")
    (entry-4 "定时器 / sweep: autopilot tick / worker interval / reconcile tick / briefing interval / historical scan")
    (entry-5 "on-demand retrieval: context_pipeline::execute / code_prefetch / hybrid retrieval")
    (entry-6 "文件系统与项目变化: ContextualCommitDetected / git diff / project intent 更新 / ~/.codex / ~/.gemini / ~/.claude/projects"))

  (pillar-core
    (core-1 "WorkerKind 是 ontology (BackgroundWorker::KIND 必须匹配子目录); active-roster 只是实例清单")
    (core-2 "worker-cluster 与 engine-cluster 并列: worker = 被治理的计算租户; engine = 运行时机制 (timer/DAG/dispatch)")
    (core-3 "PTY / LLM / xjp-router / context 是 ingress adapter, 负责把外界输入变成可执行计算, 不是 memory owner")
    (core-4 "worker 对 memory 的所有 durable 读写必须 path-level 显式列出, 对齐 intent-memory.lisp 的 writer/reader 注册表")
    (core-5 "infra/ 归 system pillar, 但 worker data-plane 穿越点必须显式声明而非暗耦合")
    (core-6 "lifecycle-style 四分: spawned / on-demand / planned / zombie-deleted")
    (core-7 "search / retrieval / forge 属于 computation, 留在 worker pillar")
    (core-8 "sole-spawn-bottleneck: 所有 slot spawn 必经 spawner.rs::spawn_tracked_slot, 0 direct pty.spawn() 调用")
    (core-9 "LLM 优先级 actor 隔离: sonnet/gemini/minimax 各自独立限流 actor, llm_gate 做全局 kill-switch")
    (core-10 "embedding 独立 provider via xjp-router (非 sonnet_gateway), 禁止 fallback")
    (core-11 "learning-engine 的推理逻辑归 intent-layer pillar; worker pillar 只留 BackgroundWorker 触发机制")
    (core-12 "lisp-survey-worker 与 arch-maintenance-worker 双重归属: 触发 on worker pillar, 语义 ownership on intent-layer pillar"))

  (pillar-egress
    (egress-1 "把 durable state 写回 memory pillar 的 conversation-logs / board / kb-manager / slot-support / system-support / llm-support / embedding-support 模块")
    (egress-2 "向 event-bus 发射或消费 9 类 domain events (pty/message/task/board/slot/knowledge/system/cascade/commit); event_log 的 SSOT ownership 始终在 pillar event-bus")
    (egress-3 "返回 session handle / dispatch receipt / model output / retrieval result / workflow result 给上游调用者")
    (egress-4 "驱动后续 worker / engine / flow 节点 / slot execution 的下一跳")
    (egress-5 "写 intent-layer pillar 拥有的 lisp 文件: <project>/.missiond/intent.lisp (via slot execution) + arch manifest files")

    (cross-pillar-notes
      (memory
        :principle "worker 负责计算与时机, memory 负责 schema / trait / durable ownership"
        :writer-reader-pattern "每 path egress :writes / :reads / :via-bus 与 intent-memory.lisp v0.5.1 的 :binds-to 对齐"
        :table-cross-ref
          (conversation-logs ["conversations" "conversation_messages" "compaction_fragments" "message_labels" "message_translations" "tool_calls" "turns" "turn_topics" "retrospectives" "user_intents" "conversation_turns"])
          (board            ["board_tasks" "board_task_snapshots" "prompt_snapshots"])
          (kb-manager       ["kb_entries" "kb_embeddings" "beacon_nodes" "ast_files" "ast_nodes" "ast_embeddings" "ast_search_hits"])
          (project-management ["projects"])
          (slot-support     ["slot_sessions" "slot_tasks"])
          (system-support   ["incidents" "inbox_messages" "daemon_state" "reconcile_watermarks" "deep_analysis" "deep_analysis_checkpoint" "image_descriptions"])
          (llm-support      ["gemini_requests" "gemini_file_uploads" "token_usage_ledger"])
          (embedding-support ["embedding column on ast_nodes/kb_entries/turns/message payload"]))

      (system-infra
        :principle "infra/ 归 system pillar, 但 worker data-plane 明穿其中 3 个文件"
        (data-plane-through
          (ingestion-router "crates/missiond-daemon/src/infra/ingestion_router.rs — message classification → worker route")
          (message-handler  "crates/missiond-daemon/src/infra/message_handler.rs — JSONL normalize → DB write SSOT; project_id 通过 ProjectRegistry::resolve(cwd) 自动填充 (commit e18d0bf)")
          (session-util     "crates/missiond-daemon/src/infra/session_util.rs — PTY session UUID + project registry helper"))
        (not-owned-here
          ["crates/missiond-daemon/src/infra/aiops.rs"
           "crates/missiond-daemon/src/infra/daemon_stats.rs"
           "crates/missiond-daemon/src/infra/ipc_handler.rs"
           "crates/missiond-daemon/src/infra/mcp_client.rs"])
        (deleted
          ["infra/git_watcher.rs — commit 65c8b59 替换为 event_analyzer_worker, 后者又被吸收到 tagger_chunker (commit 1ea1838)"]))

      (event-bus
        :principle "worker 多数是 subscriber / emitter; append-only bus 与 replay contract 归 pillar event-bus"
        :target "crates/missiond-daemon/src/event_bus.rs"
        :event-categories
          (pty-events       ["PtyStateChanged" "PtyOutput" "PtyScreenshot"])
          (message-events   ["JsonlMessageIngested" "MessagePersisted" "ConversationMessageLogged"])
          (task-events      ["TaskSubmitted" "TaskCompleted" "TaskFailed"])
          (board-events     ["BoardTaskCreated" "BoardTaskUpdated" "BoardTaskClaimed"])
          (slot-events      ["SlotBecameIdle" "SlotStuck" "SlotSessionChanged"])
          (knowledge-events ["KbEntryCreated" "KbEntryUpdated"])
          (system-events    ["WorkerStatusChanged" "ShutdownRequested" "SystemEvent::ContextualCommitDetected"])
          (cascade-events   ["CascadeTriggered" "CascadeCompleted"])
          (commit-events    ["ContextualCommitDetected{conversation_id, session_id, slot_id, commit_hash, message}"])
        :event-router-status "demoted (commit c8b76b0) — 现在是 thin Notify signal emitter, 不再 spawn orphan coroutine; 所有 event-driven worker 改用 BackgroundWorker trait 接受 ControlTree pause/resume")

      (intent-layer
        :principle "intent-layer pillar 拥有 lisp 文件 / directive-layer specs / workflows / governance; worker pillar 拥有触发机制"
        :worker-pillar-triggers
          ["lisp_survey_worker: worker pillar 触发 (ContextualCommitDetected subscribe) + intent-layer 语义 ownership (更新 <project>/.missiond/intent.lisp)"
           "arch_maintenance_worker: worker pillar 触发 + intent-layer 语义 ownership (arch manifest 文件)"]
        :learning-engine-migration
          "7 sub-engine (decision_engine / decision_harvest / extraction / historical_scanner / idle_explorer / intent_analyst / timeline_analyst) — 代码文件 primary-ownership 应迁 intent-layer pillar; worker pillar 仅保留 BackgroundWorker 触发点. 当前 lisp 在 engine-cluster 留骨架, 细节待 intent-layer phase-A iteration"
        :flow-engine-split
          "v1 (flow_engine.rs, autopilot-driven project-lifecycle) 偏认知/规划, 拟归 intent-layer pillar; v2 (engine/flow/, YAML declarative runtime) 留 worker pillar"
        :forge-boundary
          "forge-build-bridge (mission_forge_build/lint shell out) 留 worker pillar; forge 本体 (lisp→IR→rust 冲压器) 归 intent-layer pillar")

      (flow
        :principle "flow 定义 + narrative 归 flow pillar; flow runtime execution engine 归 worker pillar (flow-engine-v2)"))

    ;; ── MCP Surface to Tools Pillar ──
    (mcp-surface-to-tools
      :principle "tools pillar 通过 MCP 工具调用 worker pillar 的 path; schema 归 tools pillar, 逻辑派发到这里的 path"
      :authority "对齐 .missiond/intent-pillar-mcp-dispatch.lisp 的 handler 映射"

      (compute-tools
        :handler-namespace "crates/missiond-mcp/src/tools/compute/"
        (mission_pty_spawn       :path "section pty :: subsection slot-orchestrator :: spawner (sole-spawn-bottleneck)")
        (mission_pty_send        :path "section pty :: subsection pty-transport :: path pty-session-lifecycle")
        (mission_pty_read        :path "section pty :: subsection pty-transport :: path pty-signal-extraction")
        (mission_pty_screenshot  :path "section pty :: subsection pty-transport :: screenshot.rs")
        (mission_pty_status      :path "section pty :: subsection pty-state-machine (查询 FSM 当前 state)")
        (mission_pty_signal      :path "section pty :: subsection pty-transport :: path pty-session-lifecycle (send signal)")
        (mission_pty_confirm     :path "section pty :: subsection learned-permissions (confirm dialog 手动 MCP 路径, 对偶于 auto-approve)")
        (mission_task_submit     :path "section engine-cluster :: intent-engine :: workflow-executor-runtime")
        (mission_task_query      :path "section engine-cluster :: intent-engine :: workflow-executor-runtime")
        (mission_task_cancel     :path "section engine-cluster :: intent-engine :: workflow-executor-runtime")
        (mission_task_delegate   :path "section engine-cluster :: intent-engine :: workflow-executor-runtime")
        (mission_compute_slot    :path "section pty :: subsection slot-orchestrator :: path {claude,gemini}-slot-dispatch")
        (mission_slots           :path "section pty :: subsection slot-orchestrator :: mod.rs (list)")
        (mission_worker          :path "section orchestration-governance :: path pause-resume-cascade (含 set_project P2+P3)")
        (mission_job_poll        :path "section engine-cluster :: intent-engine :: workflow-executor-runtime")
        (mission_flow_run        :path "section engine-cluster :: flow-engine-v2 3 path")
        (mission_forge_build     :path "section worker-side-computation :: path forge-build-bridge")
        (mission_forge_lint      :path "section worker-side-computation :: path forge-build-bridge")
        (mission_sonnet_process  :path "section llm-gateways :: path sonnet-priority-gateway")
        (mission_minimax_process :path "section llm-gateways :: path minimax-legacy-gateway")
        (mission_agent           :path "section pty :: subsection slot-orchestrator :: path claude-slot-dispatch (cc_tasks)"))

      (sysinfra-tools
        :handler-namespace "crates/missiond-mcp/src/tools/sysinfra/"
        (mission_control          :path "section orchestration-governance :: path pause-resume-cascade (set_global/provider/domain)")
        (mission_pause            :path "section orchestration-governance :: path pause-resume-cascade")
        (mission_permission_query :path "section pty :: subsection learned-permissions (merged_for_slot MCP view)")
        (mission_permission_mutate :path "section pty :: subsection learned-permissions (手动增删)"))

      (knowledge-tools-crossref
        :note "knowledge-tools (kb/memory/board/skill/intent 等) 主要归 memory pillar + intent-layer pillar; worker pillar 仅在 retrieval-fusion 与 forge 有 co-ownership"
        (mission_code_search      :cross-ref "worker-side-computation :: retrieval-fusion (code_prefetch + fusion ranker)")
        (mission_embedding_ops    :cross-ref "xjp-router-gateway :: path xjp-router-embedding")
        (mission_intent           :cross-ref "intent-layer pillar :: lisp files"))))

  ;; ══════════════════════════════════════════════════════════
  ;; 2.1 PTY Layer — 5 subsection 重构
  ;; ══════════════════════════════════════════════════════════
  (section pty
    :desc "PTY 介质: 把 Claude Code / Gemini CLI / Codex 等 CLI 进程当一等公民, 做感知 + 操控 + 工位管理 + 权限学习"
    :scope-crates
      ["crates/missiond-pty/ (PTY 传输)"
       "crates/missiond-core/src/semantic_parsing/ (语义识别 Forge)"
       "crates/semantic-terminal-napi/ (NAPI 壳, 前端可调)"
       "crates/missiond-daemon/src/slot_orchestrator/ (工位编排)"
       "crates/missiond-core/src/core/learned_permissions.rs (权限学习)"
       "crates/missiond-daemon/src/permission_extract.rs"
       "crates/missiond-daemon/src/handlers/sysinfra/permission.rs (merged_for_slot MCP)"
       "crates/missiond-daemon/src/infra/session_util.rs (bridge)"
       "crates/missiond-daemon/src/events_sync.rs (JSONL bridge)"]
    :v1-cross-ref
      ["intent-pillar-transport-bootstrap.lisp :: pty-manager"
       "intent-pillar-semantic-parser.lisp (完整 parser pipeline)"
       "intent-pillar-state-machines.lisp :: pty-session-state FSM"
       "intent-pillar-engines.lisp :: slot-orchestrator + learned-permissions"]

    ;; ─────────────────────────────────────────────
    ;; 2.1.1 PTY 传输层 (pty-transport)
    ;; ─────────────────────────────────────────────
    (subsection pty-transport
      :desc "底层 PTY I/O + 终端 grid 管理 + 异常监控 + 截屏"
      :targets
        ["crates/missiond-pty/src/lib.rs (导出聚合)"
         "crates/missiond-pty/src/manager.rs (PTYManager 多 session 管理 + broadcast 事件)"
         "crates/missiond-pty/src/session.rs (PTYSession: portable-pty + alacritty_terminal + semantic stack)"
         "crates/missiond-pty/src/extractor.rs (frame-by-frame 增量 Extractor + spinner/状态栏过滤)"
         "crates/missiond-pty/src/anomaly.rs (state stuck / parser 信心 / anchor 缺失 被动监控)"
         "crates/missiond-pty/src/screenshot.rs (终端网格 PNG 截屏 + ab_glyph + 两阶段 capture+render)"]

      (path pty-session-lifecycle
        :lifecycle-style "long-lived / on-demand"
        (ingress
          :source "mission_pty_spawn / slot dispatch / flow SlotTask / mission_compute_slot"
          :entry-components
            ["crates/missiond-pty/src/manager.rs"
             "crates/missiond-pty/src/session.rs"])
        (logic-core
          (step s1 "PTYManager 创建/附着/恢复 session, 维护 session_id → runtime handle 权威索引")
          (step s2 "PTYSession 接管读写循环, 绑定 CLI 进程 + alacritty_terminal grid + semantic stack")
          (step s3 "session 生命周期变化以 ManagerEvent 形式广播给 slot-orchestrator / pty_event_worker")
          (step s4 "崩溃/退出/state change 回到 runtime authority (slot 补偿或回收)"))
        (egress
          :writes []
          :reads []
          :via-bus ["ManagerEvent::TextComplete" "ManagerEvent::Exited" "ManagerEvent::StateChange" "ManagerEvent::ConfirmRequired" "PtyStateChanged"]
          :returns "session handle / runtime status"))

      (path pty-signal-extraction
        :lifecycle-style "streaming"
        (ingress
          :source "PTYSession frame / scrollback / terminal grid delta"
          :entry-components
            ["crates/missiond-pty/src/extractor.rs"
             "crates/missiond-pty/src/anomaly.rs"
             "crates/missiond-pty/src/screenshot.rs"])
        (logic-core
          (step s1 "extractor.rs 做 frame-by-frame 增量提取, 过滤 spinner / 状态栏 / 非稳定噪声")
          (step s2 "产物喂给 semantic-parser subsection 的 pipeline (5-stage)")
          (step s3 "anomaly.rs 被动监控 stuck / parser 信心低 / anchor 缺失, 形成异常信号")
          (step s4 "screenshot.rs 按需渲染终端 grid → PNG (ws/screenshot_broker 消费)")
          (step s5 "产物回流给 pty_event_worker / slot orchestration / tool 调用者, 不直接拥有 durable storage"))
        (egress
          :writes []
          :reads []
          :via-bus ["ManagerEvent::*" "PtyOutput" "PtyScreenshot"]
          :file-writes ["terminal screenshot PNG (ephemeral artifact 经 ws broker)"]
          :returns "visible text delta / anomaly signal / screenshot artifact")))

    ;; ─────────────────────────────────────────────
    ;; 2.1.2 语义识别层 (semantic-parser)
    ;; ─────────────────────────────────────────────
    (subsection semantic-parser
      :desc "multi-layer recognizer: raw PTY screen → structured states (idle/thinking/responding/tool/confirm/title/trust-dialog/error)"
      :history "原 semantic-terminal external crate (commit 5a5f805 EXTRACTED) 现已 Forge 冲压到 missiond-core/src/semantic_parsing/; semantic-terminal-napi 保留为前端 NAPI 壳"
      :targets
        ["crates/missiond-core/src/semantic_parsing/generated.rs (Forge 冲压)"
         "crates/missiond-core/src/semantic_parsing/custom.rs (手写补丁)"
         "crates/missiond-core/src/semantic_parsing/mod.rs"
         "crates/missiond-core/src/semantic/gen_parsing.rs (旧残留, pure-utility helpers)"
         "crates/semantic-terminal-napi/src/lib.rs (NAPI 前端导出)"]

      (pipeline parser-pipeline
        :stages
          [(stage-1 "pattern-config: CliEngine enum → engine-specific YAML + parser 分派")
           (stage-2 "fingerprint-registry: 识别终端屏幕签名 (Claude Code / Gemini / Codex / trust-dialog 等)")
           (stage-3 "state-parser: Claude Code 按 detection-order = trust-dialog → confirm-dialog → idle-or-slash → processing → responding → error; Gemini 按 error → thinking → responding → tool-running → idle → idle-placeholder → idle-transitional")
           (stage-4 "confirm-parser: 识别 permission dialog + 抽取 option text (如 'Yes, don't ask again for: python3:*') → ExtractedConfirm")
           (stage-5 "tool-output-parser: [Tool: Bash] / [Tool: Read] 等 tool call 格式解析 → tool name + param pattern")]
        :shared-resource "Arc<CompiledPatterns> from YAML hot-reload")

      (component claude-code-parser
        :purpose "Claude Code CLI 输出识别"
        :detection-order ["trust-dialog" "confirm-dialog" "idle-or-slash" "processing" "responding" "error"]
        :forge-source "crates/missiond-core/src/semantic_parsing/generated.rs (+ custom.rs patch)")

      (component gemini-parser
        :purpose "Gemini CLI 输出识别"
        :detection-order ["error" "thinking" "responding" "tool-running" "idle" "idle-placeholder" "idle-transitional"]
        :forge-source "crates/missiond-core/src/semantic_parsing/generated.rs")

      (component fingerprint    :role "屏幕签名识别, 决定走哪个 parser")
      (component confirm-parser :role "permission dialog 内容抽取")
      (component tool-parser    :role "tool invocation 格式解析")
      (component status-parser  :role "activity / timer 解析")
      (component title-parser   :role "terminal title 解析")

      (component semantic-parsing-helpers
        :purpose "pure-utility 纯函数 — 横向复用"
        :target "crates/missiond-core/src/semantic/gen_parsing.rs"
        :functions "is_spinner_char / split_args / extract_phase_from_parens / sanitize_line / has_activity_timer / is_idle_prompt"
        :consumer "extractor pipeline")

      (component semantic-terminal-napi
        :purpose "前端 JS/TS 可直接调用的 NAPI 桥"
        :target "crates/semantic-terminal-napi/src/lib.rs"
        :downstream "board-frontend 某些需要语义识别的场景"))

    ;; ─────────────────────────────────────────────
    ;; 2.1.3 PTY 状态机 (pty-state-machine)
    ;; ─────────────────────────────────────────────
    (subsection pty-state-machine
      :desc "pty-session 有限状态机 — 8 states + 14 transitions, 由 semantic-parser 触发状态迁移"
      :target "semantic-terminal crate (external/Forge) — src/types.rs"
      :state-authority "ManagerEvent::StateChange 广播状态迁移, 下游 pty_event_worker 与 slot-orchestrator 消费"

      (state-machine pty-session
        (states
          (Starting     "PTY 刚拉起, CLI 进程未 ready")
          (Idle         "CLI 显示 prompt, 等待输入")
          (SlashMenu    "用户触发 / 命令菜单")
          (Thinking     "CLI 显示 spinner / thinking 状态")
          (Responding   "模型在输出文本")
          (ToolRunning  "tool 调用执行中 (Bash/Read/Edit 等)")
          (Confirming   "permission dialog 等待确认")
          (Error        "异常状态 (crash / parser 失败 / timeout)"))

        (transitions
          (Starting    -> Idle         :trigger "prompt-detected")
          (Starting    -> Error        :trigger "process-crash")
          (Idle        -> Thinking     :trigger "spinner-detected")
          (Idle        -> SlashMenu    :trigger "slash-menu-detected")
          (Idle        -> ToolRunning  :trigger "tool-activity")
          (Idle        -> Confirming   :trigger "permission-dialog")
          (SlashMenu   -> Idle         :trigger "menu-dismissed")
          (Thinking    -> Responding   :trigger "output-begins")
          (Thinking    -> ToolRunning  :trigger "tool-hint")
          (Responding  -> Idle         :trigger "prompt-returns")
          (Responding  -> ToolRunning  :trigger "tool-invoked")
          (ToolRunning -> Idle         :trigger "prompt-returns")
          (ToolRunning -> Confirming   :trigger "permission-dialog")
          (Confirming  -> ToolRunning  :trigger "confirmed")
          (Confirming  -> Idle         :trigger "denied")))

      (related-state-machines
        :note "其他 FSM 也由 v1 intent-pillar-state-machines.lisp 权威, 但它们 ownership 不在 worker pillar"
        (board-task       :归属 "memory pillar :: module board (BoardTaskStatus enum)")
        (engineering-phase :归属 "intent-layer pillar (project lifecycle phases)")
        (task             :归属 "memory pillar :: module system-support")
        (question         :归属 "memory pillar :: module system-support")
        (extraction-phase :归属 "intent-layer pillar (learning-engine 迁移后)")))

    ;; ─────────────────────────────────────────────
    ;; 2.1.4 工位编排层 (slot-orchestrator)
    ;; ─────────────────────────────────────────────
    (subsection slot-orchestrator
      :desc "3 层架构: AgentSlotManager → {ClaudeCodeSlotManager, GeminiCliSlotManager} → EngineController; 承担 sole-spawn-bottleneck + perm 注入 + registered tasks"
      :targets
        ["crates/missiond-daemon/src/slot_orchestrator/mod.rs (3 层架构聚合)"
         "crates/missiond-daemon/src/slot_orchestrator/agent.rs (AgentSlotManager 顶层路由)"
         "crates/missiond-daemon/src/slot_orchestrator/types.rs (SlotTaskConfig/Request)"
         "crates/missiond-daemon/src/slot_orchestrator/controller.rs (EngineController trait)"
         "crates/missiond-daemon/src/slot_orchestrator/claude_code.rs (ClaudeCodeSlotManager — persistent Mutex + ephemeral 信号量)"
         "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs (CC PTY 操作 + JSONL session binding + TextComplete 抽取)"
         "crates/missiond-daemon/src/slot_orchestrator/gemini_cli.rs (GeminiCliSlotManager)"
         "crates/missiond-daemon/src/slot_orchestrator/gemini_controller.rs (Driver 委托 + synthetic session_id)"
         "crates/missiond-daemon/src/slot_orchestrator/spawner.rs (sole-spawn-bottleneck)"
         "crates/missiond-daemon/src/slot_orchestrator/perm_injector.rs"
         "crates/missiond-daemon/src/slot_orchestrator/gen_engine.rs (Forge shell)"]

      ;; ── 架构硬不变量: 统一 spawn 入口 ──
      (invariant sole-spawn-bottleneck
        :function "spawner::spawn_tracked_slot"
        :statement "ALL slot spawn paths go through this function; 0 direct pty.spawn() calls exist"
        :callers
          ["pty::mission_pty_spawn"
           "compute_slot::create_slot"
           "process::spawn+restart"
           "task::auto_spawn_exited"
           "flow_engine::ensure_slot_for_task"
           "memory_scheduler::ensure_memory_slot"
           "gemini_driver::ensure_spawned"
           "main::handle_slots_reload"
           "cc_controller::spawn_and_register"]
        :pipeline
          [(step perm-inject    "从 learned_permissions.yaml 读 global+role+project+slot union → settings.local.json")
           (step tracking-env   "注入 session tracking 环境变量")
           (step pty-spawn      "实际调 PTYManager.spawn (唯一落点)")
           (step uuid-capture   "捕获 session UUID, 归入 slot_sessions 表")
           (step initial-prompt "可选 initial_prompt 注入 (待 Idle 后发送)")])

      ;; ── SlotConfig 字段 ──
      (slot-config-fields
        (initial_prompt              :type "Option<String>" :doc "首条消息待 slot Idle 后注入")
        (dangerously_skip_permissions :type bool :serde-alias "dangerouslySkipPermissions")
        (mcp_config                  :type "Option<McpConfig>" :serde-alias "mcpConfig")
        (auto_start                  :type bool :serde-alias "autoStart"))

      ;; ── Registered Tasks (main.rs: "SlotManager: 4 tasks registered") ──
      (registered-tasks
        (arch_maintenance  :slot-id "arch-surveyor"  :model sonnet :timeout 900s
          :cross-ref "workers/sonnet/arch_maintenance_worker — 触发 on worker, 语义 ownership intent-layer")
        (strategy_analyst  :slot-id "strategy"       :model gemini :timeout 900s
          :cross-ref "workers/gemini/strategy_worker")
        (gemini_router     :slot-id "gemini-router"  :model gemini :timeout 900s
          :cross-ref "handlers/comm/router_chat (mission_router_chat)")
        (lisp_survey       :slot-id "lisp-surveyor"  :model sonnet :timeout 900s
          :added "commit 79a877f"
          :cross-ref "workers/sonnet/lisp_survey_worker — 触发 on worker, 语义 ownership intent-layer"))

      (path claude-slot-dispatch
        :lifecycle-style "on-demand"
        (ingress
          :source "autopilot / flow-engine / mission_compute_slot / mission_agent / slot task dispatch"
          :entry-components
            ["crates/missiond-daemon/src/slot_orchestrator/agent.rs"
             "crates/missiond-daemon/src/slot_orchestrator/claude_code.rs"
             "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs"
             "crates/missiond-daemon/src/slot_orchestrator/spawner.rs"
             "crates/missiond-daemon/src/slot_orchestrator/perm_injector.rs"
             "crates/missiond-daemon/src/infra/session_util.rs"])
        (logic-core
          (step s1 "agent.rs 依据 task_type / engine / lifecycle 选定 ClaudeCodeSlotManager + SlotTaskConfig")
          (step s2 "claude_code.rs 治理 persistent Mutex (memory/supervisor/strategy/lisp-surveyor/arch-surveyor 长驻) 与 ephemeral 信号量并发额度")
          (step s3 "若复用现有 slot, cc_controller.rs 把 prompt 写入已绑定 PTY; 否则经 spawner.rs → sole-bottleneck → perm_injector.rs → pty.spawn")
          (step s4 "cc_controller.rs 绑定 JSONL session (~/.claude/projects/<encoded>/<session_id>.jsonl), 等 TextComplete / idle")
          (step s5 "session_util.rs 辅助 session UUID / project_id 解析, ownership 不在 worker"))
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
            ["crates/missiond-daemon/src/slot_orchestrator/agent.rs"
             "crates/missiond-daemon/src/slot_orchestrator/gemini_cli.rs"
             "crates/missiond-daemon/src/slot_orchestrator/gemini_controller.rs"
             "crates/missiond-daemon/src/slot_orchestrator/spawner.rs"
             "crates/missiond-daemon/src/slot_orchestrator/perm_injector.rs"
             "crates/missiond-daemon/src/infra/session_util.rs"
             "crates/missiond-daemon/src/llm/gemini_driver.rs"])
        (logic-core
          (step s1 "agent.rs 选 GeminiCliSlotManager 路径, 同 task_type / config 路由协议")
          (step s2 "gemini_cli.rs 管理 persistent / ephemeral lifecycle 并发门禁")
          (step s3 "gemini_controller.rs 委托 gemini_driver 执行 '/clear + send' 等原子会话动作, synthetic session_id")
          (step s4 "spawner.rs / perm_injector.rs 仍是统一 bottleneck")
          (step s5 "执行结果与运行态经 PTY 事件 + slot receipt 回流"))
        (egress
          :writes []
          :reads []
          :via-bus ["ManagerEvent::*" "SlotBecameIdle" "SlotStuck"]
          :returns "gemini slot dispatch receipt / session binding result"))

      (path slot-manager-runtime-authority
        :lifecycle-style "long-lived"
        :note "旧 slot_manager/ 目录已合入 slot_orchestrator, 但 runtime authority 语义保留 — list_slots / kill_slot / session 归属统一由 slot_orchestrator mod.rs 管"
        (ingress
          :source "mission_slots / autopilot supervision-check / main::handle_slots_reload"
          :entry-components
            ["crates/missiond-daemon/src/slot_orchestrator/mod.rs"])
        (logic-core
          (step s1 "list_slots 返回所有已 spawn slot 的 runtime state + role + session_id")
          (step s2 "supervision-check: lease recovery / stale slot 回收 / zombie slot 清理")
          (step s3 "slots.yaml reload 触发 diff — 新增/删除/更新 slot 配置"))
        (egress
          :writes ["slot_sessions"]
          :reads ["slot_sessions"]
          :via-bus ["SlotSessionChanged"]
          :memory-cross-ref ["slot-support"]
          :returns "slot list / runtime state snapshot")))

    ;; ─────────────────────────────────────────────
    ;; 2.1.5 Learned Permissions (权限学习层)
    ;; ─────────────────────────────────────────────
    (subsection learned-permissions
      :desc "auto-approve permission dialog 的学习 + 持久化 + multi-scope merge + 注入 settings.local.json"
      :added "commit ec269d7 + Phase 1-5 upgrade 2026-04-12"
      :targets
        ["crates/missiond-core/src/core/learned_permissions.rs (authority)"
         "crates/missiond-daemon/src/permission_extract.rs (共享抽取模块)"
         "crates/missiond-daemon/src/slot_orchestrator/perm_injector.rs (注入)"
         "crates/missiond-daemon/src/handlers/sysinfra/permission.rs (mission_permission_* MCP)"
         "crates/missiond-daemon/src/workers/local/pty_event_worker.rs (auto-approve 99% 路径)"]

      (invariant REQUIRES_PARAM_PATTERN
        :semantics "bare Bash (no param_pattern) rejected as too dangerous"
        :example "'python3:*' / 'npm test:*' 这种带具体 subcommand pattern 才持久化")

      (scope-model
        :precedence-at-spawn "slot > project > role > global (more specific wins on dedup)"
        (global  :scope_id ""               :applies-to "every spawn")
        (role    :scope_id "<role>"         :applies-to "role-wide (如 memory / supervisor)")
        (project :scope_id "<project_id>"   :applies-to "项目 (ProjectRegistry::resolve(cwd))")
        (slot    :scope_id "<slot_id>"      :applies-to "per-slot overrides"))

      (method get_for_spawn
        :args "role: &str, project_id: Option<&str>, slot_id: Option<&str>"
        :returns "Vec<LearnedPermission>"
        :semantics "union across all applicable scopes with later-wins dedup on (tool_pattern, param_pattern)")

      (flow permission-persistence
        :trigger "pty_event_worker::handle_confirm_required (auto-approve 99%) + mission_pty_confirm MCP (手动 1%)"
        :steps
          [(step s1 "confirm dialog 文本含 'don't ask again'/'always'/'trust'/'不再' → use_allowlist=true")
           (step s2 "permission_extract::extract_confirm(opt2_text) → ExtractedConfirm{pattern, project_path}")
           (step s3 "LearnedPermissions::learn(role, role_id, tool, allow, pattern) [always]")
           (step s4 "if project_path Some → ProjectRegistry::resolve → LearnedPermissions::learn(project, pid, tool, allow, pattern)")
           (step s5 "ConfirmResponse::Option(2) 作为 PTY 写入 (digit + Enter, 80ms apart; unicode curly apostrophe U+2019 归一)")
           (step s6 "next spawn: perm_injector::sync_learned_to_local_settings(cwd, role, project_id, slot_id, learned) — 合并进 <cwd>/.claude/settings.local.json (idempotent, dedup)")])

      (path learned-permission-read
        :lifecycle-style "on-demand"
        (ingress
          :source "spawn_tracked_slot (每次 slot spawn) / mission_permission_query (debug)"
          :entry-components
            ["crates/missiond-core/src/core/learned_permissions.rs"
             "crates/missiond-daemon/src/slot_orchestrator/perm_injector.rs"])
        (logic-core
          (step s1 "按 scope-model precedence 从 learned_permissions.yaml 读取 union")
          (step s2 "perm_injector 合并到 <cwd>/.claude/settings.local.json (preserves existing, dedup 按 (tool_pattern, param_pattern))")
          (step s3 "供 slot spawn 使用, slot 启动后 CC/Gemini CLI 自动从该文件读权限白名单"))
        (egress
          :writes []
          :reads []
          :file-writes ["<cwd>/.claude/settings.local.json (merged, idempotent)"]
          :returns "learned permission union"))

      (path learned-permission-write
        :lifecycle-style "event-driven (触发自 pty_event_worker)"
        (ingress
          :source "ManagerEvent::ConfirmRequired (auto-approve branch)"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
             "crates/missiond-daemon/src/permission_extract.rs"
             "crates/missiond-core/src/core/learned_permissions.rs"])
        (logic-core
          (step s1 "判定 auto-approve 条件 (dialog 含 trust/always/不再 等关键词)")
          (step s2 "extract_confirm(option_text) → ExtractedConfirm{pattern, project_path}")
          (step s3 "写 learned_permissions.yaml 的 role 与 project 两条 scope")
          (step s4 "回写 ConfirmResponse::Option(2) 到 PTY (digit + Enter, 80ms)"))
        (egress
          :writes []
          :reads []
          :file-writes ["learned_permissions.yaml (role scope + project scope)"]
          :via-bus []
          :returns "permission learned confirmation"))

      (mcp-merged-view
        :tool mission_permission_query
        :action merged_for_slot
        :target "crates/missiond-daemon/src/handlers/sysinfra/permission.rs"
        :returns "{slotId, role, cwd, projectId, learned:[LearnedPermission], staticRoleRule, staticSlotRule}"
        :doc "显示给定 slot spawn 时能看到的 permission union — debug/audit 视图"))

    (contract-summary
      :writes-for-pty-section ["slot_sessions"]
      :reads-for-pty-section []
      :file-writes ["<cwd>/.claude/settings.local.json" "learned_permissions.yaml" "terminal PNG artifacts (ephemeral)"]
      :event-emits ["ManagerEvent::*" "PtyStateChanged" "PtyOutput" "PtyScreenshot" "SlotBecameIdle" "SlotStuck" "SlotSessionChanged"]
      :event-consumes ["ManagerEvent::ConfirmRequired (learned permission)"]))

  ;; ══════════════════════════════════════════════════════════
  ;; 2.2 LLM Gateway Layer (embedding 已移除, 独立 xjp-router)
  ;; ══════════════════════════════════════════════════════════
  (section llm-gateways
    :desc "多 provider LLM 统一门面, 优先级 actor 隔离 + kill-switch + prompt 模板"
    :embedding-note "embedding 不再由此 section 承担 — 见 section xjp-router-gateway"
    :targets
      ["crates/missiond-daemon/src/llm/mod.rs"
       "crates/missiond-daemon/src/llm/llm_gateway.rs (顶层路由门面)"
       "crates/missiond-daemon/src/llm/llm_gate.rs (kill-switch + rate limiter + 429 backoff)"
       "crates/missiond-daemon/src/llm/sonnet_gateway.rs (Sonnet 优先级 actor, 30 RPM)"
       "crates/missiond-daemon/src/llm/gemini_driver.rs (Gemini PTY 统一驱动)"
       "crates/missiond-daemon/src/llm/gemini_cli.rs (stream-json CLI 子进程)"
       "crates/missiond-daemon/src/llm/gemini_client.rs (HTTP/CLI 模式切换)"
       "crates/missiond-daemon/src/llm/gemini_pty.rs (PTY 传输层)"
       "crates/missiond-daemon/src/llm/gemini_file_api.rs (multimodal 上传)"
       "crates/missiond-daemon/src/llm/codex_cli.rs (Codex CLI vision)"
       "crates/missiond-daemon/src/llm/minimax_gateway.rs (legacy briefing)"
       "crates/missiond-daemon/src/llm/minimax_client.rs"
       "crates/missiond-daemon/src/llm/prompts.rs (中央 prompt 存储 + Tier2/3 模板)"
       "crates/missiond-daemon/src/llm/gen_engine.rs (Forge shell)"]

    (path llm-request-routing
      :lifecycle-style "on-demand"
      (ingress
        :source "worker / engine / handler / flow node 的 chat LLM 请求"
        :entry-components
          ["crates/missiond-daemon/src/llm/llm_gateway.rs"
           "crates/missiond-daemon/src/llm/llm_gate.rs"])
      (logic-core
        (step s1 "llm_gate.rs 做 kill-switch (AtomicBool 热路径) + rate-limit + 429 backoff + provider disable 持久化")
        (step s2 "check_interactive_exempt(provider) — REQUEST_CALLER task-local 判定: router_chat 等用户 MCP 调用绕过 gate; 后台 worker 受限")
        (step s3 "llm_gateway.rs 按 model/provider 分发到 sonnet / gemini / codex / minimax(legacy) 路径")
        (step s4 "provider 返回文本/错误, 顶层门面统一失败语义回传"))
      (egress
        :writes []
        :reads []
        :via-bus ["LlmEvent::RequestStarted" "LlmEvent::ResponseCompleted"]
        :returns "model output / error / provider routing result"))

    (path sonnet-priority-gateway
      :lifecycle-style "on-demand"
      :embedding-removed "v0.3 — embedding 移交 xjp-router-gateway, sonnet 只做 chat"
      (ingress
        :source "translation_worker / retro_worker / arch_maintenance_worker / lisp_survey_worker / direct sonnet caller"
        :entry-components
          ["crates/missiond-daemon/src/llm/sonnet_gateway.rs"])
      (logic-core
        (step s1 "sonnet_gateway.rs 独立优先级 actor + 30 RPM provider 治理")
        (step s2 "model const SONNET_MODEL='claude-sonnet' (commit 43c80f4)")
        (step s3 "失败/限流/成功路径回到顶层 llm_gateway"))
      (egress
        :writes []
        :reads []
        :via-bus ["LlmEvent::*"]
        :returns "sonnet chat result"))

    (path gemini-unified-gateway
      :lifecycle-style "on-demand"
      (ingress
        :source "strategy_worker / gemini slot path / multimodal file request / direct Gemini API caller / gemini-router slot"
        :entry-components
          ["crates/missiond-daemon/src/llm/gemini_driver.rs"
           "crates/missiond-daemon/src/llm/gemini_cli.rs"
           "crates/missiond-daemon/src/llm/gemini_client.rs"
           "crates/missiond-daemon/src/llm/gemini_pty.rs"
           "crates/missiond-daemon/src/llm/gemini_file_api.rs"])
      (logic-core
        (step s1 "gemini_client.rs HTTP/CLI 模式切换 + 速率限制")
        (step s2 "gemini_driver.rs PTY 会话统一成 driver, 管 /clear 隔离 + @file 上传 + 事件机制")
        (step s3 "gemini_cli.rs 管理 stream-json CLI 子进程, tool 执行扩展超时")
        (step s4 "gemini_pty.rs PTY 传输 + Mutex 原子 /clear+send")
        (step s5 "gemini_file_api.rs multimodal (PDF/video) 走 file API + 缓存去重"))
      (egress
        :writes []
        :reads []
        :via-bus ["LlmEvent::*"]
        :memory-cross-ref ["llm-support"]
        :returns "Gemini text / multimodal result / PTY driver result"))

    (path codex-cli-gateway
      :lifecycle-style "on-demand"
      (ingress
        :source "vision_worker / image-aware codex caller"
        :entry-components
          ["crates/missiond-daemon/src/llm/codex_cli.rs"])
      (logic-core
        (step s1 "codex_cli.rs 包装 Codex/Claude Code 风格 CLI, 支持 vision/image 输入")
        (step s2 "调用前 multimodal payload 归一成 CLI 格式")
        (step s3 "解析 JSONL/stdout 事件回传"))
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
        (step s1 "minimax_gateway.rs 优先级 actor + 4 通道 + 配额跟踪")
        (step s2 "minimax_client.rs 轻量 HTTP")
        (step s3 "phase-A 标记 legacy, 不作主执行面"))
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
        (step s1 "读中央 prompt 模板 + 文件 override (热加载)")
        (step s2 "按场景选 Tier2/Tier3 模板, 与 caller 提供的 prompt bundle 合成")
        (step s3 "解析结果交回 llm_gateway / worker / flow handler"))
      (egress
        :writes []
        :reads []
        :via-bus []
        :returns "resolved prompt template bundle")))

  ;; ══════════════════════════════════════════════════════════
  ;; 2.3 Xjp-Router Gateway (新 — embedding + 未来 chat/rerank)
  ;; ══════════════════════════════════════════════════════════
  (section xjp-router-gateway
    :desc "missiond ↔ xjp-router 服务的统一 HTTP 门面; xjp-router 运行在 Windows 12900KF + RTX3090Ti, 承载 QWEN embedding (+ 未来 chat/rerank)"
    :status "**已接通未接入** — xjp-router 端已 ready, missiond 端尚无 HTTP client, 本 section 描述理想态, 文件落点 phase-C 施工确定"
    :external-service
      ["service-name: xjp-router"
       "host: Windows 12900KF + RTX3090Ti (tailscale + 公网反代可选)"
       "protocol: HTTP JSON"
       "current-endpoints: POST /embed (QWEN3)"
       "future-endpoints: POST /chat (QWEN chat) / POST /rerank"]
    :design-constraints
      ["禁止 fallback (feedback_fail_fast_no_fallback, feedback_no_fallback_embedding)"
       "embedding 只用 QWEN3, 失败直接报错 (不降级到其他 provider)"
       "配置经 secret-store / xjp-mcp-config / daemon env (phase-C 决定)"]
    :targets-pending
      ["crates/missiond-daemon/src/llm/xjp_router_client.rs (pending — 新建 HTTP client)"
       "可能加进 Cargo.toml: reqwest/hyper (多半已存在 transitively)"
       "daemon config: xjp_router_endpoint / xjp_router_auth_token"]

    (path xjp-router-embedding
      :lifecycle-style "on-demand"
      :status "pending-implementation"
      (ingress
        :source "embedding_worker EmbeddingTask 处理管道 + 未来 retrieval 预热"
        :entry-components
          ["crates/missiond-daemon/src/llm/xjp_router_client.rs (pending)"])
      (logic-core
        (step s1 "embedding_worker 组装 EmbeddingTask batch (text 列表)")
        (step s2 "xjp_router_client.embed(texts) → HTTP POST /embed {model: qwen3, texts: [...]}")
        (step s3 "路由到 Windows 12900KF 上的 QWEN3 embedding server")
        (step s4 "返回向量数组, 失败直接上抛 (禁止 fallback 到 sonnet/gemini)")
        (step s5 "embedding_worker 写回 kb_embeddings / ast_embeddings / turn_topics"))
      (egress
        :writes []
        :reads []
        :via-bus ["LlmEvent::* (同 chat LLM, 统一观测)"]
        :returns "embedding vec or fail-fast error"))

    (path xjp-router-chat-future
      :lifecycle-style "planned"
      :status "not-yet-designed"
      :rationale "未来 xjp-router 可承载 QWEN chat, 供 agent 级 chat 调用 (替代部分 gemini/sonnet 路径). 现阶段仅占位, 实际 endpoint 与 caller 设计待需求明确")

    (path xjp-router-rerank-future
      :lifecycle-style "planned"
      :status "not-yet-designed"
      :rationale "检索融合 (retrieval-fusion) 后做 rerank 可能需要更强模型, 走 xjp-router 是候选路径之一"))

  ;; ══════════════════════════════════════════════════════════
  ;; 2.4 Context Assembly (独立于 LLM gateway)
  ;; ══════════════════════════════════════════════════════════
  (section context-assembly
    :desc "LLM 调用前的 context 装配 — source ranking + budget + retrieval, 独立于 llm-gateways"
    :targets
      ["crates/missiond-daemon/src/context/mod.rs"
       "crates/missiond-daemon/src/context/slot_env.rs"
       "crates/missiond-daemon/src/context/claude_md_sync.rs"
       "crates/missiond-daemon/src/context/topology_map.rs"
       "crates/missiond-daemon/src/context/context_budget.rs"
       "crates/missiond-daemon/src/context/context_pipeline.rs"
       "crates/missiond-daemon/src/context/pure_budget/generated.rs"
       "crates/missiond-daemon/src/context/pure_budget/custom.rs"
       "crates/missiond-daemon/src/context/pure_budget/mod.rs"]
    :source-priority "slot-env → skill-context → kb-entries → conversation-history → topology-map → claude-md"

    (path slot-env-build
      :lifecycle-style "on-demand"
      (ingress
        :source "slot 激活 / LLM 调用前的 runtime context build"
        :entry-components ["crates/missiond-daemon/src/context/slot_env.rs"])
      (logic-core
        (step s1 "slot_env.rs 收集 role / cwd / project / session tracking file / secret resolve")
        (step s2 "归一成 slot 可直接消费的 env var + prompt 前置元信息")
        (step s3 "结果返回给 context_pipeline"))
      (egress
        :writes []
        :reads []
        :via-bus []
        :returns "slot-scoped environment bundle"))

    (path claude-md-managed-sync
      :lifecycle-style "on-demand / sync"
      (ingress
        :source "context assemble 需要项目托管段 / hot topics / preferences 时"
        :entry-components ["crates/missiond-daemon/src/context/claude_md_sync.rs"])
      (logic-core
        (step s1 "从 KB / project memory 抽取适合托管进 CLAUDE.md 的 preferences 与 hot topics")
        (step s2 "同步到项目 CLAUDE.md 的约定位置 (managed sections)")
        (step s3 "同步结果回给 context pipeline 作为一个 source"))
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
        :source "context pipeline 需要模块导航 / AST+KB 聚合 / 降级查询"
        :entry-components ["crates/missiond-daemon/src/context/topology_map.rs"])
      (logic-core
        (step s1 "组合 AST 结构、KB 片段与模块导航信息")
        (step s2 "优先结构化导航; 缺失时降级查询")
        (step s3 "返回路径/模块级导航结果"))
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
           "crates/missiond-daemon/src/context/pure_budget/"])
      (logic-core
        (step s1 "按 source-priority 拼: slot-env → skill-context → kb → conversation-history → topology-map → claude-md")
        (step s2 "on-demand 调 workers/local/code_prefetch.rs 做代码检索 (非 BackgroundWorker)")
        (step s3 "context_budget + pure_budget 估 token/byte + 6MB 上限裁剪 + 源间分配衰减")
        (step s4 "并发发起 retrieval, 汇总成 assembled bundle")
        (step s5 "bundle 交 llm_gateway / slot dispatch, 自身不拥有 provider 路由"))
      (egress
        :writes []
        :reads ["kb_entries" "conversations" "conversation_messages" "ast_nodes" "beacon_nodes" "ast_search_hits"]
        :via-bus []
        :memory-cross-ref ["kb-manager" "conversation-logs"]
        :returns "assembled prompt bundle + source trace"
        :cross-ref "worker-local/code-prefetch = on-demand retrieval dependency")))

  ;; ══════════════════════════════════════════════════════════
  ;; 2.5 Worker Cluster — 按 WorkerKind 四分 + functional-groups
  ;; ══════════════════════════════════════════════════════════
  (section worker-cluster
    :active-definition "spawned ∪ on-demand-call"
    :disk-footprint-summary "19 worker files on disk; 17 spawned; 1 on-demand active (code_prefetch); 1 planned non-active (experience_harvester)"
    :legacy-count-note "老图 event-workers 写 '21 workers', 是 sonnet 6(briefing 删) + codex 2(step_narrator 删) + gemini 1 + local 12 之和. 当前实际 19 于磁盘"

    (zombie-ledger
      (code-prefetch         :lifecycle-style on-demand       :status "disk-present / runtime-called / not spawned")
      (experience-harvester  :lifecycle-style planned         :status "disk-present / not spawned / plan-or-prototype — I005 待确认")
      (briefing-worker       :lifecycle-style zombie-deleted  :status "removed from sonnet/ v1.3.0 — UPDATE 语义不兼容")
      (step-narrator         :lifecycle-style zombie-deleted  :status "removed from codex/ v0.4.23 — message_narrations 表删除")
      (event-analyzer-worker :lifecycle-style zombie-absorbed :status "65c8b59 新增 → 1ea1838 吸收进 tagger_chunker 的 commit detection"))

    ;; ── WorkerKind::Sonnet (5 spawned) ──
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
      :dual-ownership-note
        ["lisp_survey_worker: 触发 (worker pillar) + 语义 ownership (intent-layer pillar — 更新 <project>/.missiond/intent.lisp)"
         "arch_maintenance_worker: 触发 (worker pillar) + 语义 ownership (intent-layer pillar — arch manifest 文件)"]

      (path embedding-worker-loop
        :lifecycle-style spawned
        :v0.3-change "step 3 从 sonnet_gateway 改为 xjp-router-embedding (禁止 fallback)"
        (ingress
          :source "EmbeddingTask MPSC channel from ast_sync_worker / backfill path"
          :entry-components
            ["crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs"
             "crates/missiond-daemon/src/llm/xjp_router_client.rs (pending v0.3)"])
        (logic-core
          (step s1 "消费 embedding_rx / embedding_tx 通道 EmbeddingTask, 分派成 conversation/KB/AST embedding 子路径")
          (step s2 "读 conversations / ast_nodes / kb_entries / compaction_fragments / 既有 embedding 表, 决定增量 upsert 或 backfill")
          (step s3 "经 xjp-router-gateway :: xjp-router-embedding path 调 QWEN3; 失败直接上抛 (禁止 fallback)")
          (step s4 "向量写回 kb_embeddings / ast_embeddings / turn_topics, 避免重复计算")
          (step s5 "记录进度, 释放后续检索可见性"))
        (egress
          :writes ["kb_embeddings" "ast_embeddings" "turn_topics"]
          :reads ["conversations" "ast_nodes" "kb_entries" "compaction_fragments" "ast_embeddings" "kb_embeddings"]
          :via-bus ["EmbeddingTask (MPSC from ast_sync_worker)"]
          :memory-cross-ref ["kb-manager" "conversation-logs" "embedding-support"]
          :returns "embedding progress / vector upsert summary"))

      (path translation-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "MessageEvent::thinking_message"
          :entry-components
            ["crates/missiond-daemon/src/workers/sonnet/translation_worker.rs"
             "crates/missiond-daemon/src/llm/sonnet_gateway.rs"])
        (logic-core
          (step s1 "订阅 thinking_message 类 message 事件, 过滤需翻译文本")
          (step s2 "读 conversation_messages 原文 + 元数据, 生成 translation prompt")
          (step s3 "经 sonnet_gateway 获取译文")
          (step s4 "写回 message_translations"))
        (egress
          :writes ["message_translations"]
          :reads ["conversation_messages"]
          :via-bus ["MessageEvent::thinking_message"]
          :memory-cross-ref ["conversation-logs"]
          :returns "translation result"))

      (path arch-maintenance-worker-cycle
        :lifecycle-style spawned
        :trigger-history "commit 65c8b59: 由 interval 3600s git-log 轮询改为 SystemEvent::ContextualCommitDetected 订阅"
        :dual-ownership "触发 worker pillar; 语义 ownership intent-layer pillar"
        (ingress
          :source "SystemEvent::ContextualCommitDetected"
          :entry-components
            ["crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
             "crates/missiond-daemon/src/slot_orchestrator/agent.rs"
             "crates/missiond-daemon/src/slot_orchestrator/claude_code.rs"
             "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs"])
        (logic-core
          (step s1 "消费带 conversation/session/slot 上下文的 ContextualCommitDetected")
          (step s2 "commit diff + project + context → architecture maintenance prompt")
          (step s3 "SlotManager.execute('arch_maintenance', prompt) → arch-surveyor slot")
          (step s4 "slot 执行 Edit 更新 architecture manifest (归 intent-layer pillar)")
          (step s5 "worker 持调度权, 实际文件写入在 slot 执行侧"))
        (egress
          :writes []
          :reads []
          :via-bus ["SystemEvent::ContextualCommitDetected"]
          :file-writes ["~/.missiond/flows/... (indirect via SlotManager.execute)" "project architecture manifest (intent-layer ownership)"]
          :write-style "indirect via slot execution"
          :returns "slot dispatch result"))

      (path retro-worker-cycle
        :lifecycle-style spawned
        :bg-worker-upgrade "commit c8b76b0: 从 loose async fn 改为 BackgroundWorker impl, 受 ControlTree 治理"
        (ingress
          :source "SessionCompleted"
          :entry-components
            ["crates/missiond-daemon/src/workers/sonnet/retro_worker.rs"
             "crates/missiond-daemon/src/llm/sonnet_gateway.rs"])
        (logic-core
          (step s1 "订阅 SessionCompleted, 选取需复盘的 conversation window")
          (step s2 "读 conversations + 统计信息, 组装 retrospective prompt")
          (step s3 "经 sonnet_gateway 生成 session retro / deep analysis")
          (step s4 "写入 deep_analysis 与 retrospectives"))
        (egress
          :writes ["deep_analysis" "retrospectives"]
          :reads ["conversations"]
          :via-bus ["SessionCompleted"]
          :memory-cross-ref ["system-support" "conversation-logs"]
          :returns "retro analysis result"))

      (path lisp-survey-update
        :lifecycle-style spawned
        :dual-ownership "触发 worker pillar; 语义 ownership intent-layer pillar (v2 intent.lisp 已列为 intent-layer component)"
        :added "commit 79a877f"
        (ingress
          :source "SystemEvent::ContextualCommitDetected"
          :entry-components
            ["crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs"
             "crates/missiond-daemon/src/slot_orchestrator/agent.rs"
             "crates/missiond-daemon/src/slot_orchestrator/claude_code.rs"
             "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs"])
        (logic-core
          (step s1 "self-trigger 过滤: slot_id == 'lisp-surveyor' 的 commit 跳过")
          (step s2 "ProjectRegistry::resolve(project_id) → intent_path; 未配置 intent.lisp 的项目跳过")
          (step s3 "60s debounce per project_id (HashMap<String, Instant>)")
          (step s4 "diff (max 8000 chars) + intent_path → survey prompt → slot_manager.execute('lisp_survey', prompt) → lisp-surveyor slot")
          (step s5 "parse response: NO_CHANGE → skip; otherwise slot Edit 更新 intent.lisp"))
        (egress
          :writes []
          :reads []
          :via-bus ["SystemEvent::ContextualCommitDetected"]
          :file-writes ["<project>/.missiond/intent.lisp (via slot Edit tool, intent-layer ownership)"]
          :write-style "indirect via slot execution"
          :returns "survey result / intent file update"))

      (contract-summary
        :writes ["kb_embeddings" "ast_embeddings" "turn_topics" "message_translations" "deep_analysis" "retrospectives"]
        :reads  ["conversations" "conversation_messages" "ast_nodes" "kb_entries" "compaction_fragments" "ast_embeddings" "kb_embeddings"]
        :file-writes ["~/.missiond/flows/..." "<project>/.missiond/intent.lisp" "project architecture manifests"]
        :external-services ["xjp-router-gateway (embedding)" "sonnet-priority-gateway (chat)"]))

    ;; ── WorkerKind::Codex (1 spawned) ──
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
          (step s1 "订阅 vision task 事件, 定位需图像理解的 conversation message")
          (step s2 "读 conversation_messages.raw_content, 提取图片引用或附件")
          (step s3 "经 codex_cli 发起图像/多模态调用")
          (step s4 "结果写回 image_descriptions"))
        (egress
          :writes ["image_descriptions"]
          :reads ["conversation_messages"]
          :via-bus ["MessageEvent::vision tasks"]
          :memory-cross-ref ["system-support" "conversation-logs"]
          :returns "vision description result"))

      (contract-summary
        :writes ["image_descriptions"]
        :reads  ["conversation_messages"]))

    ;; ── WorkerKind::Gemini (1 spawned) ──
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
        :bg-worker-upgrade "commit c8b76b0: BackgroundWorker impl, 受 ControlTree 治理"
        :trigger-note "老图写 'interval 300s flag-gated', 实际也受 SessionCompleted 驱动"
        (ingress
          :source "SessionCompleted (+ 可选 interval)"
          :entry-components
            ["crates/missiond-daemon/src/workers/gemini/strategy_worker.rs"
             "crates/missiond-daemon/src/llm/gemini_driver.rs"
             "crates/missiond-daemon/src/llm/gemini_cli.rs"
             "crates/missiond-daemon/src/llm/gemini_client.rs"])
        (logic-core
          (step s1 "订阅 SessionCompleted, 决定是否生成 strategy analysis")
          (step s2 "读 conversations / 战略状态 kb_entries / daemon_state, 组策略 prompt")
          (step s3 "走 Gemini driver / CLI / client 路径执行策略分析")
          (step s4 "结果沉淀到 inbox_messages + kb_entries(strategic-state) + deep_analysis"))
        (egress
          :writes ["inbox_messages" "kb_entries" "deep_analysis"]
          :reads ["conversations" "kb_entries" "daemon_state"]
          :via-bus ["SessionCompleted"]
          :memory-cross-ref ["system-support" "kb-manager" "conversation-logs"]
          :returns "strategy analysis result"))

      (contract-summary
        :writes ["inbox_messages" "kb_entries" "deep_analysis"]
        :reads  ["conversations" "kb_entries" "daemon_state"]))

    ;; ── WorkerKind::Local (12 disk, 10 spawned + 1 on-demand + 1 planned) ──
    (subsection worker-local
      :kind "WorkerKind::Local"
      :provider-dep []
      :disk-count 12
      :spawned-count 10
      :on-demand-count 1
      :planned-count 1
      :active-roster
        ["ast_sync_worker" "code_prefetch" "codex_ingestion_worker" "conversation_logger"
         "conversation_organizer" "gemini_logger" "gemini_reconcile_worker" "pty_event_worker"
         "reconcile_worker" "tagger_chunker" "xjpcode_briefing_worker"]
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

      ;; ── 功能分组 (v0.3 新增, 按职责横切) ──
      (functional-groups
        (cli-ingestion
          :desc "外部 CLI state → MissionD conversations 表 的桥接层 (扫描 + 归一化, 存储 ownership 在 memory pillar)"
          :members ["conversation_logger (Claude Code JSONL, 事件驱动 WatcherEvent)"
                    "codex_ingestion_worker (Codex ~/.codex/state_5.sqlite, 10s 轮询)"
                    "gemini_reconcile_worker (Gemini ~/.gemini/tmp, 10s 轮询)"
                    "reconcile_worker (Claude JSONL gap 补偿, 10s 轮询 ~/.claude/projects)"]
          :common-path "worker 扫 → infra::ingestion_router → infra::message_handler → conversations/conversation_messages"
          :note "存储动作在 memory pillar :: conversation-logs; 这里只做扫 + 归一化")
        (认知管道
          :desc "消息落地后的 Stage2/Stage3 认知处理 — 组织会话 / 标签 / turn 提取"
          :members ["conversation_organizer (S2: compaction link + orphan parent fix)"
                    "tagger_chunker (S2 + S3: noise labels + turn extraction + commit detection)"]
          :pipeline "ConversationMessageLogged → organizer → SessionOrganized → tagger_chunker"
          :note "tagger_chunker 吸收了 EventAnalyzerWorker 的 commit detection (commit 1ea1838)")
        (observability-log
          :desc "运行态观测与审计 — LLM 请求日志"
          :members ["gemini_logger (LlmEvent 消费 → gemini_requests 审计)"])
        (code-intel
          :desc "代码索引 / 检索 / 经验探索"
          :members ["ast_sync_worker (增量 tree-sitter AST, 发 EmbeddingTask)"
                    "code_prefetch (on-demand 混合检索, 非标准 BackgroundWorker)"
                    "experience_harvester (planned 虚拟信标自动生成)"]
          :note "ast_sync → embedding_tx channel → embedding_worker (跨 group 依赖)")
        (pty-runtime-hook
          :desc "PTY 运行态事件处理 (auto-approve / slot 事件产生)"
          :members ["pty_event_worker (ManagerEvent → slot_sessions / incidents / message_labels / deep_analysis_checkpoint; 触发 learned-permissions)"]
          :cross-ref "section pty :: subsection learned-permissions :: path learned-permission-write")
        (meta-briefing
          :desc "元层 briefing / 外部文件产出"
          :members ["xjpcode_briefing_worker (~/.xjpcode/xjpcode.md 60s 原子写文件)"]
          :note "不写 DB; 读 board_tasks + incidents + projects"))

      (path ast-sync-worker-cycle
        :lifecycle-style spawned
        (ingress
          :source "增量 git diff / changed file sweep / ast_sync_rx MPSC"
          :entry-components ["crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"])
        (logic-core
          (step s1 "扫变更文件或消费 ast_sync 触发")
          (step s2 "读 ast_files 做增量判断")
          (step s3 "生成 AST + beacon nodes, upsert ast_files / ast_nodes / beacon_nodes")
          (step s4 "向量化内容组装 EmbeddingTask, 发 embedding_tx")
          (step s5 "暴露更新后的代码索引给 context / search / code_prefetch"))
        (egress
          :writes ["ast_files" "ast_nodes" "beacon_nodes"]
          :reads ["ast_files"]
          :via-bus ["EmbeddingTask → embedding_tx"]
          :memory-cross-ref ["kb-manager"]
          :returns "ast sync delta / embedding tasks"))

      (path code-prefetch-on-demand-retrieval
        :lifecycle-style on-demand
        :note "非标准 BackgroundWorker; 由 context_pipeline::execute() 直接调用"
        (ingress
          :source "context_pipeline::execute() / retrieval call sites"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/code_prefetch.rs"
             "crates/missiond-daemon/src/context/context_pipeline.rs"])
        (logic-core
          (step s1 "接收 on-demand 查询, 不经 main.rs spawn")
          (step s2 "beacon_nodes / ast_nodes 上 FTS5 或结构化代码检索 + ast_search_hits/kb_entries 补充")
          (step s3 "hybrid merge/rank, 返回 snippet + 命中解释")
          (step s4 "仅返回, 不持久化"))
        (egress
          :writes []
          :reads ["beacon_nodes" "ast_nodes" "ast_search_hits" "kb_entries"]
          :via-bus []
          :memory-cross-ref ["kb-manager"]
          :returns "hybrid code retrieval result"))

      (path codex-ingestion-worker-cycle
        :lifecycle-style spawned
        :functional-group "cli-ingestion"
        :added "commit ec269d7 + 43c80f4 扩展"
        (ingress
          :source "poll ~/.codex/state_5.sqlite via rusqlite (10s interval)"
          :entry-components ["crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"])
        (logic-core
          (step s1 "rusqlite 轮询 Codex CLI 操作历史与 JSONL-like state")
          (step s2 "parse_jsonl 抽出 tool calls + text messages (assistant + user)")
          (step s3 "upsert conversations / conversation_messages / tool_calls")
          (step s4 "对账后 session state 交下游 organizer / tagger / recall"))
        (egress
          :writes ["conversations" "conversation_messages" "tool_calls"]
          :reads []
          :via-bus []
          :memory-cross-ref ["conversation-logs"]
          :returns "codex ingestion progress"))

      (path conversation-logger-cycle
        :lifecycle-style spawned
        :functional-group "cli-ingestion"
        (ingress
          :source "WatcherEvent::NewMessages / WatcherEvent::SessionInactive (Claude Code JSONL)"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/conversation_logger.rs"
             "crates/missiond-daemon/src/events_sync.rs"
             "crates/missiond-daemon/src/infra/ingestion_router.rs"
             "crates/missiond-daemon/src/infra/message_handler.rs"])
        (logic-core
          (step s1 "watcher 信号后 tail JSONL 或 session backfill")
          (step s2 "显式穿越 infra::ingestion_router.rs 分类消息")
          (step s3 "显式穿越 infra::message_handler.rs 归一 (project_id 经 ProjectRegistry::resolve(cwd) 填充)")
          (step s4 "写 conversations / conversation_messages, 必要时创建 board_tasks 做 memory hook")
          (step s5 "session 暴露给 organizer / tagger / reconcile"))
        (egress
          :writes ["conversations" "conversation_messages" "board_tasks"]
          :reads ["conversations" "board_tasks" "compaction_fragments"]
          :via-bus ["WatcherEvent::NewMessages" "WatcherEvent::SessionInactive" "JsonlMessageIngested"]
          :memory-cross-ref ["conversation-logs" "board"]
          :returns "persisted conversation delta / memory hook submission"))

      (path conversation-organizer-cycle
        :lifecycle-style spawned
        :functional-group "认知管道"
        :fix-history "commit ad889d1: 去 agent-only 过滤; commit 43c80f4: broadcast lagged warning 语义明确"
        (ingress
          :source "MessageEvent::Logged → SessionEvent::Organized"
          :entry-components ["crates/missiond-daemon/src/workers/local/conversation_organizer.rs"])
        (logic-core
          (step s1 "消费新落地 message 的 session dirty 集合")
          (step s2 "agent session: compaction link + orphan parent fix; 非 agent: 跳过 P0/P1")
          (step s3 "5s debounce 后 emit SessionOrganized")
          (step s4 "broadcast lagged → warn (permanently lost, reconcile_tick 补)"))
        (egress
          :writes ["conversations"]
          :reads ["conversations"]
          :via-bus ["MessageEvent::Logged" "SessionEvent::Organized"]
          :memory-cross-ref ["conversation-logs"]
          :returns "organized session ids / follow-up signal"))

      (path experience-harvester-prototype
        :lifecycle-style planned
        :functional-group "code-intel"
        :status "disk-present / not spawned / I005 待确认 planned vs zombie"
        (ingress
          :source "暂无稳定 subscription; 老图写 interval 60s"
          :entry-components ["crates/missiond-daemon/src/workers/local/experience_harvester.rs"])
        (logic-core
          (step s1 "读 ast_nodes / conversations / tool_calls 寻高价值经验")
          (step s2 "候选归纳为 beacon / board follow-up")
          (step s3 "phase-A 保留骨架, phase-B I005 确认"))
        (egress
          :writes ["beacon_nodes" "board_tasks"]
          :reads ["ast_nodes" "conversations" "tool_calls"]
          :via-bus []
          :memory-cross-ref ["kb-manager" "board" "conversation-logs"]
          :returns "planned harvester result"))

      (path gemini-logger-cycle
        :lifecycle-style spawned
        :functional-group "observability-log"
        (ingress
          :source "LlmEvent::RequestStarted / LlmEvent::ResponseCompleted"
          :entry-components ["crates/missiond-daemon/src/workers/local/gemini_logger.rs"])
        (logic-core
          (step s1 "监听 Gemini 请求生命周期事件")
          (step s2 "归一记录 request / response / latency / status")
          (step s3 "写入 gemini_requests"))
        (egress
          :writes ["gemini_requests"]
          :reads []
          :via-bus ["LlmEvent::*"]
          :memory-cross-ref ["llm-support"]
          :returns "gemini request log status"))

      (path gemini-reconcile-worker-cycle
        :lifecycle-style spawned
        :functional-group "cli-ingestion"
        (ingress
          :source "poll ~/.gemini/tmp (10s interval)"
          :entry-components ["crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"])
        (logic-core
          (step s1 "扫描 ~/.gemini/tmp 与 MissionD conversation state 差异")
          (step s2 "按 reconcile_watermarks 决定继续对账位点")
          (step s3 "补写 conversations / conversation_messages, 推进 reconcile_watermarks")
          (step s4 "确保 Gemini 外部 state 与内部一致"))
        (egress
          :writes ["conversations" "conversation_messages" "reconcile_watermarks"]
          :reads ["reconcile_watermarks" "conversations"]
          :via-bus []
          :memory-cross-ref ["conversation-logs" "system-support"]
          :returns "gemini reconcile progress"))

      (path pty-event-worker-cycle
        :lifecycle-style spawned
        :functional-group "pty-runtime-hook"
        :auto-approve-feature "commit ec269d7: auto-approve dialog containing 'don't ask again'/'always'/'trust'/'不再'"
        (ingress
          :source "ManagerEvent::TextComplete / Exited / StateChange / ConfirmRequired"
          :entry-components
            ["crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
             "crates/missiond-daemon/src/infra/session_util.rs"
             "crates/missiond-daemon/src/permission_extract.rs"])
        (logic-core
          (step s1 "消费 PTY runtime 广播, 识别 idle/stuck/confirm-required 状态")
          (step s2 "显式穿越 infra::session_util.rs 解析 session UUID / project_id / slot 归属")
          (step s3 "ConfirmRequired: 触发 learned-permissions (subsection) write flow — extract_confirm + learn + PTY Option(2)")
          (step s4 "更新 conversations / slot_sessions / message_labels / incidents / deep_analysis_checkpoint")
          (step s5 "发射 SlotBecameIdle / SlotStuck 驱动上层 orchestration"))
        (egress
          :writes ["conversations" "slot_sessions" "deep_analysis_checkpoint" "message_labels" "incidents"]
          :reads ["conversations" "slot_sessions" "board_tasks"]
          :via-bus ["ManagerEvent::TextComplete" "ManagerEvent::Exited" "ManagerEvent::StateChange" "ManagerEvent::ConfirmRequired" "SlotBecameIdle" "SlotStuck"]
          :file-writes ["learned_permissions.yaml (via learned-permission-write path)"]
          :memory-cross-ref ["conversation-logs" "slot-support" "system-support" "board"]
          :returns "slot runtime side effects / slot events"))

      (path reconcile-worker-cycle
        :lifecycle-style spawned
        :functional-group "cli-ingestion"
        :fix-history "commit 0adbb18: ensure_conversation_exists 传入 jsonl 首条消息时间戳作为 started_at"
        (ingress
          :source "periodic poll ~/.claude/projects (10s interval)"
          :entry-components ["crates/missiond-daemon/src/workers/local/reconcile_worker.rs"])
        (logic-core
          (step s1 "扫 Claude 项目目录 JSONL 与已持久化状态的 gap")
          (step s2 "按 reconcile_watermarks 决定补入区间")
          (step s3 "补写 conversation_messages + 推进 reconcile_watermarks")
          (step s4 "不依赖 event bus, sweep 形式最终一致"))
        (egress
          :writes ["conversation_messages" "reconcile_watermarks"]
          :reads ["reconcile_watermarks"]
          :via-bus []
          :memory-cross-ref ["conversation-logs" "system-support"]
          :returns "reconcile progress"))

      (path tagger-chunker-cycle
        :lifecycle-style spawned
        :functional-group "认知管道"
        :absorbed-features "commit 1ea1838: 吸收 EventAnalyzerWorker 的 commit detection"
        :reconcile-tick "commit 43c80f4: 新增 reconcile_tick(60s) 补处理 broadcast Lagged 漏失的 session"
        (ingress
          :source "SessionEvent::Organized (+ 60s reconcile_tick 兜底)"
          :entry-components ["crates/missiond-daemon/src/workers/local/tagger_chunker.rs"])
        (logic-core
          (step s1 "analyze_messages(stage2): noise labels + tool 分类 + commit detection (早于 early-return)")
          (step s2 "chunk_turns(stage3): turn 提取 + tail-trim + 持久化 (活跃 session 可 0 副作用)")
          (step s3 "message_labels + turns 从 conversation_messages 生成")
          (step s4 "60s reconcile 调 sessions_recently_active_without_turns (since_minutes=5, limit=50) 补漏"))
        (egress
          :writes ["message_labels" "turns"]
          :reads ["conversation_messages"]
          :via-bus ["SessionEvent::Organized" "ContextualCommitDetected (emit)"]
          :memory-cross-ref ["conversation-logs"]
          :returns "tagging / chunking result / commit events"))

      (path xjpcode-briefing-worker-cycle
        :lifecycle-style spawned
        :functional-group "meta-briefing"
        :added "commit 8e5efaf"
        (ingress
          :source "interval 60s (15s initial delay, MissedTickBehavior::Skip)"
          :entry-components ["crates/missiond-daemon/src/workers/local/xjpcode_briefing_worker.rs"])
        (logic-core
          (step s1 "读 board_tasks(running+failed) + incidents(5) + projects")
          (step s2 "汇总成 xjpcode CLI 启动可直接消费的 briefing markdown")
          (step s3 "原子 std::fs::write 到 ~/.xjpcode/xjpcode.md (非 DB)")
          (step s4 "受 ControlTree pause/resume 治理"))
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
           "conversations" "conversation_messages" "board_tasks" "tool_calls"
           "gemini_requests" "reconcile_watermarks"
           "slot_sessions" "deep_analysis_checkpoint" "message_labels" "incidents" "turns"]
        :reads
          ["ast_files" "beacon_nodes" "ast_nodes" "ast_search_hits" "kb_entries"
           "conversations" "board_tasks" "compaction_fragments" "tool_calls"
           "reconcile_watermarks" "slot_sessions" "conversation_messages"
           "incidents" "projects"]
        :file-writes ["~/.xjpcode/xjpcode.md" "learned_permissions.yaml"])))

  ;; ══════════════════════════════════════════════════════════
  ;; 2.6 Engine Cluster — 运行时机制留 worker; 学习/规划归 intent-layer
  ;; ══════════════════════════════════════════════════════════
  (section engine-cluster
    :relationship-to-worker-cluster "worker-cluster = 被治理的计算租户; engine-cluster = 运行时机制 (timer/DAG/dispatch)"
    :v0.3-boundary-shift
      ["intent-engine::board-phase-engine (flow_engine v1, project lifecycle phases) → 迁 intent-layer pillar"
       "learning-engine 全家 7 sub-engine → primary-ownership intent-layer pillar, 本 section 只留触发骨架"]

    (subsection intent-engine
      :desc "执行调度内核 — timer / queue / workflow runtime"
      :targets
        ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
         "crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs"
         "crates/missiond-daemon/src/engine/intent_engine/workflow_executor.rs"
         "crates/missiond-daemon/src/engine/intent_engine/gen_engine.rs (Forge shell)"]
      :migrated-out "flow_engine.rs (v1) → intent-layer pillar (project-lifecycle phases)"

      (path autopilot-tick
        :lifecycle-style spawned
        :depends ["db" "slot_manager" "event_bus" "llm_gateway" "context_pipeline"]
        (ingress
          :source "autopilot timer (60s, main.rs:1076-1096)"
          :entry-components ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"])
        (logic-core
          :pipeline
            [(stage-1 "memory-scheduler — 扫 pending / reminder / queue state")
             (stage-2 "extraction-check — 检查 extraction/learning 相关执行态 (虽然逻辑归 intent-layer)")
             (stage-3 "board-task-dispatch — list open tasks → CAS atomic claim → 选 slot/worker")
             (stage-4 "flow-progression — 推进 board_tasks 上挂的 flow 状态")
             (stage-5 "supervision-check — lease recovery / stale task / zombie slot 回收")])
        (egress
          :writes ["board_tasks" "prompt_snapshots"]
          :reads ["board_tasks"]
          :via-bus ["BoardEvent::*" "WorkerEvent::*"]
          :memory-cross-ref ["board"]
          :returns "tick summary / dispatch actions"))

      (path memory-scheduler-queue
        :lifecycle-style spawned
        :depends ["db"]
        (ingress
          :source "autopilot tick / internal memory work queue"
          :entry-components ["crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs"])
        (logic-core
          (step s1 "扫待执行 memory-side 任务 + reminder state")
          (step s2 "决定推进 / 延后 / 交 worker / slot")
          (step s3 "候选回 autopilot / workflow, 偏调度不偏 ownership"))
        (egress
          :writes []
          :reads ["board_tasks"]
          :via-bus []
          :memory-cross-ref ["board"]
          :returns "scheduled memory work candidates"
          :need-more-ground-truth "phase-B I002: 精确写路径待 agent scan"))

      (path workflow-executor-runtime
        :lifecycle-style on-demand
        :depends ["db" "slot_manager"]
        (ingress
          :source "skill workflow step / engine-side tool dispatch / mission_task_*"
          :entry-components ["crates/missiond-daemon/src/engine/intent_engine/workflow_executor.rs"])
        (logic-core
          (step s1 "接收 workflow step 定义 / skill execution 请求 / task_submit")
          (step s2 "按步骤调 MCP 工具 / slot / runtime primitive")
          (step s3 "结果回上层 engine / handler"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "workflow step result"
          :need-more-ground-truth "phase-B I002: workflow_executor 精确表交互")))

    (subsection flow-engine-v2
      :desc "General-purpose declarative YAML → node-sequence 执行器 (与 v1 并存)"
      :added "commit 49bd316 (2026-04-14)"
      :distinguish-from-v1
        ["flow-engine v1 (flow_engine.rs) — project-lifecycle phases, autopilot-driven, 归 intent-layer pillar"
         "flow-engine v2 (engine/flow/) — YAML declarative, general-purpose, runtime 留 worker pillar"]
      :targets
        ["crates/missiond-daemon/src/engine/flow/mod.rs (types)"
         "crates/missiond-daemon/src/engine/flow/loader.rs (YAML 加载)"
         "crates/missiond-daemon/src/engine/flow/runner.rs (DAG 执行)"
         "crates/missiond-daemon/src/engine/flow/handlers.rs (5 node-type dispatch)"
         "crates/missiond-daemon/src/engine/flow/examples/ (样例)"
         "crates/missiond-daemon/src/engine/flow/gen_engine.rs (Forge shell)"]
      :db-reuse "board_tasks 表的 flow_template / flow_phase / flow_context 三列 (v2 不加新表)"

      (node-types
        (LlmCall           :fields (provider prompt max_tokens=65536)
          :dispatch "gemini → llm_gateway::call_gemini_for_flow; sonnet → call_sonnet_stateless('architecture reviewer')")
        (SlotTask          :fields (model=opus prompt timeout_secs=3600)
          :selection "list_slots first non-excluded; EXCLUDED_ROLES=[memory, supervisor, strategy]"
          :precondition "slot must be Running (SessionState != Exited) else fail-fast")
        (McpTool           :fields (tool_name params)
          :dispatch "handlers::dispatch_tool(state, tool_name, params)")
        (DaemonAction      :fields (action params)
          :dispatch "read_intent_lisp → dispatch_tool('mission_intent'); close_flow → update_board_task(status='done')")
        (ParallelSlotTasks :fields (parallelism=3 "tasks:Vec<ParallelTaskSpec>" gather=Aggregate timeout_secs=1800)
          :added "49bd316"
          :selection "list_slots → non-excluded + Running"
          :parallelism "effective = min(parallelism, candidates.len(), tasks.len()).max(1) — 防 slot 饥饿死锁"
          :dispatch "JoinSet + Arc<Semaphore>(effective) → round-robin (idx % candidates.len())"
          :send "fire-and-forget POC; Phase 2: tokio::time::timeout + SlotBecameIdle reflow"
          :gather "Aggregate→json array / AllSuccess→fail if any None / AnySuccess→fail only if all fail"))

      (types
        (ParallelTaskSpec :fields (id prompt))
        (GatherStrategy :variants (Aggregate AllSuccess AnySuccess) :default Aggregate)
        (ErrorPolicy :variants (Stop Skip "Retry(u32)") :default Stop)
        (FlowNode :fields (id node_type save_as depends_on on_error))
        (FlowDefinition :fields (id name nodes))
        (FlowContext :fields (vars:HashMap<String,String> current_node completed_nodes last_error)
          :methods ("resolve_vars: template ${key} interpolation" "set/get")))

      (fail-fast-invariants
        :count 7
        :list
          ["Slot not running → immediate err (no auto-spawn)"
           "Unknown LLM provider → immediate err (no fallback)"
           "Unknown daemon action → immediate err (no noop)"
           "Flow YAML not found → immediate err"
           "ParallelSlotTasks empty tasks → immediate err"
           "ParallelSlotTasks no running non-excluded slots → immediate err"
           "Async recursion cycle (run_flow→execute_node→dispatch_tool→flow_run::handle→run_flow) broken via Box::pin"])

      (path flow-definition-load
        :lifecycle-style on-demand
        (ingress
          :source "mission_flow_run(action=list|status|run) / board task with flow_template"
          :entry-components ["crates/missiond-daemon/src/engine/flow/loader.rs"])
        (logic-core
          (step s1 "load_flow: $MISSIOND_HOME/flows/{flow_id}.yaml → serde_yaml::from_str::<FlowDefinition>")
          (step s2 "list_flows: scan $MISSIOND_HOME/flows/*.{yaml,yml}")
          (step s3 "验证 YAML 能 parse 成 node-based flow 定义")
          (step s4 "交 runner 或 list/status 返回元数据"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "FlowDefinition / available flows"))

      (path flow-node-handler-dispatch
        :lifecycle-style on-demand
        (ingress
          :source "runner 逐节点执行到具体 node"
          :entry-components ["crates/missiond-daemon/src/engine/flow/handlers.rs"])
        (logic-core
          (step s1 "按 NodeType 分派: LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks")
          (step s2 "每 type 按 node-types spec 执行 (见上)")
          (step s3 "结果归 runner, 由后者 save_as → ctx.vars"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "per-node execution result"))

      (path flow-runner-persist
        :lifecycle-style on-demand
        (ingress
          :source "FlowDefinition 加载完成, runner 开始"
          :entry-components ["crates/missiond-daemon/src/engine/flow/runner.rs"])
        (logic-core
          (step s1 "iterate flow.nodes skip(ctx.current_node), execute_with_retry")
          (step s2 "save_as → ctx.vars; completed_nodes.push; persist_context on each node")
          (step s3 "execute_with_retry: Retry(N) → 2^attempt secs exp backoff; Skip → warn + Ok(empty); Stop → propagate err")
          (step s4 "persist_context 调 store.update_board_task(task_id, flow_context: Some(json))")
          (step s5 "flow 完成后上层 handler 更新 flow_phase=completed / status=done; 失败 flow_phase=failed / status=failed"))
        (egress
          :writes ["board_tasks.flow_context" "board_tasks.flow_phase" "board_tasks.status"]
          :reads ["board_tasks"]
          :via-bus []
          :memory-cross-ref ["board"]
          :returns "flow runtime result / persisted context"))

      (mcp-tool mission_flow_run
        :target "crates/missiond-mcp/src/tools/compute/flow_run.rs"
        :description "Flow Engine v2: execute declarative node-based workflows"
        :required (flow_id)
        :properties (flow_id params "action[run|list|status]=run" task_id)))

    (subsection learning-engine
      :desc "学习/决策/分析 sub-engines — primary-ownership 搬 intent-layer pillar (待 intent-layer phase-A iteration)"
      :primary-ownership "intent-layer pillar (待迁)"
      :remain-here "BackgroundWorker 触发机制 + 代码文件引用占位; 详细语义 ownership 不在本 section"
      :targets
        ["crates/missiond-daemon/src/engine/learning_engine/mod.rs"
         "crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs"
         "crates/missiond-daemon/src/engine/learning_engine/decision_harvest.rs"
         "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
         "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"
         "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"
         "crates/missiond-daemon/src/engine/learning_engine/intent_analyst.rs"
         "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"
         "crates/missiond-daemon/src/engine/learning_engine/gen_engine.rs (Forge shell)"]
      :sub-engines-brief
        [(decision-engine     :role "decision-cascade: kb-lookup → gemini-consult → decision-slot → human-escalation")
         (decision-harvest    :role "决策泛化 / 模式归纳")
         (extraction          :role "事件→知识提取 (two-phase: 快速 + 深度); extraction-phase FSM 归 intent-layer")
         (historical-scanner  :role "回溯会话扫描")
         (idle-explorer       :role "空闲期探索触发")
         (intent-analyst      :role "意图分析; **唯一明确写表**: user_intents + conversation_turns.intent_group_id (memory :: conversation-logs ownership)")
         (timeline-analyst    :role "时间轴分析")]

      (intent-analyst-concrete-contract
        :reason "仅此 sub-engine 在 memory pillar v0.5.1 frozen 里有明确 writer 声明 (ConversationStore::insert_user_intent)"
        :path "intent-layer pillar 具体 path (待 phase-A)"
        :cross-ref "intent-memory.lisp :: module conversation-logs :: binds-to intent_analyst")

      (note "其他 6 sub-engine 的精确表级契约是 phase-B I003 的探索目标 — 但结果应回填 intent-layer pillar 而非 worker pillar")))

  ;; ══════════════════════════════════════════════════════════
  ;; 2.7 Orchestration & Governance (ControlTree + bootstrap)
  ;; ══════════════════════════════════════════════════════════
  (section orchestration-governance
    :desc "worker registry + ControlTree 6 层治理 + daemon bootstrap 6 phase 依赖图"
    :targets
      ["crates/missiond-daemon/src/workers/registry.rs"
       "crates/missiond-daemon/src/control_tree.rs"
       "crates/missiond-daemon/src/main.rs"
       "crates/missiond-daemon/src/state.rs"
       "crates/missiond-daemon/src/supervisor.rs"
       "crates/missiond-daemon/src/event_bus.rs"
       "crates/missiond-daemon/src/event_router.rs (demoted to signal emitter)"]

    (path worker-kind-registration
      :lifecycle-style bootstrap
      (ingress
        :source "daemon bootstrap Phase 5"
        :entry-components ["crates/missiond-daemon/src/workers/registry.rs"])
      (logic-core
        (step s1 "BackgroundWorker trait: const KIND:WorkerKind + methods(name/extra_deps/run)")
        (step s2 "KIND 必须匹配子目录 — 'Directory structure is the contract' 硬不变量")
        (step s3 "spawn_worker 按 KIND 自动注入 provider dependency (Dependency::Provider(Sonnet/Codex/Gemini))")
        (step s4 "注册表产 WorkerHandle / WorkerInfo / WorkerContext, 供 runtime + tools 查询")
        (step s5 "所有 worker lifecycle 经此纳入治理, 无裸 tokio::spawn"))
      (egress
        :writes []
        :reads []
        :via-bus ["WorkerStatusChanged"]
        :returns "worker registry / handles / provider dependency injection"))

    (path pause-resume-cascade
      :lifecycle-style "runtime-control"
      :struct-authority "crates/missiond-daemon/src/control_tree.rs ControlTree"
      (ingress
        :source "mission_worker (MCP) / control mutation / debug override / bootstrap restore"
        :entry-components ["crates/missiond-daemon/src/control_tree.rs"])
      (logic-core
        (step s1 "ControlTree 6 字段: global_paused / providers / domains / workers / slot_roles / projects + domain_paused_at(informational)"))

      (struct ControlTree
        (field global_paused :type bool)
        (field providers     :type "HashMap<CtlProvider, bool>")
        (field domains       :type "HashMap<CtlDomain, bool>")
        (field workers       :type "HashMap<String, bool>"
          :tri-state "true=force-paused / false=force-resumed(debug override) / absent=follow cascade")
        (field slot_roles    :type "HashMap<String, bool>")
        (field projects      :type "HashMap<String, bool>"
          :added "commit 50a5296 (P2+P3)"
          :semantics "is_project_paused(id) 独立分支, NOT in is_effectively_paused worker cascade — project 控 data flow 而非 worker")
        (field domain_paused_at :type "HashMap<CtlDomain, i64>" :note "informational only"))

      (cascade-priority
        (1 worker-explicit-override
          :semantics "workers[name]=true → 总 paused; workers[name]=false → 总 resumed (debug)")
        (2 global-kill-switch
          :semantics "global_paused=true → 所有 worker paused 除非 worker override")
        (3 provider-domain-cascade
          :semantics "每个 Dependency::Provider / Dependency::Domain 检查; 任一 true → paused"))

      (methods
        (is_worker_paused :semantics "worker override > global; absent → follow global_paused")
        (is_effectively_paused :args "(worker_name, deps: &[Dependency])"
          :semantics "full cascade: worker override > global > provider/domain deps")
        (status_summary :returns "serde_json::Value"
          :fields "global_paused, providers, domains, workers_paused, workers_force_resumed, slot_roles"))

      (struct ControlManager
        :pattern "push-based watch broadcast (NOT polling)"
        :transport "tokio::watch::channel<ControlTree>"
        :semantics "mutations → atomic update via send_modify() → all subscribers notified via changed().await"
        :persistence "control_tree.json — crash recovery via spawn_blocking write"
        :zero-cost "worker await watch::Receiver::changed() in select! — 零 HashMap 轮询"
        :mutations "set_global_paused / set_provider / set_domain / set_worker / set_slot_role / set_project"
        :note "set_project paused=true → insert; paused=false → remove (no false entry)")

      (egress
        :writes []
        :reads []
        :via-bus ["WorkerStatusChanged"]
        :file-writes ["control_tree.json"]
        :returns "pause/resume status summary"))

    (path daemon-bootstrap-spawn-order
      :lifecycle-style bootstrap
      :authority "intent-pillar-transport-bootstrap.lisp :: daemon-init + v2 intent.lisp :: system-layer :: bootstrap"
      (ingress
        :source "daemon startup (binary main)"
        :entry-components
          ["crates/missiond-daemon/src/main.rs"
           "crates/missiond-daemon/src/state.rs"])
      (logic-core
        :phases
          [(Phase-1 "Infrastructure: db pool → embed_model → event_bus")
           (Phase-1.5 "ProjectRegistry: store.list_projects() → ProjectRegistry::new(projects) → SharedProjectRegistry (commit e18d0bf, 需早于 slot_manager)")
           (Phase-2 "Core modules: pty_manager → slot_manager → mission_control")
           (Phase-3 "Gateways: gemini_gateway → sonnet_gateway → llm_gateway (pending: xjp_router_client)")
           (Phase-4 "Pipelines: context_pipeline → worker_registry → control_tree")
           (Phase-5 "Workers: 17 BackgroundWorker spawn (见 workers/ 子目录)")
           (Phase-6 "Engines & IO: autopilot → ipc-handler → ws-server")])

      (depends-graph
        (project_registry (db)
          :note "store.list_projects() → ProjectRegistry::new(projects) → SharedProjectRegistry")
        (pty_manager     (event_bus))
        (slot_manager    (db pty_manager event_bus))
        (mission_control (db slot_manager event_bus))
        (gemini_gateway  (db))
        (sonnet_gateway  (slot_manager))
        (llm_gateway     (gemini_gateway sonnet_gateway))
        (context_pipeline (db slot_manager))
        (autopilot       (db slot_manager event_bus llm_gateway context_pipeline)))

      (app-state
        :target "crates/missiond-daemon/src/state.rs"
        :fields "db pool / event_bus / slot_manager / llm_gateway / context_pipeline / project_registry / 4 MPSC senders"
        :invariant "只读访问 (RwLock read only); 启动后不再 write — 状态权威在 DB + event_bus"
        :added-field "project_registry: SharedProjectRegistry (commit e18d0bf) — path→project_id 解析 + 项目元数据缓存")

      (supervisor
        :target "crates/missiond-daemon/src/supervisor.rs"
        :role "worker 健康监控 + 重启")

      (invariant "每 phase 依赖前一 phase; ProjectRegistry 必须早于 message_handler; event_bus 必须早于任何 handler (防事件丢失)")
      (egress
        :writes []
        :reads ["projects"]
        :via-bus []
        :memory-cross-ref ["project-management"]
        :returns "booted runtime topology"
        :need-more-ground-truth "worker 数 / spawn 位点随 main.rs 演化, phase-B 需再扫")))

  ;; ══════════════════════════════════════════════════════════
  ;; 2.8 Worker-side Computation (retrieval + forge)
  ;; ══════════════════════════════════════════════════════════
  (section worker-side-computation
    :desc "计算型 path — 检索融合 + forge 冲压桥"

    (path retrieval-fusion
      :lifecycle-style on-demand
      (ingress
        :source "mission_kb_search / mission_memory / mission_code_search / context assembly"
        :entry-components
          ["crates/missiond-daemon/src/context/context_pipeline.rs"
           "crates/missiond-daemon/src/handlers/knowledge/kb.rs"
           "crates/missiond-daemon/src/workers/local/code_prefetch.rs"])
      (logic-core
        (step s1 "并发 4 路检索: vector-hnsw / fulltext-gin / fuzzy-trigram / tag-exact")
        (step s2 "project_id OR NULL 作用域过滤")
        (step s3 "code_prefetch (代码) + KB + history 做融合打分")
        (step s4 "返回 ranked candidates + snippet + source trace; 不 durable write"))
      (egress
        :writes []
        :reads ["kb_entries" "ast_nodes" "beacon_nodes" "ast_search_hits" "conversations" "conversation_messages"]
        :via-bus []
        :memory-cross-ref ["kb-manager" "conversation-logs"]
        :returns "ranked retrieval results"
        :need-more-ground-truth "phase-B I004: fusion ranker 真实实现文件待 grep 确认"))

    (path forge-build-bridge
      :lifecycle-style on-demand
      :boundary-note "本 path 只是 missiond 侧的 shell out bridge; forge 冲压器本体 (lisp→IR→rust) 归 intent-layer pillar"
      (ingress
        :source "mission_forge_build / mission_forge_lint"
        :entry-components
          ["crates/missiond-daemon/src/handlers/compute/forge.rs"
           "external jarvis-forge CLI"])
      (logic-core
        (step s1 "ProjectRegistry 查 project_id → 项目根目录")
        (step s2 "shell out 'forge build <root>' 或 'forge lint <root>' (FORGE_BIN env override)")
        (step s3 "捕获 stdout / stderr / exit_code / violations_raw")
        (step s4 "结果作 compute tool response 返回; 必要时触发 survey / intent-layer follow-up"))
      (egress
        :writes []
        :reads ["projects"]
        :via-bus []
        :memory-cross-ref ["project-management"]
        :returns "forge execution result")))

  ;; ══════════════════════════════════════════════════════════
  ;; need-more-ground-truth (phase-B I001-I005 + 新发现)
  ;; ══════════════════════════════════════════════════════════
  (need-more-ground-truth
    (I001 "slot_manager/ 残留引用 — grep 'slot_manager' crates/ 看是否还有 mod/use 引用需清理")
    (I002 "workflow_executor.rs 精确表级 R/W")
    (I003 "learning-engine 7 sub-engine 除 intent_analyst 外的 precise table contract (归 intent-layer pillar 后考察)")
    (I004 "retrieval-fusion 的 fusion ranker 真实文件 (是否有专门实现?)")
    (I005 "experience_harvester.rs 去留 — planned 功能还是将删 prototype")
    (I006 "xjp-router HTTP client 实际落点 (crates/missiond-daemon/src/llm/xjp_router_client.rs? 或别处?) — v0.3 声明 pending, phase-C 施工确定")
    (I007 "intent-layer pillar phase-A 后, board-phase-engine + learning-engine 7 sub 正式迁离 worker pillar 的清理动作 (何时执行 / 谁执行)")
    (I008 "flow-engine v1 (flow_engine.rs) 当前 project-lifecycle phases 具体执行逻辑 — intent-layer pillar phase-A 需吸收")
    (I009 "Global compute_slot vs per-role slot_orchestrator 并发语义 (EXCLUDED_ROLES + persistent Mutex + ephemeral 信号量) 完整 FSM 是否需要独立 pillar 文档"))
)
