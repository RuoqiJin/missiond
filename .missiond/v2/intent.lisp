;; ══════════════════════════════════════════════════════
;; MissionD — Intent v2
;; 按指挥官心智模型收拢: 七大板块
;;   一 · 记忆          — 系统记得什么 (库: schema + 状态)
;;   二 · worker        — PTY + LLM 接入 + 21 后台 worker + 编排 + engine (计算)
;;   三 · 工具          — 对外暴露的能力
;;   四 · 事件总线      — 进程内神经网络(入点 / 核心 / 出点)
;;   五 · 意图层        — 系统的自我描述 + 全局指令 + 动作/指令规约
;;   六 · 系统层        — 类型 + 传输 + RPC Gateway + 工具
;;   七 · 流程 (flow)   — 跨 pillar 的动作前后流程: memory 静态 + worker 计算 的编排
;; ══════════════════════════════════════════════════════

(intent missiond-v2
  (version "v2-draft-recursive-standard")
  (granularity L2-Topology)
  (created "2026-04-19")
  (parent "intent.lisp (v1, 27 个分文件)")
  (note "v2 按概念重组,不按物理代码层。v1 保留为历史参考。")

  ;; ── 2026-04-21 系统级导航资产 (开 pillar refactor 前必读) ──
  (navigation-assets
    (source-of-truth-index "intent-pillar-source-index.lisp"
      :desc "判真索引 — 哪个旧图代表哪 pillar 的代码真相 (gptpro 2026-04-21 产出); v0.2 (2026-04-26) 增 stable section-id registry: 7 pillar baseline + status taxonomy + implements 路径, 主 Lisp 后续压缩/拆分的 cross-ref 锚点都走这里")
    (drift-audit "drift-audit-2026-04-21.md"
      :desc "跨 pillar 代码 snapshot — worker/engine/infra footprint + bootstrap count + zombie + 跨 pillar 表 caller 精确数字")
    (refactor-methodology ".missiond/workflows/pillar-refactor.lisp"
      :desc "memory pillar 实战凝结方法论 — 5 phase × 原则 × anti-patterns × checklist")
    (architecture-dsl "architecture-dsl.lisp"
      :desc "可复用架构 DSL: pillar/function/flow/tool 的 ingress → logic-core → egress 结构与检查规则; v0.3 (2026-04-26) 加 execution dual-plane handoff rule (control-plane + durability-plane) 与 scoped-commit-subset; 主 Lisp 暂不压缩, 先建索引和 checker 约定")
    (precompression-note
      :desc "本次 wave 11 决议: 主 Lisp 不压缩 — 等 file-first writer + review gate + PLAN DAG 最小闭环稳定后, 才按 compression-policy.allowed 批量压缩状态文本; 物理 split shard 是再下一步; 详见 architecture-dsl.lisp :: judgement-now / intent-pillar-source-index.lisp :: judgement-now")
    (plan-dag-scheduler-design
      :desc "wave 11 D 组: 完整 PLAN DAG scheduler Lisp 架构 (architecture-designed) — 当前 plan-runner v0 单节点 dispatch 已 code-aligned, v1 多节点 DAG 协议落地 anchor"
      :flow-anchor ".missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s6 execution-runner :: dag-scheduler (11-stage logic-core + node schema + node FSM + claim-lease + anti-patterns + open-questions)"
      :actor-anchor ".missiond/v2/intent-intent-layer.lisp :: section action-instruction-actor :: actor plan-dag-scheduler"
      :evidence-anchor ".missiond/v2/intent-memory.lisp :: module directive-layer :: file-first-artifacts :: artifact plan-node-state-projection"
      :worker-cross-ref ".missiond/v2/intent-worker.lisp :: section claudecode-workstation-orchestration :: dispatch-decision-matrix + execution-strategy-record"
      :coordination-protocol "复用 memory pillar :: module board :: helper agent-execution-coordination (id-counters / claims-with-lease / audit-repair) — 不自建 ID 池, D010 教训"
      :status architecture-designed))

  ;; ── v2 递归同构标准: 原子 / 分子 / pillar ──
  (recursive-architecture-standard
    :goal "所有 pillar 都按 ingress → logic-core → egress 描述; logic-core 内继续按功能递归展开"
    :shape
      ((pillar "pillar-ingress → pillar-core → pillar-egress")
      (function "ingress → logic-core(step s1/s2/...) → egress")
      (step "ordered action with owner pillar + reads/writes/emits/returns")
      (tool "schema ingress → dispatch logic-core → ToolResult/audit egress")
      (flow "trigger/state ingress → ordered cross-pillar steps → writes/emits/returns/downstream egress"))
    :ownership-rules
      ["memory owns durable schema/state"
       "event-bus owns append/subscribe/persistence log"
       "tools owns external endpoint schema/routing/audit"
       "worker owns runtime mechanics/execution"
       "intent-layer owns prescription/reasoning/self-description"
       "system-layer owns type/process/transport/RPC/pure runtime substrate"
       "flow owns cross-pillar choreography narrative"]
    :editing-rule "后续梳理任何功能时, 先定位所属 pillar, 再按 ingress/core/egress 下钻到 step")


  ;; ═══════════════════════════════════════════════════
  ;;  一 · 记忆 (Memory)
  ;;  系统的长期记忆 — 详见独立 lisp
  ;; ═══════════════════════════════════════════════════
  ;; 详细规格在 intent-memory.lisp (草稿),本处只作导航摘要
  (pillar memory
    :file ".missiond/v2/intent-memory.lisp"
    :status "v0.5.8 — 9 modules + directive artifacts + agent-execution contract upgraded to dual-plane handoff (execution Lisp control plane + scoped git commit durability plane) + capability-usage-read-model semantic evidence v1 + directive-layer actor v0 all code-aligned partial; plan-runner v1 (full PLAN DAG scheduler 复用 agent-execution-coordination claim/lease 协议) architecture-designed pending; scoped commit daemon enforce / file-first .lisp writer / 完整 PLAN DAG 代码同构仍 pending"
    :paradigm "4 mature modules (project-management / board / kb-manager / conversation-logs) 自治 + 系统支持 + 横切"

    (purpose "系统长期记忆: 4 个业务模块自治管理自己的表 + 底层系统支持层 + 横切")
    (storage "PostgreSQL via sqlx::PgPool")
    (gateway "crates/missiond-core/src/db/ — 唯一 DB 入口")

    (migrated-out
      "embedding-provider → pillar 二 worker :: xjp-router-gateway (qwen3 独立 provider, code-aligned for embedding)"
      "gen-crud (Forge 冲压) → pillar 二 2.5 code-generation"
      "search-engines → pillar 二 2.6 search-engines (搜索是计算不是数据)"
      "event-bus 4 表 → pillar 四 §4.6 persistence-layer (event_log / subscriptions / blob_storage / dlq)")

    ;; ── 结构 (9 module: 5 business + 4 support + 横切) + 5 surface (v0.4.18 pillar-interfaces) ──
    (structure
      ;; ── 5 Business Modules — 各自 in/core/out + 显式 module-tables-owned ──
      (module project-management
        :desc   "项目作用域: 注册 + per-project 代码快照 intent.lisp 文件 + skills"
        :target "intent-memory.lisp :: module project-management"
        :owned-tables 5
        :v0.4.4-change "specs 4 表 (intent/plan/workflow/user_intents) 迁到 pillar 五 action-instruction-specs"
        :v0.4.16-correction "user_intents 实际从未迁出, 仍在 conversation-logs (trait=ConversationStore)"
        :v0.4.17-change "intent/plan/workflow 3 张从 pillar 五 回归 memory 新建 module directive-layer; v0.4.25 校正为 store-ready actor-pending"
        :v0.4.19-rename "命名去歧义: DB 表 intent → directive; module intent-layer → directive-layer; 避和 <project>/.missiond/intent.lisp (代码画像) 混淆"
        :mcp    "mission_project / mission_intent (只读 FILE) / mission_skill_*")

      (module board
        :desc   "任务队列: 27 列 7 态 FSM + autopilot + flow + agent_questions + prompt_snapshots"
        :target "intent-memory.lisp :: module board"
        :owned-tables 4
        :mcp    "mission_board_* (8 个) + mission_question")

      (module kb-manager
        :desc   "知识库: 语义记忆 + 代码索引 (ast/beacons) + 访问审计 + KB↔AST 链接"
        :target "intent-memory.lisp :: module kb-manager"
        :owned-tables 9
        :mcp    "mission_kb_* / mission_insight / mission_memory / mission_code_search / mission_universe_graph")

      (module conversation-logs
        :desc   "三引擎(Claude Code/Gemini/Codex)会话记录 + 派生分析 + user_intents"
        :target "intent-memory.lisp :: module conversation-logs"
        :owned-tables 15
        :v0.4.16-change "+1 user_intents 校正回归 (writer=intent_analyst, trait=ConversationStore)"
        :non-db-source "PTY JSONL (~/.claude/projects/{encoded}/*.jsonl)"
        :mcp    "mission_conversation_* / mission_retrospective_manage / mission_audit / mission_llm_trace")

      (module directive-layer
        :desc   "user utterance → lisp 指令编译 pipeline (directive → plan → workflow 三段式)"
        :target "intent-memory.lisp :: module directive-layer"
        :owned-tables 3
        :status "store+manager code-aligned partial (DirectiveLayerStore + Pg impl + mission_directive/plan/workflow read/control/draft surfaces exist; actors pending)"
        :future-writer "pillar 五 actor preferred: directive-compiler / plan-compiler / workflow-distiller; MCP tools are manager/read/control surface"
        :mcp    "mission_directive / mission_plan / mission_workflow (code-aligned partial)")

      ;; ── 4 Support Modules (v0.4.13-15 从 category system-support 分化 + v0.4.21 新增 embedding) ──
      (module llm-support
        :desc   "LLM 调用观测 — 请求日志 + 文件上传 + token 成本"
        :target "intent-memory.lisp :: module llm-support"
        :owned-tables 3
        :migrated-from "v0.4.13 category system-support :: global-observability"
        :mcp    "mission_llm_trace / mission_cost_report (🚧)")

      (module slot-support
        :desc   "Slot 运行时 — session 绑定 + learning-engine AI 任务 + dynamic slot lifecycle"
        :target "intent-memory.lisp :: module slot-support"
        :owned-tables 3
        :migrated-from "v0.4.14 category system-support :: compute-runtime"
        :mcp    "mission_slots / mission_slot_history / mission_compute_slot")

      (module system-support
        :desc   "系统级基础 — 告警 + router 归档 + vision 缓存 + infra 游标 + backfill + capability usage derived monitor + 4 legacy"
        :target "intent-memory.lisp :: module system-support"
        :owned-tables 14
        :migrated-from "v0.4.15 category 升格为 module (剩 LLM 3 + slot 3 分离后的 10 active + 4 legacy)"
        :mcp    "mission_incident / mission_router_chat / mission_sys_config / mission_sys_logs / mission_infra_query / mission_inbox (legacy)")

      (module embedding-support
        :desc   "embedding 列跨表治理 — 0 张独占表, 管 5 承载表 + 1 audit 表的列契约 (column-ownership)"
        :target "intent-memory.lisp :: module embedding-support"
        :owned-tables 0
        :special-nature "column-ownership vs row-ownership 双轨: 本 module 管 '列契约 + policy', 承载表的行归原 module (kb-manager / conversation-logs / project-management)"
        :migrated-from "v0.4.21 cross-cutting :: capability embedding-storage-governance 升格")

      ;; 横切能力
      (cross-cutting
        :desc   "db-trait-abstraction (9 store) / retention-policy / migrations-runner (embedding 治理 v0.4.21 已升格为 module)"
        :target "intent-memory.lisp :: cross-cutting")

      ;; Pillar Interfaces — 正交维度 (v0.4.18)
      (pillar-interfaces
        :desc   "5 surface (mcp / worker-trait / frontend / cross-pillar / external-filesystem) × 9 module 正交矩阵"
        :target "intent-memory.lisp :: pillar-interfaces"
        :binding "每个 writer/reader 通过 :binds-to 指向 surface; 96 个 writer/reader 100% 覆盖"))

    ;; ── 关键基础设施位置 (快速导航) ──
    (key-locations
      (mission-store-trait    :at "crates/missiond-core/src/db/traits.rs  — 9 store 超 trait (v0.4.20 修正, 原 13)")
      (projects-table         :at "crates/missiond-core/src/db/pg/project.rs")
      (board-table            :at "crates/missiond-core/src/db/board.rs")
      (knowledge-table        :at "crates/missiond-core/src/db/knowledge.rs")
      (conversation-table     :at "crates/missiond-core/src/db/conversation.rs")
      (audit-table            :at "crates/missiond-core/src/db/audit.rs")
      (timeline-ssot          :at "pillar 四 event_log (SSOT, v1.3.0+) — 原 timeline.rs 代码待 cutover 后删")
      (intent-loader          :at "crates/missiond-daemon/src/handlers/knowledge/intent.rs")
      (lisp-survey-worker     :at "crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs")
      (conversation-logger    :at "crates/missiond-daemon/src/workers/local/conversation_logger.rs")
      (embedding-worker       :at "crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs  — 生成路径在 pillar 二 2.3")
      (context-pipeline       :at "crates/missiond-daemon/src/context/")
      (flow-engine-v2         :at "crates/missiond-daemon/src/engine/flow/")
      (migrations             :at "crates/missiond-core/migrations/"))

    :maturity-ladder "v0.4.x 演进: 4 成熟模块 → 5+3 (v0.4.15 category 升格) → 5+4 (v0.4.21 embedding-support 新建) + pillar-interfaces (v0.4.18) 正交维度 + 命名去歧义 (v0.4.19)"
    :note "此 pillar 只列导航; 详细模块内部 in/core/out 在 intent-memory.lisp")



  ;; ═══════════════════════════════════════════════════
  ;;  二 · worker (Worker Layer)
  ;;  MissionD 如何驱动外部 / 后台计算
  ;;  = 三种传输介质 (PTY / LLM API / 本地) + 统一编排
  ;; ═══════════════════════════════════════════════════
  (pillar worker
    :canonical-ref ".missiond/v2/intent-worker.lisp"
    :canonical-status "v0.5 phase-C 2026-04-26 (recursive contract + xjp-router provider + mission_execution manager + project-root spawn cwd design + claudecode-workstation-orchestration policy operational-practice + architecture-designed; shared execution log + scoped commit handoff 升级为默认并行工位协议; mission_execution dispatch_strategy/target_project/requested_cwd 已写入 companion log meta — code-aligned partial; plan-runner v0 + auto-selection v1 sexp hint parsing 已 code-aligned partial 单节点 dispatch; 完整 PLAN DAG scheduler architecture-designed; scoped commit daemon enforce / ExecutionEvent dispatch metadata / PlanNodeStateChanged 扩展 / plan-runner v1 仍 code-alignment pending)"
    :v0.1-archive ".missiond/v2/drafts/gptpro/intent-worker.lisp"
    :v0.2-gptpro-archive ".missiond/v2/drafts/gptpro/intent-worker-v0.2.lisp"
    :execution-log ".missiond/v2/worker-pillar-execution.lisp"
    (purpose "系统如何把计算派出去 — 三种传输 + 统一调度抽象")

    ;; ── 2.1 PTY 传输: 直接控制 CLI 进程 ──
    (section pty
      (desc "对 Claude / Gemini / Codex CLI 的终端级感知 + 操作,把终端当一等公民")

      (component pty-manager
        (desc "多会话管理器: 生命周期 + 调度 + 异常处理")
        :target "crates/missiond-pty/src/manager.rs")

      (component session
        (desc "单个 PTY 会话: 读写 / 截屏 / 异常 / 增量提取")
        :target "crates/missiond-pty/src/session.rs"
        :children ("screenshot 截屏" "extractor 增量提取" "anomaly 异常检测"))

      (component semantic-parser
        (desc "PTY 输出 → 结构化状态: idle / running / confirm / title / tool-call / fingerprint")
        :target "crates/semantic-terminal (独立外部 crate)"
        :tracks ("state 状态" "confirm 确认对话框" "tool 工具调用" "title 终端标题" "fingerprint 指纹识别"))

      (component pty-event-worker
        (desc "监听 PTY 状态变更,发射 slot 事件,自动批准已知确认弹窗")
        :target "crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
        :emits "SlotBecameIdle / SlotStuck"
        :auto-approves "'don't ask again' / 'always' / 'trust' / '不再' 关键词")

      (component slot-manager
        (desc "计算位(slot)管理: 常驻 Claude CLI 实例池 + 动态按需调度")
        :target "crates/missiond-daemon/src/slot_manager/"
        :authority "SlotManager 是槽位生命周期的唯一权威")

      (component slot-orchestrator
        (desc "按 slot 角色驱动对应 PTY 控制器,代码中 CC/Gemini 两类控制器; project-bound spawn cwd 必须是目标项目根")
        :target "crates/missiond-daemon/src/slot_orchestrator/"
        :children ("cc_controller.rs — Claude Code PTY 控制器"
                   "gemini_controller.rs — Gemini CLI PTY 控制器"
                   "agent.rs — agent 任务派发"))

      (component conversation-ingestion
        (desc "PTY 写出 JSONL 后的解包路由 — conversation-logger worker 的后端")
        :target "crates/missiond-daemon/src/events_sync.rs"
        :routes "handle_new_events (实时增量) / backfill_conversation_events (启动一次性回填)"
        :helpers "extract_visible_text / extract_tool_names_csv / extract_tool_names"
        :writes-to "conversation_messages / conversation_events"
        :consumer "conversation-logger worker (local)"
        :note "文件名 events_sync.rs 是历史遗留,与 TimelineEvent 总线无关,实际只处理 PTY JSONL → DB"))

    ;; ── 2.2 LLM 接入层: 多模型统一门面 ──
    (section llm-gateways
      (desc "按 model 路由到 API 或 PTY,叠加限流 / 重试 / 观测")
      :target "crates/missiond-daemon/src/llm/"

      (component llm-gateway
        (desc "顶层 trait + 工厂,按 model_id 分派到具体 gateway")
        :target "llm/llm_gateway.rs")

      (component llm-gate
        (desc "跨 gateway 共享的限流闸门 (并发 / rate-limit)")
        :target "llm/llm_gate.rs")

      (component sonnet-gateway
        (desc "Claude Sonnet API chat gateway")
        :target "llm/sonnet_gateway.rs"
        :routes-to "Anthropic API (chat)"
        :used-by "translation / arch-maintenance / retro / lisp-survey workers"
        :embedding-removed "embedding 已迁 xjp-router-gateway; sonnet 只做 chat")

      (component xjp-router-gateway
        (desc "QWEN3 embedding 独立 provider adapter; 未来可扩 chat/rerank")
        :target "llm/xjp_router_client.rs"
        :routes-to "xjp-router HTTP /embed on Windows 12900KF + RTX3090Ti"
        :used-by "embedding-worker → kb_embeddings / ast_embeddings / turn_topics"
        :status "code-aligned for embedding; chat/rerank deferred"
        :embedding-invariant "qwen3 是唯一 embedding provider, 禁止降级兜底 — 失败直接报错")

      (component gemini-gateway
        (desc "Gemini 多路适配 — driver 分派 + CLI PTY / Cloud API / File API 三种模式")
        :target "llm/{gemini_driver,gemini_cli,gemini_client,gemini_file_api,gemini_pty}.rs"
        :modes "CLI PTY / Cloud API / File API")

      (component codex-gateway
        (desc "Codex = Claude Code PTY 模式 — 经 slot_orchestrator/cc_controller")
        :target "llm/codex_cli.rs + slot_orchestrator/cc_controller.rs"
        :routes-to "Claude Code CLI via PTY")

      (component minimax-gateway
        (desc "⚠ 已弃用,保留向后兼容,生产路径已迁 Sonnet")
        :target "llm/{minimax_gateway,minimax_client}.rs"
        :status "deprecated")

      (component prompts
        (desc "跨 gateway 共享 system / task prompt 模板")
        :target "llm/prompts.rs"))

    ;; ── 2.3 后台 worker 集群: 19 个计算租户 ──
    (section workers 19
      (desc "反应式 + 定时 + 外部触发的后台计算单元,按执行介质分组")
      :target "crates/missiond-daemon/src/workers/"
      :v1.3.0-change  "sonnet 组 briefing_worker 删除 (SSOT cutover, commit 6789509); 6 → 5"
      :v0.4.12-change "codex 组 step_narrator 删除 (narration 表下线); 2 → 1; 总 20 → 19"

      (group sonnet 5
        :examples "embedding / translation / arch-maintenance / retro / lisp-survey"
        :routes-via "SonnetGateway (直接 API)"
        :target "workers/sonnet/"
        :writes-to-memory
          ("embedding → kb-manager(knowledge.embedding_vec + ast) + conv-logs(message_embeddings + topic_vectors) + project-mgmt(skill_topics)"
           "translation → conv-logs(message_translations)"
           "arch-maintenance → kb-manager(knowledge category=architecture)"
           "retro → conv-logs(retrospective_results)"
           "lisp-survey → 项目 .missiond/intent.lisp 文件 (project-management)"))
      (group codex 1
        :examples "vision"
        :routes-via "Claude Code PTY via slot_orchestrator/cc_controller"
        :target "workers/codex/"
        :writes-to-memory "vision → system-support(image_descriptions)"
        :v0.4.12-removed "step_narrator.rs 随 message_narrations 表下线")
      (group gemini 1
        :examples "strategy"
        :routes-via "Gemini CLI PTY via slot_orchestrator/gemini_controller"
        :target "workers/gemini/")
      (group local 12
        :examples "conversation-logger / conversation-organizer / pty-event / tagger-chunker / experience-harvester / reconcile / gemini-reconcile / ast-sync / code-prefetch / codex-ingestion / gemini-logger / xjpcode-briefing"
        :routes-via "纯本地计算,无 LLM 依赖"
        :target "workers/local/"
        :note "数量最多,承担 JSONL 摄入 / 分块 / 打标 / 时间线同步 / 外部状态对账"
        :writes-to-memory
          ("conversation-logger / codex-ingestion / gemini-logger / gemini-reconcile → conv-logs (三引擎摄入)"
           "conversation-organizer → conv-logs(turns + tool_calls)"
           "experience-harvester / tagger-chunker → kb-manager(knowledge)"
           "ast-sync → kb-manager(ast_nodes/beacons)"
           "gemini-reconcile → system-support(reconcile_watermarks)")))

    ;; ── 2.4 编排: 生命周期 + 级联控制 ──
    (section orchestration
      (desc "worker 注册 / spawn / 级联 pause-resume 的统一治理")

      (component worker-registry
        (desc "BackgroundWorker trait 注册 + spawn + ControlTree 依赖自动注入")
        :target "workers/registry.rs"

        (trait BackgroundWorker
          (const KIND :type "WorkerKind" :enum "Sonnet / Codex / Gemini / Local")
          (method name         :returns "&str")
          (method extra-deps   :returns "Vec<Dependency>")
          (method run          :args "ctx: WorkerContext"))

        (struct WorkerRegistry :desc "全局注册表 + spawn 入口")
        (struct WorkerHandle   :desc "单个 worker 句柄 — 停止 / 状态查询")
        (struct WorkerContext  :desc "注入: ControlManager + AppState + shutdown signal")
        (struct WorkerInfo     :desc "对外元信息")

        :invariant "KIND 必须匹配 worker 所在子目录; ControlTree provider 依赖由 spawn_worker 自动注入")

      (component control-tree
        (desc "细粒度级联 pause / resume — worker / 数据流 / 项目 三层隔离")
        :target "crates/missiond-daemon/src/control_tree.rs"

        (struct ControlTree
          (field global-paused       :type "bool"                          :desc "全局总闸")
          (field providers           :type "HashMap<CtlProvider, bool>"    :desc "按 LLM provider 暂停 (Sonnet/Codex/Gemini)")
          (field domains             :type "HashMap<CtlDomain, bool>"      :desc "按数据域暂停 (Memory/Embedding/KB/...)")
          (field workers             :type "HashMap<String, bool>"         :desc "按 worker 名显式覆盖")
          :workers-semantics "true=强制暂停; false=强制恢复(调试 override); 不存在=跟随级联"
          (field slot-roles          :type "HashMap<String, bool>"         :desc "按槽位角色暂停")
          (field projects            :type "HashMap<String, bool>"         :desc "按 project_id 暂停(项目级数据流隔离)")
          (field domain-paused-at    :type "HashMap<CtlDomain, i64>"       :desc "域暂停时间戳,仅信息性不参与判断"))

        (cascade-priority
          :method "is_effectively_paused(worker_name, deps: &[Dependency])"
          :order "三级优先,从高到低:"
          (p1 worker-explicit-override
            :semantics "workers[name]=true → 恒暂停; =false → 恒恢复(debug); 不存在 → 跟随级联")
          (p2 global-kill-switch
            :semantics "global_paused=true → 全部 worker 暂停(除非 worker 有显式覆盖)")
          (p3 provider-domain-cascade
            :semantics "逐个检查 Dependency::Provider / Dependency::Domain; 任一 true → 暂停"))

        (struct ControlManager
          :pattern "push-based watch broadcast (NOT polling)"
          :transport "tokio::watch::channel<ControlTree>"
          :semantics "变更 → send_modify() 原子更新 → 所有订阅者经 changed().await 收通知"
          :persistence "control_tree.json — 崩溃恢复经 spawn_blocking 写入"
          :worker-await "Worker 在 select! 中 await watch::Receiver::changed() — 零成本异步推送,非 HashMap 轮询"
          (mutations "set_global_paused / set_provider / set_domain / set_worker / set_slot_role / set_project"))

        :project-pause-note "is_project_paused(id) 由 handler 独立检查,不属于 is_effectively_paused() 的 worker 级联 — 项目控制数据流,不控制 worker")

      ;; ── 驱动 memory state transitions 的 engine (非 worker 也非 dispatcher) ──
      (component autopilot
        (desc "任务队列自主推进引擎 — tick 扫 board → CAS 占用 → 派发 → lease 回收")
        :target "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
        :tick "5-10s 可配"
        :tick-pipeline "memory-scheduler → extraction-check → board-task-dispatch → flow-progression → supervision-check"

        (dispatch-logic
          "list_autopilot_tasks: WHERE auto_execute=1 AND status='open' ORDER BY (assignee 存在) → order_idx"
          "claim_board_task(id, autopilot_id, 'pty_slot') 原子 CAS 占用"
          "claim 成功 → status→running + 派给 assignee 或自动选 slot"
          "list_running_autopilot_tasks: 监控已 claim 任务的租约"
          "lease 超期 → recover_stale_running_tasks 强制 reset open")

        (writes-to-memory
          :primary "board_tasks (CAS claim / status 推进 / lease 回收)"
          :auxiliary "prompt_snapshots (save_prompt_snapshot: task 执行时 prompt + KB citation 存档)")

        :serves "pillar 一 memory :: module board :: path task-queue-lifecycle"
        :scan-reads "board_tasks (内部决策前置读取, 自读自写闭环)"
        :invariant "CAS 原子保证多 executor 并发安全; lease 保证崩溃后任务可回收")

      (component flow-engine-v2
        (desc "声明式工作流编排引擎 — flow YAML 节点的运行时执行器")
        :target "crates/missiond-daemon/src/engine/flow/{mod,runner,handlers,loader}.rs"
        :node-types 5 "LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks"
        :loads "$MISSIOND_HOME/flows/*.yaml (pillar 五 :: workflows :kind executable)"

        (execution-model
          "逐节点执行; 每节点完成后 persist_context → update_board_task(flow_context JSON)"
          "flow_phase 推进 + 变量插值"
          "支持分支 + 并行 (ParallelSlotTasks)")

        (writes-to-memory
          :primary "board_tasks.flow_context (每节点 persist 保证崩溃可恢复)"
          :storage "flow_context JSONB 独立于 status 变动存续")

        :serves "pillar 一 memory :: module board :: path task-queue-lifecycle"
        :invariant "每节点执行后必须 persist; 失败时上游节点结果保留"))

    ;; ── 2.5 Code Generation (Forge 冲压) ──
    (section code-generation
      (desc "Forge 冲压: Lisp → IR → Rust; build-time + MCP 运行时触发")
      :cross-ref "pillar 五 intent-layer :: component forge (冲压器本体, 独立仓库)"
      :migrated-in "memory v0.2 :: 1.4 cross-cutting :: gen-crud (2026-04-19)"

      (component gen-crud
        (desc "CRUD 代码按领域分文件, 模式驱动 — 改 lisp 不改代码")
        :target "crates/missiond-core/src/db/gen_{kb,board,conversation,compute,knowledge,misc,pipeline,skill,audit}.rs")

      (component gen-types
        (desc "跨 crate 共享的枚举 + 结构体")
        :target "crates/missiond-core/src/types/gen_types.rs")

      (component gen-server
        (desc "MCP JSON-RPC 服务器骨架")
        :target "crates/missiond-mcp/src/gen_server.rs")

      (invocation
        :build-time "Forge CLI 冲压 (外部构建步骤, ~/Projects/jarvis-forge)"
        :runtime    "mission_forge_build MCP tool (daemon 运行时触发)"))

    ;; ── 2.6 Search Engines (搜索引擎) ──
    (section search-engines
      (desc "四路查询 + 融合打分 — 所有需要从记忆检索的计算")
      :migrated-in "memory v0.3 :: module search-engines (2026-04-19) — 搜索是计算不是数据, 归 worker"
      :rationale "搜索引擎是 computation, 被搜索的内容才是 memory"

      ;; 索引维护来源
      (index-sources
        (source migrations-defs
          :desc "HNSW / GIN FTS / trigram 索引在 SQL migration 中定义"
          :code "crates/missiond-core/migrations/*.sql")

        (source embedding-column-writes
          :desc "embedding 列写入 → HNSW 索引增量更新 (pgvector 原生)"
          :writer   "2.3 workers :: embedding-worker (sonnet 组)"
          :provider "2.2 llm-gateways :: xjp-router-gateway (qwen3 路由, code-aligned)"
          :writes   "5 张表 embedding_vec 列 (knowledge / conversation_topic_vectors / message_embeddings / skill_topics / ast_nodes)"
          :governance "契约见 pillar 一 memory :: cross-cutting :: capability embedding-storage-governance (v0.4.6+)"
          :invariant "禁止降级兜底, 失败直接报错")

        (source db-write-auto-index
          :desc "PG 原生机制 — 写入相关表时 GIN FTS / trigram 索引自动更新"
          :mechanism "PostgreSQL GIN / pg_trgm 扩展原生支持"))

      ;; 四路引擎
      (engine vector-hnsw
        :desc   "HNSW 近邻搜索 — 语义相似"
        :impl   "pgvector 扩展"
        :index  "knowledge.embedding_vec / conversations.summary_embedding"
        :dim    512
        :query  "ORDER BY embedding_vec <=> $query_vec LIMIT K")

      (engine fulltext-gin
        :desc   "GIN FTS 全文索引 — 关键词匹配"
        :impl   "PostgreSQL to_tsvector + GIN"
        :index  "knowledge.content / conversations.summary / messages.content"
        :query  "tsvector @@ plainto_tsquery($q)")

      (engine fuzzy-trigram
        :desc   "trigram 模糊字符串匹配 — 拼写容错 / 子串匹配"
        :impl   "pg_trgm 扩展"
        :query  "col ILIKE '%$q%' + similarity(col, $q) > threshold")

      (engine tag-exact
        :desc   "category / tags 精确过滤 — 结构化索引"
        :query  "WHERE category = $cat AND tags @> ARRAY[$t]")

      ;; 融合打分
      (component fusion-ranker
        :desc     "四路结果融合"
        :strategy "向量分 + FTS 分 + trigram 分 + tag 过滤, 加权聚合"
        :scoping  "叠加 pillar 一 memory :: project-management :: scope-mechanism (project_id OR NULL)"
        :code     "daemon/src/handlers/knowledge/kb.rs + context/retrieval.rs")

      ;; 消费者 (谁在用搜索)
      (consumers
        (consumer mcp-kb-search
          :tools "mission_kb_search / mission_kb_query / mission_kb_ops"
          :invoked-by "Claude Code / 前端 / Agent")

        (consumer mcp-insight-recall
          :tools "mission_insight / mission_memory / mission_code_search"
          :focus "综合洞察 / 记忆召回 / 代码语义搜索")

        (consumer context-pipeline-retrieval
          :code    "daemon/src/context/{pipeline,retrieval}.rs"
          :purpose "为 LLM 调用拼 prompt 的语义检索"
          :budget  "estimate_tokens + allocate_budget 多源边际打分"
          :note    "最密集的搜索消费者 — 每次 LLM 调用都触发")

        (consumer mcp-universe-graph
          :tool  "mission_universe_graph"
          :reads "跨项目 KB 索引 → 生成实体/关系图"))))


  ;; ═══════════════════════════════════════════════════
  ;;  三 · 工具 (Tools)
  ;;  MCP 协议 + 对外暴露的全部能力
  ;; ═══════════════════════════════════════════════════
  (pillar tools
    :canonical-ref ".missiond/v2/intent-tools.lisp"
    :canonical-status "v0.7 phase-C 2026-04-26 (83 actual tools classified; mission_directive/plan/workflow actor v0 + plan-runner v0 + auto-selection v1 + methodology compiler v0 + generated flow loader + mission_execution dispatch_strategy companion log + mission_capability_usage semantic evidence v1 all code-aligned partial; mission_execution complete future fields for scoped commit handoff architecture-designed; 不新增 tool — scoped commit handoff 复用 mission_execution, agent-team 仍任务 .md 提示; file-first .lisp writer / 完整 PLAN DAG / auto QuestionEvent / semantic lifting / forge compiler / ExecutionEvent dispatch metadata / planner-class model alias / scoped commit daemon enforce 仍 code-alignment pending)"
    :gptpro-v0.1-archive ".missiond/v2/drafts/gptpro/intent-tools.lisp"
    (purpose "通过 MCP JSON-RPC 协议暴露给 Claude Code / 其他 Agent 的能力集")

    (component mcp-server
      (desc "stdio JSON-RPC 服务器,MCP 协议入口")
      :target "crates/missiond-mcp")

    (component dispatch
      (desc "请求 → 域 → handler 的路由分派")
      :target "crates/missiond-daemon/src/infra/mcp_client.rs")

    (component tool-schema
      (desc "所有工具的 JSON Schema 声明(当前 83 个工具,4 大域)")
      :target ".missiond/intent-mcp-defs.lisp"
      :count "67+ tools")

    (domains
      (compute    "slot / task / worker / job / cascade")
      (sysinfra   "permission / config / log / daemon / infra / power")
      (knowledge  "kb / board / cascade / skill / memory / intent")
      (comm       "conversation / pty / question / router_chat / timeline / inbox"))

    (component tool-call-log
      (desc "所有工具调用的执行记录,供审计 / 统计 / 回放")
      :tables "tool_calls"))


  ;; ═══════════════════════════════════════════════════
  ;;  四 · 事件总线 (Event Bus) — Log-as-Bus
  ;;  追加式日志即总线,7 步流水线处理 + tail-and-pull 订阅
  ;; ═══════════════════════════════════════════════════
  ;; 详细规格在独立 lisp(v1.3.4 architecture-unlocked),本处只作导航摘要
  (pillar event-bus
    :file ".missiond/v2/intent-event-bus.lisp"
    :execution-log ".missiond/v2/intent-event-bus-execution.lisp"
    :lock-status "architecture-unlocked v1.3.4 — direct edit allowed; Domain::Execution + CapabilityUsage ObservabilityEvent code-aligned, current domain count 13"
    :paradigm "Log-as-Bus(追加式日志是唯一真理源,不是 broadcast + 补漏)"

    (purpose "进程内神经网络 — 追加式日志 + 类型化 topic 路由 + 游标式订阅")

    (one-line-spec
      "DB seq + 13 domain topic + at-least-once + batch-ack cursor (双阈值) "
      "+ subscription-name PK + pause=drop/live-resume + >8KB side-channel "
      "+ producer-ack-after-commit + no-global-min-replay + tail-and-pull catch-up")

    ;; ── 结构 ──
    (structure
      (section-4.1 ingress
        :desc    "唯一入口 log.append(event, opts)"
        :target  "crates/missiond-core/src/event/log/mod.rs")

      (section-4.2 core
        :desc    "7 步流水线(上到下对应执行顺序,前 4 同步 / 后 3 异步)"
        :target  "crates/missiond-core/src/event/pipeline/"
        (step-1 guard   :at "pipeline/step1_guard/"   :does "因果深度 ≤ 10 + 类型解析")
        (step-2 decide  :at "pipeline/step2_decide/"  :does "claim-check 8KB 阈值 + ephemeral 决策")
        (step-3 commit  :at "pipeline/step3_commit/"  :does "批处理 INSERT + BIGSERIAL seq + dedup")
        (step-4 ack     :at "pipeline/step4_ack/"     :does "oneshot 回 producer")
        (step-5 tail    :at "pipeline/step5_tail/"    :does "Dispatcher 长轮询 event_log")
        (step-6 gate    :at "pipeline/step6_gate/"    :does "control-plane 暂停域过滤")
        (step-7 fanout  :at "pipeline/step7_fanout/"  :does "Topic<T> broadcast 扇出"))

      (section-4.3 egress
        :desc    "tail-and-pull 两阶段 + cursor + 6 个 combinators"
        :target  "crates/missiond-core/src/event/subscription/")

      (section-4.4 cross-cutting
        :desc    "causation-guard + metrics + 9 chaos tests + InMemoryBus"
        :targets
          ("crates/missiond-core/src/event/pipeline/step1_guard/causation.rs"
           "crates/missiond-core/src/event/metrics/"
           "crates/missiond-core/tests/event_chaos.rs"
           "crates/missiond-core/src/event/in_memory/"))

      (section-4.5 deferred
        :desc "FreezeAndCatchUp + Prometheus backend 已声明未实现"))

    ;; ── 关键基础设施位置(快速导航)──
    (key-locations
      (log-schema         :at "crates/missiond-core/migrations/20260419000000_event_log.sql")
      (domain-types       :at "crates/missiond-core/src/event/events/ (13 个 domain enum)")
      (log-trait          :at "crates/missiond-core/src/event/log/mod.rs")
      (log-writer         :at "crates/missiond-core/src/event/pipeline/step3_commit/log_writer.rs")
      (dispatcher         :at "crates/missiond-core/src/event/pipeline/step5_tail/")
      (subscription-api   :at "crates/missiond-core/src/event/subscription/api.rs")
      (daemon-bus-glue    :at "crates/missiond-daemon/src/bus/")
      (ws-bridge          :at "crates/missiond-daemon/src/bus/ws_bridge.rs  — 前端 wire-format 字节级保留")
      (retention-cron     :at "crates/missiond-daemon/src/bus/retention_cron.rs"))

    ;; ── 重构来龙去脉 ──
    (refactor-lineage
      :migrated-from "v1: DaemonEvent god enum + Timeline Writer + event_router 8 consumers + 4 MPSC bypass + sweeper"
      :migrated-to   "v2: 13 domain enum + event_log 单一真理源 + Dispatcher live-only + 14 typed subscribers"
      :branch        "refactor/event-bus-v2 (merged to main commit e139ecf, 2026-04-19)"
      :refactor-commits 16
      :refactor-summary ".missiond/v2/_refactor-summary.md"
      :methodology-template ".missiond/workflows/bus-refactor.lisp")

    :note "worker 集群 / worker-registry / control-tree 在 pillar 二;此 pillar 只管事件基础设施")


  ;; ═══════════════════════════════════════════════════
  ;;  五 · 意图层 (Intent Layer)
  ;;  系统的自我描述 + 自感知 + 自演化
  ;; ═══════════════════════════════════════════════════
  (pillar intent-layer
    :canonical-ref ".missiond/v2/intent-intent-layer.lisp"
    :canonical-status "v0.4 phase-B 2026-04-25 (unified-entry-pipeline actor v0 全部 code-aligned partial: directive-compiler v0 / plan-compiler v0 / plan-runner v0 + auto-selection v1 (单节点) / workflow-distiller v0 / methodology compiler v0 / generated flow loader; capability-evolution-governance semantic evidence v1 已 code-aligned partial 5 sources + lisp hint merge-candidate; workstation-dispatch-policy operational-practice + companion log dispatch_strategy 已落; section action-instruction-actor :: actor plan-dag-scheduler architecture-designed (完整 11-stage / per-node FSM / claim-lease 复用 agent-execution-coordination); file-first .lisp writer / 高阶 semantic lifting / forge compiler / 完整 PLAN DAG scheduler 代码同构 / auto QuestionEvent gate / ExecutionEvent dispatch metadata / planner-class model alias 仍 code-alignment pending)"
    :gptpro-v0.1-archive ".missiond/v2/drafts/gptpro/intent-intent-layer.lisp"
    (purpose "元层: 系统如何描述自己, 如何感知变化, 如何演进, 以及全局用户指令")

    (component intent-files
      (desc ".missiond/*.lisp 意图声明, 按主题拆分并行加载")
      :granularities "L1-Blueprint / L2-Topology / L3-Implementation"
      :count "27 files (v1) + this v2 draft")

    (component intent-graph
      (desc "文件间 module-link 关系, 构成有向图, 可供可视化 / 治理")
      :target "forge-daemon/src/intent_graph.rs")

    (component forge
      (desc "外部冲压器: Lisp 意图 → IR → Rust 代码 (Generation Gap 隔离)")
      :location "~/Projects/jarvis-forge (独立仓库)"
      :breaks-if "codegen-pattern-change / ir-whitelist-change")

    (component lisp-survey-worker
      (desc "检测 ContextualCommitDetected → 差量更新对应项目的 intent.lisp")
      :target "workers/sonnet/lisp_survey_worker.rs"
      :debounce "60s per project_id"
      :prevents-self-loop "slot_id == lisp-surveyor 的 commit 自动跳过")

    (component governance
      (desc "治理规则 / lint / 模式声明: strict-codegen / descriptive / experimental")
      :target "forge-daemon/src/governance.rs")

    ;; ── 全局 CLAUDE.md · 跨项目永久用户指令 ──
    (component global-claudemd
      (desc "全域总纲 — 指挥官对 Claude 的跨项目永久指令")
      :path "~/.claude/CLAUDE.md"
      :scope global-user
      :format "Markdown + 可选 YAML frontmatter"
      :purpose "全局偏好 / 行为约束 / 宇宙总纲 — 每次会话必加载"
      :loaded-by "Claude Code 系统启动自动加载进 system prompt"
      :writer "用户手动编辑 / Claude Code Edit tool"
      :nature "元层约束 — 非业务记忆 (项目级约束见 memory pillar :: project-management :: helper project-claudemd-manager)"
      :rationale "放 pillar 五 而非 memory pillar: 此文件是"系统如何被指挥"的声明, 属元层")

    (component global-claudemd-manager
      (desc "全局 ~/.claude/CLAUDE.md 的读/写/reload 管理")
      :actions "read / edit / reload"
      :code "crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs + crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs"
      :mcp "mission_global_instruction (read/edit/reload)"
      :readers "Claude Code 每次会话启动"
      :writers "mission_global_instruction(action=edit) / 用户手动 / Claude Code Edit tool (文件层)"
      :status "code-aligned; read/edit full, reload manual-reload-required because Claude Code owns session bootstrap"
      :cross-ref "项目级 <project>/CLAUDE.md 的 manager 在 memory pillar :: project-management :: helper project-claudemd-manager")

    ;; ═══════════════════════════════════════════════════
    ;; Action-Instruction Specs — 动作与指令规约
    ;;   (v0.4.4 从 memory pillar 迁入)
    ;;   区别:memory 只存'项目代码真实状态的 intent.lisp';
    ;;         本 section 管所有'描述动作和指令'的 DB 表 + 文件
    ;; ═══════════════════════════════════════════════════
    (section action-instruction-specs
      (desc "所有描述'应该做什么 / 如何做'的规约 — DB 表 (schema 归 memory directive-layer) + Lisp/YAML 文件")
      :migrated-from "memory pillar :: project-management (4 tables) + non-db-forms (3 variants + 1 form) in v0.4.4"
      :rationale "memory 记'是什么'(facts); 本层记'应该做什么'(prescriptions) — 分层原则"
      :v0.4.19-note "DB 表 schema 实际在 memory :: module directive-layer 管, 本 section 只做概念性 cross-ref; intent 表 → directive 表 rename 同步"

      ;; ── DB 表 (v0.4.17: schema 归 memory directive-layer module, 本 section 只概念性描述) ──
      ;; v0.4.16: user_intents 移回 memory :: conversation-logs (writer=intent_analyst, trait=ConversationStore)
      ;; v0.4.17: intent/plan/workflow 3 张从 pillar 五 action-specs 剥离到 memory :: directive-layer
      ;;          原因: 按 'memory=库' 原则, schema + trait 接口归 memory; pillar 五 actor 是未来 writer
      ;;          撤回 v0.4.16 drop-candidate 误判 — 用户澄清这是 '刚建未启用' 预留 schema
      (component directive-spec-db
        (desc "user utterance → lisp 指令编译记录 — 三段式 pipeline 第一段")
        :table "directive"
        :schema-owned-by "memory :: module directive-layer :: plumbing directive-compilation"
        :cross-ref "intent-memory.lisp :: module directive-layer"
        :status "store+manager code-aligned partial"
        :future-writer "pillar 五 directive-compiler actor; mission_directive 是管理面 (compile dry-run/persist draft)")
        :v0.4.19-rename "原名 intent 表 → directive 表 (避命名歧义: 和 <project>/.missiond/intent.lisp 代码画像文件区分)"
        :vs-per-project-intent "memory :: project-management 里的 <project>/.missiond/intent.lisp 是 factual 代码快照; 本表是 'Jarvis 对用户话的 lisp 指令编译'")

      (component plan-spec-db
        (desc "directive 编译出的执行 DAG — 绑 board_task + 版本 + FSM")
        :schema-owned-by "memory :: module directive-layer :: plumbing plan-execution"
        :cross-ref "intent-memory.lisp :: module directive-layer"
        :status "store+manager code-aligned partial"
        :future-writer "pillar 五 plan-compiler actor — plan 编译 / FSM 迁移 / supersede-chain 策略; mission_plan 当前提供 dry-run/draft + execute bridge")

      (component workflow-spec-db
        (desc "从成功 plan 蒸馏的可复用模板 — 带 match_rules + 统计")
        :schema-owned-by "memory :: module directive-layer :: plumbing workflow-templates"
        :cross-ref "intent-memory.lisp :: module directive-layer"
        :status "store+manager code-aligned partial"
        :future-writer "pillar 五 workflow-distiller actor — distillation 算法 / 匹配阈值 / LRU 策略; mission_workflow 当前提供 match/apply/read-only/distill dry-run")

      ;; v0.4.16: user-intents-db component 已删除 — 该表归属 memory :: conversation-logs
      ;; writer: engine/learning_engine/intent_analyst.rs
      ;; readers: intent_analyst self + autopilot.rs:1496 (get_recent_intents)
      ;; trait: ConversationStore::insert_user_intent + 5 查询方法 (traits.rs:136-150)

      ;; ── Lisp / YAML 文件 (3 类) ──
      (component system-level-intent-files
        (desc "系统主架构 + pillar 级细节规约 Lisp 文件")
        :paths (".missiond/v2/intent.lisp 系统主架构"
                ".missiond/v2/intent-event-bus.lisp architecture-unlocked v1.3.3"
                ".missiond/v2/intent-memory.lisp v0.5.4"
                ".missiond/intent-db-*.lisp Forge 源 lisp"
                ".missiond/intent-pillar-*.lisp v1 分 pillar lisp")
        :purpose "系统自我描述 + Forge 冲压源"
        :moved-from "memory :: non-db-forms :: lisp-spec-files variant system-main/detail (v0.4.4)"
        :note "本层所有 intent*.lisp 都描述'系统应该如何'; 项目 intent.lisp (code snapshot) 不在这里")

      (component workflows
        (desc "统一工作流规约 — 两种 kind 共同表达'多步工作流', 但受众/粒度/执行性不同")
        :unified-in "v0.4.5 (原 workflow-lisp-templates + flow-yaml-templates 合并为单一 component)"
        :design-rationale "两种 kind 形式差异大但概念一致 — 保留各自格式优势, 统一纳管"

        (kind methodology
          (desc "Lisp 方法论模板 — 人类 / agent 参考的 SSOT; 机器执行需先编译成 executable YAML")
          :path ".missiond/workflows/*.lisp"
          :consumers "human + mission_intent tool + agent 参考 + future methodology compiler"
          :granularity "抽象叙事 — phases / principles / anti-patterns / baseline-numbers / decision-authority"
          :examples "bus-refactor.lisp (11-phase 事件总线重构方法论)"
          :executability "human-readable source; machine execution via F-methodology-to-executable-compile → generated YAML → mission_flow_run")

        (kind executable
          (desc "YAML 声明式节点编排 — flow-engine-v2 运行时执行")
          :path "$MISSIOND_HOME/flows/*.yaml"
          :loader "daemon/src/engine/flow/loader.rs"
          :executor "pillar 二 2.4 orchestration :: flow-engine-v2"
          :parser "serde_yaml::from_str::<FlowDefinition>"
          :granularity "具体机器操作 — 5 node types: LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks"
          :executability "✓ 机器执行")

        (relationship-between-kinds
          :overlap "都描述'多步工作流'"
          :split-axis "受众 (human vs machine) + 粒度 (抽象 vs 具体) + 执行性"
          :why-not-unify-format "Lisp 富元数据给人看, YAML 轻量 schema 给 flow-engine 消费; 硬统一两边都难用"
          :cross-ref-convention "可约定同名对照 (如 bus-refactor.lisp ↔ bus-refactor.yaml), 非强制"
          :future-possibility "已升级为 architecture target: F-methodology-to-executable-compile; 当前代码对齐待实现"))

      ;; ── Manager ──
      (component specs-manager
        (desc "action/instruction specs 的读/写/reload — 大部分 TBD")
        :actions "compile / approve / list / get / supersede / match / record-execution / sync-with-file"
        :status "manager/tools code-aligned partial — mission_directive / mission_plan / mission_workflow 提供 read/control/draft persistence/execute bridge; runtime writer actor 未实现"
        :files-status "intent/workflow/flow 文件层已有 readers (mission_intent / flow-engine-v2); writers 多为手动编辑"
        :cross-ref "memory :: project-management :: path project-code-snapshot (读 per-project 代码快照 FILE, 职责不同)"
        :future-work "实现 directive-compiler / plan-compiler / workflow-distiller actor, 并把 dry-run compile/distill 升级为 LLM-backed writer"))


  ;; ═══════════════════════════════════════════════════
  ;;  六 · 系统层 (System Layer)
  ;;  类型 + 传输 + RPC Gateway + 工具 — 运行时底座 (DB / 观测 已迁入 pillar 一)
  ;; ═══════════════════════════════════════════════════
  (pillar system-layer
    :canonical-ref ".missiond/v2/intent-system-layer.lisp"
    :canonical-status "v0.2 phase-B 2026-04-25 (runtime substrate + sysinfra/daemon/control surfaces)"
    :gptpro-v0.1-archive ".missiond/v2/drafts/gptpro/intent-system-layer.lisp"
    (purpose "无业务语义的运行时底座 — 类型 / 进程 / 传输 / RPC / 工具; DB 与观测已迁入 pillar 一 memory")

    ;; ── 6.1 核心共享类型 ──
    (section core-types
      (desc "跨 crate 共享的枚举 + 结构体 — 单一真理源")
      :target "crates/missiond-core/src/types/gen_types.rs (Forge-generated)"
      :v1-cross-ref "intent-types.lisp"

      (enums "BoardTaskStatus / EngineeringPhase / TaskStatus / EventType / AsyncJobStatus / AgentQuestionStatus / IncidentSeverity / IncidentSource / CliEngine / Lifecycle / SlotTrait")
      (structs "BoardTask / ConversationMessage / KnowledgeEntry / Task / InboxMessage / TaskEvent / AgentQuestion / IncidentRow / DynamicSlot / SkillTopic / ToolCallRecord")

      :serde "derive Serialize/Deserialize + as_str/from_str — DB / IPC / JSON-RPC 共享"
      :authority "枚举定义合法状态迁移 (例: BoardTaskStatus Open→Running→Done,禁跳)")

    ;; ── 6.2 进程与传输 ──
    (section process-transport
      (desc "守护进程生命周期 + IPC/WS 传输 + 全局状态 + 监督")
      :v1-cross-ref "intent-pillar-transport-bootstrap.lisp"

      (component bootstrap
        (desc "main.rs 启动序列 — 6 阶段严格依赖顺序")
        :target "crates/missiond-daemon/src/main.rs"
        (phases 6
          (p1 "DB pool + embed_model + event_bus")
          (p2 "ProjectRegistry 从 PG 加载")
          (p3 "PTYManager + SlotManager + MissionControl")
          (p4 "LLM gateways: sonnet / gemini / codex / (minimax 可选)")
          (p5 "Context pipeline + WorkerRegistry + ControlTree")
          (p6 "21 workers spawn + autopilot + ipc-handler + ws-server"))
        :invariant "每阶段依赖前一阶段;ProjectRegistry 必须早于 message_handler;event_bus 必须早于任何 handler(防事件丢失)")

      (component app-state
        (desc "全局共享状态 — Arc<RwLock<...>> 贯穿所有 handler")
        :target "crates/missiond-daemon/src/state.rs"
        :fields "db pool / event_bus / slot_manager / llm_gateway / context_pipeline / project_registry / 4 MPSC senders"
        :invariant "只读访问 (RwLock 只 read),启动后不再 write — 状态权威在 DB + event_bus")

      (component ws-server
        (desc "WebSocket 服务器 — 前端订阅端")
        :target "crates/missiond-core/src/ws/server.rs"
        :sub-components ("screenshot-broker — 异步截屏流分发"
                         "jarvis-trace — trace span 分发给客户端"))

      (component ipc
        (desc "mcp ↔ daemon 双向通信 (Unix socket / TCP)")
        :target "crates/missiond-core/src/ipc/mod.rs")

      (component supervisor
        (desc "worker 健康监控 + 重启")
        :target "crates/missiond-daemon/src/supervisor.rs"))

    ;; ── 6.3 RPC Gateway ──
    (section rpc-gateway
      (desc "JSON-RPC 服务器 — stdio(MCP 协议)+ IPC(daemon)双传输")
      :target "crates/missiond-mcp/src/gen_server.rs (Forge-generated)"
      :v1-cross-ref "intent-rpc-gateway.lisp"

      (methods "initialize / notifications/initialized / tools/list / tools/call / ping")

      (dispatch
        :rule "数据驱动: tool_name → handler 映射,非硬编码 match"
        :scope "83 tools × 4 domains / 8 legacy groups (schema 归 pillar 三)")

      (error-codes "UNKNOWN_TOOL / UNKNOWN_ACTION / MISSING_PARAM / INVALID_PARAM / NOT_FOUND / PERMISSION_DENIED / IPC_TIMEOUT / SPAWN_FAILED / DB_ERROR")

      :role "纯 plumbing — pillar 三 持有 tool schema,这里只负责路由 + 错误码")

    ;; ── 6.4 纯工具模块 ──
    (section pure-utils
      (desc "无 I/O 无状态的确定性工具函数 — 横向复用")
      :v1-cross-ref "intent-pure-utility.lisp"

      (component semantic-parsing-helpers
        :target "crates/missiond-core/src/semantic/gen_parsing.rs"
        :functions "is_spinner_char / split_args / extract_phase_from_parens / sanitize_line / has_activity_timer / is_idle_prompt"
        :consumer "extractor pipeline (pillar 二 2.1 PTY)")

      (component string-safety
        :target "crates/missiond-core/src/util/gen_string_helpers.rs"
        :functions "safe_byte_truncate / safe_char_truncate"
        :desc "UTF-8 边界安全截断,CJK 多字节字符不会断开"
        :rationale "替换代码库里所有 &s[..N] 危险切片")

      (component token-budget
        :target "crates/missiond-daemon/src/context/gen_budget.rs"
        :functions "estimate_tokens (英文 /4, 中文 /2) / allocate_budget (N 源 + 边际递减)"
        :consumer "context 窗口规划")))


  ;; ═══════════════════════════════════════════════════
  ;;  七 · 流程 (Flow)
  ;;  跨 pillar 的动作前后流程 — 把 memory 静态与 worker 计算串联成 narrative
  ;; ═══════════════════════════════════════════════════
  (pillar flow
    :canonical-ref ".missiond/v2/intent-flow.lisp"
    :canonical-status "v0.7 phase-C 2026-04-26 (83 actual tools indexed + F-intent-alignment-plan-execution-loop 8 stages canonical 统一入口 + 双 review gate + plan-runner 内部调度契约 + actor v0 code-aligned partial; F-execution-log-governance 增 F-scoped-commit-handoff: shared execution Lisp control plane + scoped git commit durability plane; F-methodology / F-capability-usage / F-workstation-dispatch 已 code-aligned partial; 完整 PLAN DAG scheduler architecture-designed; 不引入新 tool — file-first .lisp writer / 完整 PLAN DAG 代码同构 / auto QuestionEvent / semantic lifting / forge compiler / ExecutionEvent dispatch metadata / scoped commit daemon enforce 仍 pending)"
    :gptpro-v0.1-archive ".missiond/v2/drafts/gptpro/intent-flow.lisp"
    (purpose "跨 pillar 的动作前后流程 — 把 memory 静态与 worker 计算串联成 narrative")
    (rationale "v0.4.7 从 board 拆出 autopilot/flow-engine 后, 丢失了 end-to-end narrative; 本 pillar 补上")

    (principle
      :memory "状态 (snapshot) — 记什么是什么"
      :worker "机制 (engine) — 做怎么做"
      :flow   "编排 (choreography) — 串什么时候什么顺序做什么")

    (naming-convention
      :stage-id "s1 / s2 / ..."
      :at-target "pillar-X :: module/section :: component (跨 pillar 跳点)"
      :writes    "产生什么数据变动"
      :emits     "产生什么 DomainEvent (可选)")

    (flows-catalog
      :scope "当前 board-centric; 可扩展到 KB mutation / conversation ingestion / retro / context assembly"
      :count 5

      ;; ── Flow 1: 任务主生命周期 ──
      (flow board-task-main-lifecycle
        (desc "任务从创建到完成 — board 最核心的 end-to-end")
        (trigger "mission_board_create / decomposed child / autopilot scan auto_execute=1")

        (stages
          (s1 create
            :at     "pillar 一 memory :: board :: mcp-board-lifecycle"
            :writes "board_tasks status=open"
            :emits  "BoardTaskCreated")

          (s2 scan-decide
            :at      "pillar 二 2.4 :: autopilot"
            :reads   "board_tasks WHERE auto_execute=1 AND status=open"
            :decides "是否 claim + 派给哪个 slot / worker")

          (s3 atomic-claim
            :at     "pillar 一 memory :: board :: state-machine"
            :writes "status=running + claim_executor_id + lease_expires_at"
            :atomicity "SQL CAS — open→running 原子操作"
            :emits  "BoardTaskClaimed + BoardTaskStatusChanged")

          (s4 execute
            :at     "pillar 二 2.1 PTY slot / 2.3 workers / 2.4 flow-engine-v2"
            :action "实际执行任务; 有 flow_template 则走 flow-engine 逐节点"
            :side-effects "autopilot.save_prompt_snapshot → prompt_snapshots"
            :flow-ref "flow-engine-v2-node-execution (若走节点模式)")

          (s5 report-completion
            :at     "pillar 一 memory :: board :: core-operations"
            :writes "status=done/failed + claim_executor_id 清除 + lease 释放"
            :emits  "BoardTaskStatusChanged")

          (s6 downstream-cascade
            :at     "pillar 二 2.4 :: autopilot"
            :action "检查 depends_on 的下游 → unblock 或 retry-cascade"
            :optional true))

        (alternative-path lease-recovery
          :trigger   "autopilot tick 发现 lease_expires_at < now() 且 status=running"
          :at        "pillar 二 2.4 :: autopilot"
          :action    "调 BoardStore::recover_stale_running_tasks"
          :writes    "status=open + claim 清除"
          :rationale "executor 崩溃不留僵尸任务"))

      ;; ── Flow 2: 任务拆解 ──
      (flow board-task-decompose
        (desc "父任务 AI 分析 → 子任务 DAG")
        (trigger "mission_board_decompose(task_id, slot_id, hints)")
        (stages
          (s1 request
            :at     "pillar 一 memory :: board :: mcp-board-lifecycle"
            :action "派 slot 做分析")
          (s2 analyze
            :at     "pillar 二 2.1 :: PTY slot"
            :action "slot LLM 执行 AI 分析, 产出结构化 subtask plan")
          (s3 write-dag
            :at     "pillar 一 memory :: board :: core-operations"
            :writes "新 board_tasks rows (parent_id + depends_on JSONB)"
            :emits  "BoardTaskCreated (每个子任务一次)"))
        (result "Parent task + DAG of children with dependency links"))

      ;; ── Flow 3: Agent 提问阻塞 (已实现 auto-unblock) ──
      (flow agent-question-block-resume
        (desc "Agent 卡住 → 提问 → task 被 block → 回答后 auto-unblock")
        (trigger "mission_question create with task_id")
        (stages
          (s1 question-create
            :at     "pillar 一 memory :: board :: helper agent-questions"
            :writes "agent_questions status=pending"
            :side-effect "CAS UPDATE board_tasks SET status=blocked WHERE id=task_id"
            :serves "flow 暂停 — executor 不再 claim 此任务")

          (s2 human-answer
            :at     "用户手动 / 其他 agent / Claude Code 交互"
            :writes "agent_questions status=answered + answer text"
            :code   "db/question.rs :: answer_agent_question()")

          (s3 auto-unblock
            :at     "pillar 一 memory :: board :: answer_agent_question (同事务)"
            :trigger "answer_agent_question() 检查 task 所有 pending 问题是否全部 answered/dismissed"
            :writes "board_tasks status=blocked→open (仅当最后一个问题解决时)"
            :emits  "QuestionEvent::Resolved 到 event-bus"
            :code   "db/question.rs:156-170"))
        (status "✓ auto-unblock 已实现 — 之前标的 gap 是错的, v0.4.12 修正"))

      ;; ── Flow 4: Autopilot tick 流水线 ──
      (flow autopilot-tick-pipeline
        (desc "autopilot 每 5-10s 的完整 tick — 多个子流程依次跑")
        (trigger "autopilot 计时器 (5-10s)")
        (stages
          (s1 memory-scheduler
            :at "pillar 二 2.4 :: autopilot"
            :action "扫待唤醒的 reminder / 提醒 (若有)")
          (s2 extraction-check
            :at "pillar 二 2.4 :: autopilot"
            :action "检查 extract-worker 状态 + 进度")
          (s3 board-task-dispatch
            :at "pillar 二 2.4 :: autopilot"
            :flow-ref "board-task-main-lifecycle s2-s4 (scan + claim + 派发)")
          (s4 flow-progression
            :at "pillar 二 2.4 :: flow-engine-v2"
            :action "推进所有 flow_template 非空的 running task 一个节点")
          (s5 supervision-check
            :at "pillar 二 2.4 :: autopilot"
            :action "lease recovery (见 Flow 1 alternative-path) + 僵尸 slot 检测")))

      ;; ── Flow 5: Flow-engine 节点执行 ──
      (flow flow-engine-v2-node-execution
        (desc "flow_template YAML 节点的运行时执行 — board 的可选子流")
        (trigger "board_task.status=running 且 flow_template 非空")
        (stages
          (s1 load-yaml
            :at    "pillar 二 2.4 :: flow-engine-v2 :: loader"
            :reads "$MISSIOND_HOME/flows/<flow_template>.yaml"
            :parses-to "FlowDefinition (serde_yaml)")
          (s2 execute-node
            :at    "pillar 二 2.4 :: flow-engine-v2 :: runner"
            :types "LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks"
            :action "按节点类型分派, 变量插值 + 执行")
          (s3 persist-context
            :at     "pillar 一 memory :: board :: data-model"
            :writes "board_tasks.flow_context (JSONB) — 节点产出 + 状态"
            :invariant "每节点完成必须 persist — 崩溃恢复基础")
          (s4 advance-or-complete
            :at "pillar 二 2.4 :: flow-engine-v2 :: runner"
            :decides "flow_phase++ / 分支 / 全部完成则 report (→ Flow 1 s5)")))

      ;; ── 未覆盖的候选 (待扩展) ──
      (future-flows
        (kb-mutation-to-indexed      "mission_kb_mutate → knowledge 写 → embedding-worker → HNSW 索引")
        (conversation-jsonl-ingest   "PTY JSONL → conversation-logger → DB → briefing → embedding")
        (retrospective-trigger       "会话结束信号 → retro-worker → retrospective_results")
        (context-assembly            "LLM 调用前 → ContextPipeline → KB + conversations 拼 prompt")
        (project-init                "mission_project init → projects row + intent_path 解析 + 初始 lisp-survey"))))

) ;; end intent missiond-v2
