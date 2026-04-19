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
  ;;  系统的长期记忆 — 详见独立 lisp
  ;; ═══════════════════════════════════════════════════
  ;; 详细规格在 intent-memory.lisp (草稿),本处只作导航摘要
  (pillar memory
    :file ".missiond/v2/intent-memory.lisp"
    :status "草稿 v0.4.4 — 4 成熟模块 + 精简 system-support; action/instruction specs 已迁 pillar 五"
    :paradigm "4 mature modules (project-management / board / kb-manager / conversation-logs) 自治 + 系统支持 + 横切"

    (purpose "系统长期记忆: 4 个业务模块自治管理自己的表 + 底层系统支持层 + 横切")
    (storage "PostgreSQL via sqlx::PgPool")
    (gateway "crates/missiond-core/src/db/ — 唯一 DB 入口")

    (migrated-out
      "embedding-provider → pillar 二 2.2 sonnet-gateway (qwen3 双角色)"
      "gen-crud (Forge 冲压) → pillar 二 2.5 code-generation"
      "search-engines → pillar 二 2.6 search-engines (搜索是计算不是数据)"
      "event-bus 4 表 → pillar 四 §4.6 persistence-layer (event_log / subscriptions / blob_storage / dlq)")

    ;; ── 结构 (4 模块 + 1 分类 + 横切) ──
    (structure
      ;; 成熟模块 — 各自 in/core/out + 显式 module-tables-owned
      (module project-management
        :desc   "项目作用域: 注册 + per-project 代码快照 intent.lisp 文件 + skills"
        :target "intent-memory.lisp :: module project-management"
        :owned-tables 5
        :v0.4.4-change "specs 4 表 (intent/plan/workflow/user_intents) 迁到 pillar 五 action-instruction-specs"
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
        :desc   "三引擎(Claude Code/Gemini/Codex)会话记录 + 摘要/翻译/打标/复盘"
        :target "intent-memory.lisp :: module conversation-logs"
        :owned-tables 14
        :non-db-source "PTY JSONL (~/.claude/projects/{encoded}/*.jsonl)"
        :mcp    "mission_conversation_* / mission_retrospective_manage / mission_audit / mission_llm_trace")

      ;; 分类 — 系统支持层
      (category system-support
        :desc   "系统级基础表 — 观测 + 图片缓存 + 基建 + 运行时游标 + legacy"
        :target "intent-memory.lisp :: category system-support"
        :owned-tables 20
        :content "global-observability / vision-assets / infrastructure / compute-runtime / legacy"
        :v0.4.1-change "-1 (system_timeline 合并进 pillar 四 event_log 作 SSOT)")

      ;; 横切能力
      (cross-cutting
        :desc   "db-trait-abstraction / retention-policy / migrations-runner"
        :target "intent-memory.lisp :: cross-cutting"))

    ;; ── 关键基础设施位置 (快速导航) ──
    (key-locations
      (mission-store-trait    :at "crates/missiond-core/src/db/traits.rs  — 13 store 超 trait")
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

    :maturity-ladder "3 module + 2 category + cross-cutting; 某分类稳定后可晋升为 module"
    :note "此 pillar 只列导航; 详细模块内部 in/core/out 在 intent-memory.lisp")



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
        (desc "Claude Sonnet API + 千问 qwen3 embedding provider (同一 gateway 双角色)")
        :target "llm/sonnet_gateway.rs"
        :routes-to "Anthropic API (chat) + 千问 qwen3 API (embedding)"
        :used-by "embedding / translation / briefing / arch-maintenance / retro workers"
        :embedding-invariant "qwen3 是唯一 embedding provider, 禁止降级兜底 — 失败直接报错"
        :migrated-in "memory v0.2 :: 1.4 cross-cutting :: embedding-provider (2026-04-19)")

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

    ;; ── 2.3 后台 worker 集群: 21 个计算租户 ──
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
          :provider "2.2 llm-gateways :: sonnet-gateway (qwen3 路由)"
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
  ;;  四 · 事件总线 (Event Bus) — Log-as-Bus
  ;;  追加式日志即总线,7 步流水线处理 + tail-and-pull 订阅
  ;; ═══════════════════════════════════════════════════
  ;; 详细规格在独立的冻结 lisp(v1.1.0 锁定),本处只作导航摘要
  (pillar event-bus
    :file ".missiond/v2/intent-event-bus.lisp"
    :execution-log ".missiond/v2/intent-event-bus-execution.lisp"
    :lock-status "frozen v1.1.0 — ask-before-edit"
    :paradigm "Log-as-Bus(追加式日志是唯一真理源,不是 broadcast + 补漏)"

    (purpose "进程内神经网络 — 追加式日志 + 类型化 topic 路由 + 游标式订阅")

    (one-line-spec
      "DB seq + 12 domain topic + at-least-once + batch-ack cursor (双阈值) "
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
      (domain-types       :at "crates/missiond-core/src/event/events/ (12 个 domain enum)")
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
      :migrated-to   "v2: 12 domain enum + event_log 单一真理源 + Dispatcher live-only + 14 typed subscribers"
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
      :code "TBD — 目前 Claude Code 直接读, 无 daemon 侧 MCP manager"
      :future "未来可补 mission_global_instruction (read/edit/reload) MCP tool"
      :readers "Claude Code 每次会话启动"
      :writers "用户手动 / Claude Code Edit tool (文件层)"
      :status "文件层存在, daemon/MCP 层无 manager — 待实现"
      :cross-ref "项目级 <project>/CLAUDE.md 的 manager 在 memory pillar :: project-management :: helper project-claudemd-manager")

    ;; ═══════════════════════════════════════════════════
    ;; Action-Instruction Specs — 动作与指令规约
    ;;   (v0.4.4 从 memory pillar 迁入)
    ;;   区别:memory 只存'项目代码真实状态的 intent.lisp';
    ;;         本 section 管所有'描述动作和指令'的 DB 表 + 文件
    ;; ═══════════════════════════════════════════════════
    (section action-instruction-specs
      (desc "所有描述'应该做什么 / 如何做'的规约 — DB 表 + Lisp/YAML 文件")
      :migrated-from "memory pillar :: project-management (4 tables) + non-db-forms (3 variants + 1 form) in v0.4.4"
      :rationale "memory 记'是什么'(facts); 本层记'应该做什么'(prescriptions) — 分层原则"

      ;; ── DB 表 (4 张) ──
      (component intent-spec-db
        (desc "项目 intent 规约 DB 镜像 — 描述项目应该做什么")
        :table "intent"
        :migration "20260420000000_intent_plan_workflow.sql"
        :status "❌ schema-only — 无 Rust reader/writer"
        :moved-from "memory :: project-management :: module-tables-owned (v0.4.4)"
        :vs-per-project-intent "memory 里的 <project>/.missiond/intent.lisp 是 factual 代码快照; 本表是 instruction 规约")

      (component plan-spec-db
        (desc "执行计划 DB 表 — 描述 action 步骤")
        :table "plan"
        :status "❌ schema-only"
        :moved-from "memory (v0.4.4)")

      (component workflow-spec-db
        (desc "工作流模板 DB 表 — 可复用的 action 组合")
        :table "workflow"
        :status "❌ schema-only"
        :moved-from "memory (v0.4.4)")

      (component user-intents-db
        (desc "用户意图识别记录 — 解析用户指令后的结构化意图")
        :table "user_intents"
        :moved-from "memory (v0.4.4)")

      ;; ── Lisp / YAML 文件 (3 类) ──
      (component system-level-intent-files
        (desc "系统主架构 + pillar 级细节规约 Lisp 文件")
        :paths (".missiond/v2/intent.lisp 系统主架构"
                ".missiond/v2/intent-event-bus.lisp frozen v1.3.0"
                ".missiond/v2/intent-memory.lisp 草稿 v0.4.4"
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
          (desc "Lisp 方法论模板 — 人类 / agent 参考, 非运行时执行")
          :path ".missiond/workflows/*.lisp"
          :consumers "human + mission_intent tool + agent 参考"
          :granularity "抽象叙事 — phases / principles / anti-patterns / baseline-numbers / decision-authority"
          :examples "bus-refactor.lisp (11-phase 事件总线重构方法论)"
          :executability "✗ 非运行时执行, 纯文档")

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
          :future-possibility "若需要, 可用 Forge 从 Lisp 冲压 YAML (SSOT Lisp + 冲压副本), 当前不做"))

      ;; ── Manager ──
      (component specs-manager
        (desc "action/instruction specs 的读/写/reload — 大部分 TBD")
        :actions "read / write / reload / sync-with-file"
        :status "mostly TBD — DB 4 表是 schema-only, 无 Rust 实现"
        :files-status "intent/workflow/flow 文件层已有 readers (mission_intent / flow-engine-v2); writers 多为手动编辑"
        :cross-ref "memory :: project-management :: path project-code-snapshot (读 per-project 代码快照 FILE, 职责不同)"
        :future-work "要么实现 4 DB 表的 sync worker, 要么下次 migration DROP 以消除 dead schema")))


  ;; ═══════════════════════════════════════════════════
  ;;  六 · 系统层 (System Layer)
  ;;  类型 + 传输 + RPC Gateway + 工具 — 运行时底座 (DB / 观测 已迁入 pillar 一)
  ;; ═══════════════════════════════════════════════════
  (pillar system-layer
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
        :scope "78 tools × 8 groups (schema 归 pillar 三)")

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

) ;; end intent missiond-v2
