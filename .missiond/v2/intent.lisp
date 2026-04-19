;; ══════════════════════════════════════════════════════
;; MissionD — Intent v2
;; 按指挥官心智模型收拢: 六大板块
;;   一 · 记忆          — 系统记得什么
;;   二 · worker        — PTY + LLM 接入 + 21 后台 worker + 编排
;;   三 · 工具          — 对外暴露的能力
;;   四 · 事件总线      — 进程内神经网络(入点 / 核心 / 出点)
;;   五 · 意图层        — 系统的自我描述
;;   六 · 系统层        — DB CRUD + 类型 + 传输 + 启动 + 观测 + 工具
;; ══════════════════════════════════════════════════════

(intent missiond-v2
  (version "v2-draft")
  (granularity L2-Topology)
  (created "2026-04-19")
  (parent "intent.lisp (v1, 27 个分文件)")
  (note "v2 按概念重组,不按物理代码层。v1 保留为历史参考。")


  ;; ═══════════════════════════════════════════════════
  ;;  一 · 记忆 (Memory)
  ;;  系统的长期记忆 — 持久化在 PostgreSQL 的一切
  ;; ═══════════════════════════════════════════════════
  (pillar memory
    (purpose "一切需要跨会话保留的数据: 知识 / 会话 / 决策 / 复盘 / 审计 / 向量")
    (storage "PostgreSQL via sqlx::PgPool")
    (gateway "crates/missiond-core/src/db/ — 唯一 DB 入口")

    (component knowledge-base
      (desc "语义级记忆,按 category 组织 (architecture/bugfix/policy/preference/memory 等 40+ 类)")
      :tables "knowledge, knowledge_categories"
      :search "向量 + 全文 + 标签 三路融合")

    (component conversations
      (desc "所有 Claude / Codex / Gemini 会话的原始记录 + 分析产物")
      :tables "conversations, conversation_messages, conversation_turns, message_narrations")

    (component retrospectives
      (desc "会话结束后的复盘分析与决策提取")
      :tables "retrospective_results")

    (component audit
      (desc "全链路审计 + 决策统计 + LLM 调用追踪")
      :tables "system_audit, decision_log, llm_traces")

    (component timeline
      (desc "统一时间线事件流,供 UI / 查询 / 回放")
      :tables "system_timeline")

    (component embeddings
      (desc "向量表示,用于语义搜索")
      :tables "conversation_topic_vectors, knowledge 的 embedding 列"
      :provider "千问 qwen3 via SonnetGateway (禁止降级兜底)"))


  ;; ═══════════════════════════════════════════════════
  ;;  二 · worker (Worker Layer)
  ;;  MissionD 如何驱动外部 / 后台计算
  ;;  = 三种传输介质 (PTY / LLM API / 本地) + 统一编排
  ;; ═══════════════════════════════════════════════════
  (pillar worker
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
        (desc "按 slot 角色驱动对应 PTY 控制器,代码中 CC/Gemini 两类控制器")
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
        (desc "Claude Sonnet 直接 API — embedding / translation / briefing / retro 用")
        :target "llm/sonnet_gateway.rs"
        :routes-to "Anthropic API"
        :used-by "embedding / translation / briefing / arch-maintenance / retro workers")

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

    ;; ── 2.3 任务板 (Board): 工作分发队列 ──
    ;; Claude Code / 人类投任务, worker / autopilot 拉任务 — worker 的主要输入口
    (section board
      (desc "系统的任务队列 — 所有可分发工作的权威存储,worker 的唯一输入面")
      :v1-cross-ref "intent-db.lisp / intent-pillar-state-machines.lisp / intent-mcp-defs.lisp / intent-pillar-engines.lisp"

      (component data-model
        (desc "board_tasks 表 27 列,按关注点分组")
        :table "board_tasks"
        :access-layer "pillar 六 6.1 BoardStore trait + db/gen_board.rs"
        :types "pillar 六 6.2 BoardTask + BoardTaskStatus + FlowContext"

        (fields-by-concern
          (identity   "id (TaskId PK) / task_id")
          (content    "title / description / category / priority")
          (lifecycle  "status / retry_count / max_retries / timeout_secs")
          (claim      "claim_executor_id / claim_executor_type / claimed_at / lease_expires_at")
          (hierarchy  "parent_id (FK) / depends_on (JSONB Vec<TaskId> — DAG)")
          (execution  "auto_execute / prompt_template / assignee (slot 提示)")
          (flow       "flow_template / flow_phase / flow_context (JSON)")
          (scope      "project / server / context_intent")
          (dedup      "dedupe_key — 同 key 最多一个 open")
          (ui         "hidden / order_idx / notes_count / due_date"))

        (indexes 5
          (idx-status   :on "status"     :used-by "列表 / 仪表盘过滤")
          (idx-parent   :on "parent_id"  :used-by "层级遍历")
          (idx-category :on "category"   :used-by "活动分类")
          (idx-dedupe   :on "dedupe_key" :used-by "幂等查找")
          (idx-order    :on "order_idx"  :used-by "UI 稳定排序")))

      (component state-machine
        (desc "BoardTaskStatus — 7 态有限状态机")
        :v1-cross-ref "intent-pillar-state-machines.lisp"

        (states 7
          (open      "初始态,可被 claim")
          (running   "已占用,执行中")
          (verifying "执行完待确认(engineering flow 专用)")
          (done      "成功终态")
          (blocked   "依赖未满足 / 手动阻塞")
          (failed    "错误终态")
          (skipped   "主动跳过终态"))

        (transitions
          "open → running     : claim (CAS claim_executor_id)"
          "open → blocked     : check_dependencies 失败"
          "running → verifying: engineering flow 执行完待审"
          "running → done     : executor 报成功"
          "running → failed   : executor 报失败 / lease 超时"
          "blocked → open     : 上游依赖解除"
          "terminal → open    : mission_board_retry (reset + retry_count++)")

        :atomicity "open→running 是 SQL CAS,保证并发独占"
        :terminal-states "done / failed / skipped — 不可前转,只能 retry"
        :recovery "lease_expires_at 超期 → recover_stale_running_tasks() 强制回 open")

      (component operations
        (desc "核心 lifecycle 操作,全部走 BoardStore trait")

        (op create
          :method "create_board_task(input) → BoardTask"
          :idempotency "dedupe_key 已存在 open/running/blocked → 返回旧值,不建新")
        (op claim
          :method "claim_board_task(id, executor_id, executor_type) → Option<BoardTask>"
          :cas "UPDATE WHERE status='open' AND claim_executor_id IS NULL"
          :callers "autopilot / 手动 dispatcher / MCP mission_board_claim")
        (op update
          :method "update_board_task(id, input) → Option<BoardTask>"
          :features "选择性字段合并 / 终态时自动清 claim_executor_*")
        (op decompose
          :method "mission_board_decompose(taskId, slotId, hints) → {parent, children}"
          :flow "派 slot 分析 → 结构化子任务计划 → 按 depends_on DAG 链接")
        (op retry
          :method "retry_board_task(task_id, reset_downstream: bool) → Vec<TaskId>"
          :cascade "reset_downstream=true: BFS find_downstream_tasks 递归重置")
        (op note-add
          :method "add_board_task_note(taskId, content, noteType, author?) → BoardTaskNote"
          :side-effect "notes_count++,不改 status")
        (op delete
          :method "delete_board_task(id) → i64"
          :cascade "FK ON DELETE CASCADE — 子任务 + notes 全删"))

      (component events-emitted
        (desc "board 变动发出的 DaemonEvent — 骨干在 pillar 四 4.1/4.2,语义归此")
        :variants "BoardTaskCreated / BoardTaskStatusChanged / BoardTaskNoteAdded / BoardTaskClaimed / BoardTaskDeleted / BoardTaskUpdated"
        :emitted-by "MCP handler (mission_board_*) + autopilot engine"
        :consumers "前端 WS 实时显示 + Timeline Writer 持久化 system_timeline (v1 未声明专属 worker consumer)")

      (component autopilot-integration
        (desc "autopilot engine — board 的主要 worker 侧消费者")
        :target "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
        :tick-pipeline "memory-scheduler → extraction-check → board-task-dispatch → flow-progression → supervision-check"
        :dispatch-logic
          ("list_autopilot_tasks: WHERE auto_execute=1 AND status='open' ORDER BY (assignee 存在) → order_idx"
           "claim_board_task(id, autopilot_id, 'pty_slot') 原子占用"
           "claim 成功 → status→running + 派给 assignee 或自动选 slot"
           "list_running_autopilot_tasks: 监控已 claim 任务的租约"
           "lease 超期 → recover_stale_running_tasks 强制 reset")
        :tick-interval "5-10s 可配")

      (mcp-tool-surface 8
        (desc "对外 MCP 工具 — schema 归 pillar 三 tools,实现 crates/missiond-mcp/src/tools/knowledge/board.rs")
        (tools
          (mission_board_query     :actions "list / get / search / summary / clear_done")
          (mission_board_create    :desc "新建 — 可设 dedupe_key / parentId / dependsOn / autoExecute")
          (mission_board_update    :desc "选择性字段更新 / 批量 ids / 终态自动清 claim")
          (mission_board_delete    :desc "级联删除(子任务 + notes)")
          (mission_board_claim     :desc "CAS 原子占用")
          (mission_board_note_add  :desc "附注,不改 status")
          (mission_board_retry     :desc "reset→open + 可选级联重置下游")
          (mission_board_decompose :desc "派 slot 分析 → 生成子任务 DAG"))
        :caller-mix "用户(CLI) / autopilot / slot executor / flow engine / KB worker")

      (invariants 9
        (atomic-claim             "CAS guard,两个 executor 不能同时占一任务")
        (ownership-exclusion      "running 任务 claim_executor_id 恒一")
        (lease-recovery           "executor 崩溃 → lease 超期 → 自动 reset 回 open")
        (dag-consistency          "check_dependencies 遇 failed/skipped 上游即 blocked")
        (terminal-irreversibility "done/failed/skipped 只能 retry,不能直接前转")
        (cascade-deletion         "parent 删除 → 子任务 + notes 全删,无孤儿")
        (deduplication            "dedupe_key 非空时,同 key 最多一个 open 任务")
        (ordering                 "order_idx = MAX+1 on create,UI 稳定排序")
        (flow-context-persistence "flow_context JSON 独立于 status 变动存续")))

    ;; ── 2.4 后台 worker 集群: 21 个计算租户 ──
    (section workers 21
      (desc "反应式 + 定时 + 外部触发的后台计算单元,按执行介质分组")
      :target "crates/missiond-daemon/src/workers/"

      (group sonnet 6
        :examples "embedding / translation / briefing / arch-maintenance / retro / lisp-survey"
        :routes-via "SonnetGateway (直接 API)"
        :target "workers/sonnet/")
      (group codex 2
        :examples "step-narrator / vision"
        :routes-via "Claude Code PTY via slot_orchestrator/cc_controller"
        :target "workers/codex/")
      (group gemini 1
        :examples "strategy"
        :routes-via "Gemini CLI PTY via slot_orchestrator/gemini_controller"
        :target "workers/gemini/")
      (group local 12
        :examples "conversation-logger / conversation-organizer / pty-event / tagger-chunker / experience-harvester / reconcile / gemini-reconcile / ast-sync / code-prefetch / codex-ingestion / gemini-logger / xjpcode-briefing"
        :routes-via "纯本地计算,无 LLM 依赖"
        :target "workers/local/"
        :note "数量最多,承担 JSONL 摄入 / 分块 / 打标 / 时间线同步 / 外部状态对账"))

    ;; ── 2.5 编排: 生命周期 + 级联控制 ──
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

        :project-pause-note "is_project_paused(id) 由 handler 独立检查,不属于 is_effectively_paused() 的 worker 级联 — 项目控制数据流,不控制 worker")))


  ;; ═══════════════════════════════════════════════════
  ;;  三 · 工具 (Tools)
  ;;  MCP 协议 + 对外暴露的全部能力
  ;; ═══════════════════════════════════════════════════
  (pillar tools
    (purpose "通过 MCP JSON-RPC 协议暴露给 Claude Code / 其他 Agent 的能力集")

    (component mcp-server
      (desc "stdio JSON-RPC 服务器,MCP 协议入口")
      :target "crates/missiond-mcp")

    (component dispatch
      (desc "请求 → 域 → handler 的路由分派")
      :target "crates/missiond-daemon/src/infra/mcp_client.rs")

    (component tool-schema
      (desc "所有工具的 JSON Schema 声明(67+ 个工具,4 大域)")
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
  ;;  四 · 事件总线 (Event Bus)
  ;;  进程内神经网络 — 入点 / 处理分析核心 / 出点
  ;; ═══════════════════════════════════════════════════
  ;; 详细定义已抽离到独立文件以便并行加载和聚焦浏览
  (pillar event-bus
    :file "intent-event-bus.lisp"
    :purpose "两层事件模型 + Timeline Writer 枢纽 + 双通道广播扇出"
    :structure "4.1 入点 (Ingress) / 4.2 核心 (Core) / 4.3 出点 (Egress)"
    :note "worker 集群 / worker-registry / control-tree 在 pillar 二;此 pillar 只管事件基础设施")


  ;; ═══════════════════════════════════════════════════
  ;;  五 · 意图层 (Intent Layer)
  ;;  系统的自我描述 + 自感知 + 自演化
  ;; ═══════════════════════════════════════════════════
  (pillar intent-layer
    (purpose "元层: 系统如何描述自己,如何感知变化,如何演进")

    (component intent-files
      (desc ".missiond/*.lisp 意图声明,按主题拆分并行加载")
      :granularities "L1-Blueprint / L2-Topology / L3-Implementation"
      :count "27 files (v1) + this v2 draft")

    (component intent-graph
      (desc "文件间 module-link 关系,构成有向图,可供可视化 / 治理")
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
      :target "forge-daemon/src/governance.rs"))


  ;; ═══════════════════════════════════════════════════
  ;;  六 · 系统层 (System Layer)
  ;;  DB CRUD + 类型 + 传输 + 启动 + 观测 + 工具 — 支撑一切的底座
  ;; ═══════════════════════════════════════════════════
  (pillar system-layer
    (purpose "无业务语义的底层基础设施 — 任何业务 pillar 都经由它触达外部世界 (DB / 网络 / 时间 / 文件)")

    ;; ── 6.1 DB 访问层 ──
    (section db
      (desc "PgPool + 统一 trait 抽象,一切持久化的唯一入口")
      :target "crates/missiond-core/src/db/"
      :v1-cross-ref "intent-pillar-db-core.lisp / intent-db*.lisp"

      (component mission-store
        (desc "顶层 super-trait,聚合 13 个领域 store")
        :target "crates/missiond-core/src/db/traits.rs (~750 行)"
        :invariant "其他 crate 只依赖 trait 不依赖实现 — 可切换 PG / SQLite"
        (stores 13 "Conversation / Message / ToolCall / Event / Retrospective / Vision / Knowledge / Board / Timeline / Slot / Skill / Observability / Project"))

      (component pg-store
        (desc "生产实现:原生 async sqlx")
        :target "crates/missiond-core/src/db/pg_*/ (15 文件)")

      (component sqlite-store
        (desc "遗留兼容实现 (spawn_blocking 包装),生产路径已迁 PG")
        :status "⚠ deprecated — 不再维护")

      (component migrations
        (desc "schema 演进 — daemon 启动时 sqlx::migrate! 自动跑")
        :target "crates/missiond-core/migrations/"
        :count "16 migrations (含 seed + backfill)"
        :entry "启动 phase 1")

      (component gen-crud
        (desc "Forge 冲压生成的 CRUD 代码 — 按领域分文件")
        :target "crates/missiond-core/src/db/gen_{kb,board,conversation,compute,knowledge,misc,pipeline,skill,audit}.rs"
        :pattern "模式驱动 — 改需求改 lisp 不改代码"))

    ;; ── 6.2 核心共享类型 ──
    (section core-types
      (desc "跨 crate 共享的枚举 + 结构体 — 单一真理源")
      :target "crates/missiond-core/src/types/gen_types.rs (Forge-generated)"
      :v1-cross-ref "intent-types.lisp"

      (enums "BoardTaskStatus / EngineeringPhase / TaskStatus / EventType / AsyncJobStatus / AgentQuestionStatus / IncidentSeverity / IncidentSource / CliEngine / Lifecycle / SlotTrait")
      (structs "BoardTask / ConversationMessage / KnowledgeEntry / Task / InboxMessage / TaskEvent / AgentQuestion / IncidentRow / DynamicSlot / SkillTopic / ToolCallRecord")

      :serde "derive Serialize/Deserialize + as_str/from_str — DB / IPC / JSON-RPC 共享"
      :authority "枚举定义合法状态迁移 (例: BoardTaskStatus Open→Running→Done,禁跳)")

    ;; ── 6.3 进程与传输 ──
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

    ;; ── 6.4 RPC Gateway ──
    (section rpc-gateway
      (desc "JSON-RPC 服务器 — stdio(MCP 协议)+ IPC(daemon)双传输")
      :target "crates/missiond-mcp/src/gen_server.rs (Forge-generated)"
      :v1-cross-ref "intent-rpc-gateway.lisp"

      (methods "initialize / notifications/initialized / tools/list / tools/call / ping")

      (dispatch
        :rule "数据驱动: tool_name → handler 映射,非硬编码 match"
        :scope "78 tools × 8 groups (schema 归 pillar 三)")

      (error-codes "UNKNOWN_TOOL / UNKNOWN_ACTION / MISSING_PARAM / INVALID_PARAM / NOT_FOUND / PERMISSION_DENIED / IPC_TIMEOUT / SPAWN_FAILED / DB_ERROR")

      :role "纯 plumbing — pillar 三 持有 tool schema,这里只负责路由 + 错误码")

    ;; ── 6.5 Project Registry ──
    (section project-registry
      (desc "CWD → project_id 解析 + 多项目隔离基础设施")
      :target "crates/missiond-daemon/src/state.rs (SharedProjectRegistry = Arc<RwLock<ProjectRegistry>>)"
      :v1-cross-ref "intent-pillar-db-core.lisp"
      :moved-from "pillar 五 (原误归意图层,实为系统层)"

      (component projects-table
        :migration "20260410000000_projects.sql + 20260410200000 seed"
        :columns "id(PK) / path(unique) / intent_path / active / slots(array) / github_url / timestamps"
        :backfill "conversations.project_id 按 path 匹配回填")

      (component in-memory-index
        (desc "启动时从 DB 加载到内存,longest-prefix 路径匹配")
        :methods "resolve(cwd) / exclusive_slots / list_active"
        :invariant "只在启动时写,之后只读")

      :scope-semantics "NULL project_id = 全局 (跨项目可见,如公共 KB);非 NULL = 项目私有"
      :note "项目是基础设施概念(路径 / slot 归属 / 激活)不是领域实体")

    ;; ── 6.6 观测基础设施 ──
    (section observability
      (desc "低层事件与度量的存储 + 查询底座 — 非业务数据,是系统自证")
      :v1-cross-ref "intent-pillar-db-observability.lisp / intent-db-misc.lisp"

      (component system-timeline-table
        (desc "系统事件日志 — pillar 四 的 DB 出口,此处描述表结构")
        :table "system_timeline"
        :columns "seq(autoincrement) / trace_id / span_id / parent_span_id / event_type / summary / payload / created_at"
        :queries "by trace_id / by event_type / 分层抽样 / FTS 搜索"
        :ttl "7 天自动清理"
        :see-also "pillar 四 4.2 timeline-writer (写入者) + 4.3 db-persistence (出口)")

      (component gemini-requests
        (desc "Gemini API 调用性能日志")
        :table "gemini_requests"
        :columns "id / caller / model / prompt_chars / duration_ms / status / error_msg / created_at")

      (component token-usage-ledger
        (desc "LLM 调用成本追踪 — 按模型 / slot / 会话聚合")
        :table "token_usage_ledger"
        :aggregations "by model / by slot / by conversation / time-series")

      (component incidents
        (desc "告警 / 异常聚合表,带去重窗口")
        :table "incidents"
        :dedup "dedupe_key + 时间窗口"
        :upstream "incident-tx MPSC (pillar 四 4.1 入点)")

      :invariant "审计表 append-only,UPDATE 只改状态列不改身份列;清理粗粒度(按时间 DELETE)")

    ;; ── 6.7 纯工具模块 ──
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

) ;; end intent missiond-v2
